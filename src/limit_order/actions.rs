//! High-level, stateless limit-order action builders.
//!
//! These functions take a snapshot of on-chain state (`Option<&MakerBook>`)
//! plus the user's intent, and return the list of instructions to land plus
//! the resulting limit-order IDs. Async fetching is in `ArcherClient`.
//!
//! All actions follow the "read-modify-write" pattern: the caller fetches the
//! current `MakerBook` (if any), passes it in, gets back instructions, then
//! signs and sends.

use crate::onchain::{
    builders::{
        create_clear_book_instruction, create_close_maker_book_instruction,
        create_initialize_maker_book_instruction, create_maker_deposit_funds_instruction,
        create_maker_withdraw_funds_instruction, create_update_book_instruction, UpdateBookParams,
    },
    ArcherUnit, BaseLots, MakerBook, MakerDepositFundsParams, MakerLevel, MakerWithdrawFundsParams,
    QuoteLots,
};
use solana_program::{instruction::Instruction, pubkey::Pubkey};

use crate::config::MarketConfig;
use crate::error::{ArcherSDKError, SdkResult};
use crate::identity::{append_archer_account, append_authority, Identity};
use crate::math::ticks::price_to_ticks;
use crate::pda;

use super::book::{resolve_new_order, LocalBook};
use super::types::{LimitOrderId, NewLimitOrder};

/// Optional collateral movement bundled into a place/cancel call.
///
/// Used in two directions:
/// * **As a deposit** (in `build_place` / `build_replace_all`): the lots
///   are moved from the user's ATAs into the maker book. Pass `0` to skip
///   a side.
/// * **As a withdrawal** (in `build_cancel*` / `build_cancel_all` /
///   `build_close_book`): the lots are moved from the maker book to the
///   user's ATAs. `u64::MAX` on either field signals "drain all free
///   balance" — matching the program's semantics.
///
/// The struct is identical in both directions; semantics are determined by
/// the calling function.
#[derive(Debug, Clone, Copy)]
pub struct CollateralArgs {
    pub base_lots: u64,
    pub quote_lots: u64,
    pub maker_base_ata: Pubkey,
    pub maker_quote_ata: Pubkey,
}

/// What a single batched place/modify/cancel produced.
#[derive(Debug, Clone)]
pub struct LimitOrderActionResult {
    /// Ordered list of instructions to land in a single transaction.
    pub instructions: Vec<Instruction>,
    /// IDs of newly placed (or repriced) orders, in the same order as the
    /// caller's input slice. For pure cancels this is empty.
    pub placed_ids: Vec<LimitOrderId>,
    /// Next valid sequence number written to chain. Callers caching this can
    /// re-use it for follow-up calls instead of refetching.
    pub next_sequence_number: u64,
}

/// Place one or more limit orders. Bootstraps the MakerBook if needed.
///
/// Behaviour:
/// * If the user has no MakerBook on chain, prepends `InitializeMakerBook`.
/// * If `deposit` is supplied, inserts `MakerDepositFunds` between init and
///   `UpdateBook`. Required when the book lacks free collateral for the
///   requested orders.
/// * Anchor mid is `book.mid_price_ticks` if the book exists and has a
///   non-zero anchor; otherwise the first order's absolute price in ticks.
pub fn build_place(
    owner: impl Into<Identity>,
    market: &Pubkey,
    current_book: Option<&MakerBook>,
    orders: &[NewLimitOrder],
    deposit: Option<CollateralArgs>,
    config: &MarketConfig,
) -> SdkResult<LimitOrderActionResult> {
    let identity = owner.into();
    if orders.is_empty() {
        return Err(ArcherSDKError::EmptyOrderList);
    }

    let (anchor, mut local, current_seq, needs_init) = match current_book {
        Some(book) if book.mid_price_ticks != 0 => (
            book.mid_price_ticks,
            LocalBook::from_maker_book(book),
            book.last_updated_sequence_number,
            false,
        ),
        Some(book) => {
            // Book exists but mid is uninitialized (no levels ever written).
            // Use first order's price as the anchor.
            let anchor = price_to_ticks(orders[0].price, config)?;
            (
                anchor,
                LocalBook::new(anchor),
                book.last_updated_sequence_number,
                false,
            )
        }
        None => {
            let anchor = price_to_ticks(orders[0].price, config)?;
            (anchor, LocalBook::new(anchor), 0u64, true)
        }
    };

    let mut placed_ids = Vec::with_capacity(orders.len());
    for new in orders {
        let (id, size_lots) = resolve_new_order(new, config)?;
        local.place(id, size_lots)?;
        placed_ids.push(id);
    }

    let next_seq = current_seq + 1;
    let (maker_book_pda, _) = pda::derive_maker_book(market, &identity.maker());
    let mut instructions = Vec::with_capacity(3);

    if needs_init {
        instructions.push(append_authority(
            create_initialize_maker_book_instruction(
                identity.maker(),
                *market,
                crate::onchain::MAKER_KIND_LO,
            ),
            &identity,
        ));
    }

    if let Some(dep) = deposit {
        if dep.base_lots > 0 || dep.quote_lots > 0 {
            instructions.push(append_authority(
                create_maker_deposit_funds_instruction(
                    MakerDepositFundsParams {
                        base_lots: BaseLots::new(dep.base_lots),
                        quote_lots: QuoteLots::new(dep.quote_lots),
                    },
                    identity.maker(),
                    maker_book_pda,
                    *market,
                    config.base_mint,
                    config.quote_mint,
                    dep.maker_base_ata,
                    dep.maker_quote_ata,
                    config.base_vault,
                    config.quote_vault,
                    config.base_token_program,
                    config.quote_token_program,
                ),
                &identity,
            ));
        }
    }

    instructions.push(update_book_ix(
        &identity,
        *market,
        maker_book_pda,
        anchor,
        next_seq,
        &local,
    )?);

    Ok(LimitOrderActionResult {
        instructions,
        placed_ids,
        next_sequence_number: next_seq,
    })
}

