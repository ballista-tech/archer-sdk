//! High-level, stateless limit-order action builders.
//!
//! Every book write here is an `UpdateBookLimit`: limit orders are post-only
//! by construction and always carry the market's registered maker books
//! ([`PostOnly`]). `UpdateBook` is the market-maker path and is not used for
//! limit orders.
//!
//! These functions take a snapshot of on-chain state (`Option<&MakerBook>`)
//! plus the user's intent, and return the list of instructions to land plus
//! the resulting limit-order IDs. Async fetching is in `ArcherClient`.
//!
//! All actions follow the "read-modify-write" pattern: the caller fetches the
//! current `MakerBook` (if any), passes it in, gets back instructions, then
//! signs and sends. The write is a list of per-level compare-and-set ops
//! ([`LevelOp`]) derived from the snapshot, so a fill that lands between the
//! read and the write fails the instruction (`LevelSizeMismatch`, 527) and the
//! caller refetches and rebuilds — nothing is ever written back over a fill.
//! An instruction carries at most 32 ops; larger batches are split across
//! consecutive `UpdateBookLimit`s with consecutive sequence numbers.

use crate::onchain::{
    builders::{
        create_clear_book_instruction, create_close_maker_book_instruction,
        create_initialize_maker_book_instruction, create_maker_deposit_funds_instruction,
        create_maker_withdraw_funds_instruction, create_update_book_limit_instruction,
    },
    ArcherUnit, BaseLots, LevelOp, MakerBook, MakerDepositFundsParams, MakerWithdrawFundsParams,
    QuoteLots, MAX_LEVEL_OPS,
};
use solana_program::{instruction::Instruction, pubkey::Pubkey};

use crate::config::MarketConfig;
use crate::error::{ArcherSDKError, SdkResult};
use crate::identity::Identity;
use crate::pda;

use super::book::{resolve_new_order, LocalBook};
use super::types::{CancelMode, LimitOrderId, NewLimitOrder, PostOnly};

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
/// * If the user has no MakerBook on chain, prepends `InitializeMakerBook`
///   (kind LO).
/// * If `deposit` is supplied, inserts `MakerDepositFunds` between init and
///   the book update. Required when the book lacks free collateral for the
///   requested orders.
/// * Emits one place op per order, each expecting an empty slot at its
///   price, so a level that appeared there in between fails the write.
/// * The write is an `UpdateBookLimit`: the program rejects a placed level
///   that crosses a registered maker (`PostOnlyWouldCross`) unless
///   `post_only.cross_policy` is `Allow`.
///
/// Fails locally if an order would duplicate a resting price, overfill a
/// side, or cross the user's own book.
pub fn build_place(
    owner: impl Into<Identity>,
    market: &Pubkey,
    current_book: Option<&MakerBook>,
    orders: &[NewLimitOrder],
    deposit: Option<CollateralArgs>,
    post_only: &PostOnly,
    config: &MarketConfig,
) -> SdkResult<LimitOrderActionResult> {
    let identity = owner.into();
    if orders.is_empty() {
        return Err(ArcherSDKError::EmptyOrderList);
    }

    let (mut local, current_seq, needs_init) = match current_book {
        Some(book) => (
            LocalBook::from_maker_book(book)?,
            book.last_updated_sequence_number,
            false,
        ),
        None => (LocalBook::empty(), 0u64, true),
    };

    let mut ops = Vec::with_capacity(orders.len());
    let mut placed_ids = Vec::with_capacity(orders.len());
    for new in orders {
        let (id, size_lots) = resolve_new_order(new, config)?;
        ops.push(local.place(id, size_lots)?);
        placed_ids.push(id);
    }
    local.check_not_crossed()?;

    let (maker_book_pda, _) = pda::derive_maker_book(market, &identity.maker());
    let mut instructions = Vec::with_capacity(3);

    if needs_init {
        instructions.push(create_initialize_maker_book_instruction(
            identity,
            *market,
            crate::onchain::MAKER_KIND_LO,
        ));
    }
    append_deposit(&mut instructions, &identity, market, maker_book_pda, deposit, config);

    let next_seq = push_level_ops(
        &mut instructions,
        &identity,
        market,
        maker_book_pda,
        post_only,
        current_seq,
        &ops,
    );

    Ok(LimitOrderActionResult {
        instructions,
        placed_ids,
        next_sequence_number: next_seq,
    })
}