/// Modify an existing limit order's price and/or size.
///
/// Returns the *new* `LimitOrderId` in `placed_ids[0]`. If `new_price` rounds
/// to the same tick offset as the old order, only the size changes and the ID
/// is preserved; otherwise the operation is semantically cancel + place.
pub fn build_modify(
    owner: impl Into<Identity>,
    market: &Pubkey,
    current_book: &MakerBook,
    id: LimitOrderId,
    new_price: f64,
    new_size: f64,
    config: &MarketConfig,
) -> SdkResult<LimitOrderActionResult> {
    let identity = owner.into();
    let anchor = current_book.mid_price_ticks;
    if anchor == 0 {
        return Err(ArcherSDKError::AnchorMidUninitialized);
    }

    let mut local = LocalBook::from_maker_book(current_book);
    // Verify the source order exists locally, then drop it.
    local.cancel(id)?;

    let new = NewLimitOrder {
        side: id.side,
        price: new_price,
        size: new_size,
    };
    let (new_id, new_size_lots) = resolve_new_order(&new, config)?;
    local.place(new_id, new_size_lots)?;

    let next_seq = current_book.last_updated_sequence_number + 1;
    let (maker_book_pda, _) = pda::derive_maker_book(market, &identity.maker());
    let ix = update_book_ix(&identity, *market, maker_book_pda, anchor, next_seq, &local)?;

    Ok(LimitOrderActionResult {
        instructions: vec![ix],
        placed_ids: vec![new_id],
        next_sequence_number: next_seq,
    })
}

/// Cancel one or more limit orders atomically. All IDs must currently exist.
/// Optionally bundles a withdraw of newly freed collateral.
pub fn build_cancel(
    owner: impl Into<Identity>,
    market: &Pubkey,
    current_book: &MakerBook,
    ids: &[LimitOrderId],
    withdraw: Option<CollateralArgs>,
    config: &MarketConfig,
) -> SdkResult<LimitOrderActionResult> {
    let identity = owner.into();
    if ids.is_empty() {
        return Err(ArcherSDKError::EmptyOrderList);
    }
    let anchor = current_book.mid_price_ticks;
    if anchor == 0 {
        return Err(ArcherSDKError::AnchorMidUninitialized);
    }

    let mut local = LocalBook::from_maker_book(current_book);
    for id in ids {
        local.cancel(*id)?;
    }

    let next_seq = current_book.last_updated_sequence_number + 1;
    let (maker_book_pda, _) = pda::derive_maker_book(market, &identity.maker());

    let mut instructions = Vec::with_capacity(2);
    instructions.push(update_book_ix(
        &identity,
        *market,
        maker_book_pda,
        anchor,
        next_seq,
        &local,
    )?);

    append_withdraw(
        &mut instructions,
        &identity,
        market,
        maker_book_pda,
        withdraw,
        config,
    );

    Ok(LimitOrderActionResult {
        instructions,
        placed_ids: Vec::new(),
        next_sequence_number: next_seq,
    })
}

/// Cancel every active order via `ClearBook`. Cheaper than rewriting the whole
/// book with `UpdateBook` when the user wants a wipe.
pub fn build_cancel_all(
    owner: impl Into<Identity>,
    market: &Pubkey,
    current_book: &MakerBook,
    withdraw: Option<CollateralArgs>,
    config: &MarketConfig,
) -> SdkResult<LimitOrderActionResult> {
    let identity = owner.into();
    let next_seq = current_book.last_updated_sequence_number + 1;
    let (maker_book_pda, _) = pda::derive_maker_book(market, &identity.maker());

    let mut instructions = Vec::with_capacity(2);
    instructions.push(append_archer_account(
        create_clear_book_instruction(identity.authority(), maker_book_pda, next_seq),
        &identity,
    ));

    append_withdraw(
        &mut instructions,
        &identity,
        market,
        maker_book_pda,
        withdraw,
        config,
    );

    Ok(LimitOrderActionResult {
        instructions,
        placed_ids: Vec::new(),
        next_sequence_number: next_seq,
    })
}

/// Replace the user's entire active order set in one atomic `UpdateBook`.
///
/// Useful for portfolio-style "this is my new desired state" callers.
/// Anchor mid is taken from the existing book if present; otherwise from the
/// first order's price.
pub fn build_replace_all(
    owner: impl Into<Identity>,
    market: &Pubkey,
    current_book: Option<&MakerBook>,
    orders: &[NewLimitOrder],
    deposit: Option<CollateralArgs>,
    config: &MarketConfig,
) -> SdkResult<LimitOrderActionResult> {
    let identity = owner.into();
    if orders.is_empty() {
        return Err(ArcherSDKError::EmptyOrderList);
    }

    let (anchor, current_seq, needs_init) = match current_book {
        Some(book) if book.mid_price_ticks != 0 => (
            book.mid_price_ticks,
            book.last_updated_sequence_number,
            false,
        ),
        Some(book) => (
            price_to_ticks(orders[0].price, config)?,
            book.last_updated_sequence_number,
            false,
        ),
        None => (price_to_ticks(orders[0].price, config)?, 0u64, true),
    };

    let mut local = LocalBook::new(anchor);
    let mut placed_ids = Vec::with_capacity(orders.len());
    for new in orders {
        let (id, size_lots) = resolve_new_order(new, config)?;
        local.place(id, size_lots)?;
        placed_ids.push(id);
    }

    let next_seq = current_seq + 1;
    let (maker_book_pda, _) = pda::derive_maker_book(market, &identity.maker());

    let mut instructions = Vec::with_capacity(3);
    if needs_init {
        instructions.push(append_authority(
            create_initialize_maker_book_instruction(
                identity.maker(),
                *market,
                crate::onchain::MAKER_KIND_LO,
            ),
            &identity,
        ));
    }
    if let Some(dep) = deposit {
        if dep.base_lots > 0 || dep.quote_lots > 0 {
            instructions.push(append_authority(
                create_maker_deposit_funds_instruction(
                    MakerDepositFundsParams {
                        base_lots: BaseLots::new(dep.base_lots),
                        quote_lots: QuoteLots::new(dep.quote_lots),
                    },
                    identity.maker(),
                    maker_book_pda,
                    *market,
                    config.base_mint,
                    config.quote_mint,
                    dep.maker_base_ata,
                    dep.maker_quote_ata,
                    config.base_vault,
                    config.quote_vault,
                    config.base_token_program,
                    config.quote_token_program,
                ),
                &identity,
            ));
        }
    }
    instructions.push(update_book_ix(
        &identity,
        *market,
        maker_book_pda,
        anchor,
        next_seq,
        &local,
    )?);

    Ok(LimitOrderActionResult {
        instructions,
        placed_ids,
        next_sequence_number: next_seq,
    })
}

/// Tear down an empty book: ClearBook → optional Withdraw → CloseMakerBook.
///
/// `ClearBook` is always included so the caller doesn't have to verify the
/// book is already empty.
pub fn build_close_book(
    owner: impl Into<Identity>,
    market: &Pubkey,
    current_book: &MakerBook,
    withdraw: Option<CollateralArgs>,
    config: &MarketConfig,
) -> SdkResult<LimitOrderActionResult> {
    let identity = owner.into();
    let next_seq = current_book.last_updated_sequence_number + 1;
    let (maker_book_pda, _) = pda::derive_maker_book(market, &identity.maker());

    let mut instructions = Vec::with_capacity(3);
    instructions.push(append_archer_account(
        create_clear_book_instruction(identity.authority(), maker_book_pda, next_seq),
        &identity,
    ));
    append_withdraw(
        &mut instructions,
        &identity,
        market,
        maker_book_pda,
        withdraw,
        config,
    );
    instructions.push(append_authority(
        create_close_maker_book_instruction(identity.maker(), *market),
        &identity,
    ));

    Ok(LimitOrderActionResult {
        instructions,
        placed_ids: Vec::new(),
        next_sequence_number: next_seq,
    })
}