/// Modify an existing limit order's price and/or size.
///
/// Returns the *new* `LimitOrderId` in `placed_ids[0]`. If `new_price` rounds
/// to the same tick as the old order, this is a single strict resize op and
/// the ID is preserved; otherwise it is a cancel (under `cancel_mode`) plus a
/// place at the new price, in one instruction. `new_size` of zero cancels.
#[allow(clippy::too_many_arguments)]
pub fn build_modify(
    owner: impl Into<Identity>,
    market: &Pubkey,
    current_book: &MakerBook,
    id: LimitOrderId,
    new_price: f64,
    new_size: f64,
    post_only: &PostOnly,
    cancel_mode: CancelMode,
    config: &MarketConfig,
) -> SdkResult<LimitOrderActionResult> {
    let identity = owner.into();
    let mut local = LocalBook::from_maker_book(current_book)?;

    let new = NewLimitOrder {
        side: id.side,
        price: new_price,
        size: new_size,
    };
    let (new_id, new_size_lots) = resolve_new_order(&new, config)?;

    let (ops, placed_ids) = if new_id == id {
        let op = local.resize(id, new_size_lots)?;
        let still_resting = if new_size_lots == 0 { Vec::new() } else { vec![id] };
        (vec![op], still_resting)
    } else {
        let cancel = local.cancel(id, cancel_mode)?;
        if new_size_lots == 0 {
            (vec![cancel], Vec::new())
        } else {
            let place = local.place(new_id, new_size_lots)?;
            (vec![cancel, place], vec![new_id])
        }
    };
    local.check_not_crossed()?;

    let (maker_book_pda, _) = pda::derive_maker_book(market, &identity.maker());
    let mut instructions = Vec::with_capacity(1);
    let next_seq = push_level_ops(
        &mut instructions,
        &identity,
        market,
        maker_book_pda,
        post_only,
        current_book.last_updated_sequence_number,
        &ops,
    );

    Ok(LimitOrderActionResult {
        instructions,
        placed_ids,
        next_sequence_number: next_seq,
    })
}

/// Cancel one or more limit orders atomically. All IDs must currently exist
/// in the snapshot. Optionally bundles a withdraw of newly freed collateral.
///
/// A cancel can never cross, so the program never rejects it under post-only;
/// the registry set in `post_only` is still required by the instruction.
/// `cancel_mode` picks the guard: [`CancelMode::Any`] removes whatever still
/// rests, [`CancelMode::Strict`] fails if a fill changed the size in between.
#[allow(clippy::too_many_arguments)]
pub fn build_cancel(
    owner: impl Into<Identity>,
    market: &Pubkey,
    current_book: &MakerBook,
    ids: &[LimitOrderId],
    withdraw: Option<CollateralArgs>,
    post_only: &PostOnly,
    cancel_mode: CancelMode,
    config: &MarketConfig,
) -> SdkResult<LimitOrderActionResult> {
    let identity = owner.into();
    if ids.is_empty() {
        return Err(ArcherSDKError::EmptyOrderList);
    }

    let mut local = LocalBook::from_maker_book(current_book)?;
    let mut ops = Vec::with_capacity(ids.len());
    for id in ids {
        ops.push(local.cancel(*id, cancel_mode)?);
    }

    let (maker_book_pda, _) = pda::derive_maker_book(market, &identity.maker());
    let mut instructions = Vec::with_capacity(2);
    let next_seq = push_level_ops(
        &mut instructions,
        &identity,
        market,
        maker_book_pda,
        post_only,
        current_book.last_updated_sequence_number,
        &ops,
    );

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

/// Cancel every active order via `ClearBook`. One instruction regardless of
/// how many orders rest, and it needs no registry set.
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
    instructions.push(create_clear_book_instruction(identity, maker_book_pda, next_seq));

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

/// Replace the user's entire active order set with `orders`.
///
/// Diffs the snapshot against the desired set: levels not in `orders` are
/// cancelled (under `cancel_mode`), levels in both with a different size are
/// resized (strict), new levels are placed. Levels already at the desired
/// size are left alone, so an unchanged book emits no write at all. Useful
/// for portfolio-style "this is my new desired state" callers.
///
/// `placed_ids` lists every order in `orders`, in input order, whether it was
/// newly placed, resized or untouched.
#[allow(clippy::too_many_arguments)]
pub fn build_replace_all(
    owner: impl Into<Identity>,
    market: &Pubkey,
    current_book: Option<&MakerBook>,
    orders: &[NewLimitOrder],
    deposit: Option<CollateralArgs>,
    post_only: &PostOnly,
    cancel_mode: CancelMode,
    config: &MarketConfig,
) -> SdkResult<LimitOrderActionResult> {
    let identity = owner.into();
    if orders.is_empty() {
        return Err(ArcherSDKError::EmptyOrderList);
    }

    let (mut local, current_seq, needs_init) = match current_book {
        Some(book) => (
            LocalBook::from_maker_book(book)?,
            book.last_updated_sequence_number,
            false,
        ),
        None => (LocalBook::empty(), 0u64, true),
    };

    let mut desired = Vec::with_capacity(orders.len());
    for new in orders {
        desired.push(resolve_new_order(new, config)?);
    }
    let ops = local.diff_to(&desired, cancel_mode)?;
    local.check_not_crossed()?;
    let placed_ids = desired.iter().map(|(id, _)| *id).collect();

    let (maker_book_pda, _) = pda::derive_maker_book(market, &identity.maker());
    let mut instructions = Vec::with_capacity(3);
    if needs_init {
        instructions.push(create_initialize_maker_book_instruction(
            identity,
            *market,
            crate::onchain::MAKER_KIND_LO,
        ));
    }
    append_deposit(&mut instructions, &identity, market, maker_book_pda, deposit, config);

    let next_seq = push_level_ops(
        &mut instructions,
        &identity,
        market,
        maker_book_pda,
        post_only,
        current_seq,
        &ops,
    );

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
    instructions.push(create_clear_book_instruction(identity, maker_book_pda, next_seq));
    append_withdraw(
        &mut instructions,
        &identity,
        market,
        maker_book_pda,
        withdraw,
        config,
    );
    instructions.push(create_close_maker_book_instruction(identity, *market));

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

/// Append the `UpdateBookLimit`(s) for `ops`, at most [`MAX_LEVEL_OPS`] per
/// instruction, each with the next sequence number. Returns the last
/// sequence number written — `current_seq` itself when there are no ops.
fn push_level_ops(
    instructions: &mut Vec<Instruction>,
    identity: &Identity,
    market: &Pubkey,
    maker_book_pda: Pubkey,
    post_only: &PostOnly,
    current_seq: u64,
    ops: &[LevelOp],
) -> u64 {
    let mut seq = current_seq;
    for chunk in ops.chunks(MAX_LEVEL_OPS) {
        seq += 1;
        instructions.push(create_update_book_limit_instruction(
            identity,
            *market,
            maker_book_pda,
            post_only.registry,
            &post_only.registry_books,
            post_only.cross_policy as u8,
            seq,
            chunk,
        ));
    }
    seq
}

fn append_deposit(
    instructions: &mut Vec<Instruction>,
    identity: &Identity,
    market: &Pubkey,
    maker_book_pda: Pubkey,
    deposit: Option<CollateralArgs>,
    config: &MarketConfig,
) {
    let Some(dep) = deposit else { return };
    if dep.base_lots == 0 && dep.quote_lots == 0 {
        return;
    }
    instructions.push(create_maker_deposit_funds_instruction(
        MakerDepositFundsParams {
            base_lots: BaseLots::new(dep.base_lots),
            quote_lots: QuoteLots::new(dep.quote_lots),
        },
        identity,
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
    ));
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
    instructions.push(create_maker_withdraw_funds_instruction(
            MakerWithdrawFundsParams {
                base_lots: BaseLots::new(w.base_lots),
                quote_lots: QuoteLots::new(w.quote_lots),
            },
            identity,
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
        ));
}