/// Compute the exact collateral (in lots) the program will lock for an
/// intended set of limit orders. Matches `update_book`'s solvency check
/// 1:1, so depositing exactly this amount is sufficient to back the orders.
///
/// Includes:
/// * Ask side: sum of `size_in_base_lots`.
/// * Bid side: ceiling per-level `compute_quote_lots_ceiling(size, abs_price)`.
/// * **Maker-fee buffer** on the bid total when `maker_fee_ppm > 0`,
///   computed as `ceil(quote * maker_fee_ppm / 1_000_000)` — same formula
///   the program uses inside `update_book`. Rebates (negative fee) need
///   no buffer.
///
/// Callers can still pad a few lots on top to absorb rounding races against
/// concurrent fills, but the returned numbers are not under-counts.
pub fn compute_required_collateral(
    orders: &[NewLimitOrder],
    config: &MarketConfig,
) -> SdkResult<(u64, u64)> {
    let mut base = 0u64;
    let mut quote = 0u64;
    for new in orders {
        let (id, size_lots) = resolve_new_order(new, config)?;
        match id.side {
            crate::onchain::Side::Ask => base = base.saturating_add(size_lots),
            crate::onchain::Side::Bid => {
                let quote_lots = config.quote_lots_ceil(size_lots, id.price_ticks);
                quote = quote.saturating_add(quote_lots);
            }
        }
    }

    // Mirror update_book's fee buffer: only positive maker fees require it.
    // fee_buffer = ceil(quote * maker_fee_ppm / 1_000_000).
    if config.maker_fee_ppm > 0 && quote > 0 {
        let fee_ppm = config.maker_fee_ppm as u128;
        let buffer_u128 = (quote as u128)
            .checked_mul(fee_ppm)
            .and_then(|v| v.checked_add(999_999))
            .ok_or(ArcherSDKError::ArithmeticOverflow {
                operation: "compute_required_collateral: fee buffer",
            })?
            / 1_000_000u128;
        let buffer =
            u64::try_from(buffer_u128).map_err(|_| ArcherSDKError::ArithmeticOverflow {
                operation: "compute_required_collateral: fee buffer overflow",
            })?;
        quote = quote
            .checked_add(buffer)
            .ok_or(ArcherSDKError::ArithmeticOverflow {
                operation: "compute_required_collateral: fee buffer add",
            })?;
    }

    Ok((base, quote))
}

/// Render a `LocalBook` to its sorted `MakerLevel` arrays and wrap into an
/// `UpdateBook` instruction.
fn update_book_ix(
    identity: &Identity,
    market: Pubkey,
    maker_book_pda: Pubkey,
    anchor_mid_ticks: u64,
    sequence_number: u64,
    local: &LocalBook,
) -> SdkResult<Instruction> {
    let (bid_levels, ask_levels): (Vec<MakerLevel>, Vec<MakerLevel>) = local.to_maker_levels()?;
    Ok(append_archer_account(
        create_update_book_instruction(
            identity.authority(),
            market,
            maker_book_pda,
            UpdateBookParams {
                mid_price_ticks: anchor_mid_ticks,
                bid_levels,
                ask_levels,
                sequence_number,
            },
        ),
        identity,
    ))
}

fn append_withdraw(
    instructions: &mut Vec<Instruction>,
    identity: &Identity,
    market: &Pubkey,
    maker_book_pda: Pubkey,
    withdraw: Option<CollateralArgs>,
    config: &MarketConfig,
) {
    let Some(w) = withdraw else { return };
    if w.base_lots == 0 && w.quote_lots == 0 {
        return;
    }
    instructions.push(append_authority(
        create_maker_withdraw_funds_instruction(
            MakerWithdrawFundsParams {
                base_lots: BaseLots::new(w.base_lots),
                quote_lots: QuoteLots::new(w.quote_lots),
            },
            identity.maker(),
            maker_book_pda,
            *market,
            config.base_mint,
            config.quote_mint,
            w.maker_base_ata,
            w.maker_quote_ata,
            config.base_vault,
            config.quote_vault,
            config.base_token_program,
            config.quote_token_program,
        ),
        identity,
    ));
}
