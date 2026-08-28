//! Maker operation instruction builders.
//!
//! Thin wrappers around the program crate's instruction builders.
//! The SDK converts human-readable inputs (prices, token amounts) into
//! on-chain types, then delegates to the vendored builders in [`crate::onchain::builders`] for
//! serialization and account layout.

use crate::onchain::{ArcherUnit, BaseLots, QuoteLots, Ticks};
use solana_program::instruction::Instruction;
use solana_program::pubkey::Pubkey;

use crate::onchain::{
    builders::UpdateBookParams, builders::UpdateMidPriceParams,
    MakerDepositFundsParams, MakerWithdrawFundsParams,
};

use crate::config::MarketConfig;
use crate::error::SdkResult;
use crate::identity::{append_archer_account, append_authority, Identity};
use crate::math::lots::{base_amount_to_lots, quote_amount_to_lots};
use crate::math::BookUpdate;

/// Build the instruction(s) for a full book update.
///
/// Depending on whether the mid price changed, this returns either:
/// - `[UpdateMidPrice, UpdateBook]` — 2 instructions if mid shifted.
/// - `[UpdateBook]` — 1 instruction if mid is unchanged.
///
/// Both go in the same transaction. This is the primary function
/// market makers call on every quote cycle.
///
/// # Arguments
///
/// - `book_update` — From [`crate::math::build_book_update`] or
///   [`crate::math::build_book_from_spread`].
/// - `market` — The market account pubkey.
/// - `identity` — Who is quoting. **Pass your pubkey** if you are the maker;
///   a [`Identity`] only if you are a platform quoting for someone else's
///   ArcherAccount.
pub fn build_update_instructions(
    book_update: &BookUpdate,
    market: &Pubkey,
    identity: impl Into<Identity>,
    sequence_number: u64,
) -> SdkResult<Vec<Instruction>> {
    let identity = identity.into();
    let signer = identity.authority();
    let (maker_book_pda, _) = crate::pda::derive_maker_book(market, &identity.maker());
    let mut instructions = Vec::with_capacity(2);

    if book_update.mid_price_changed {
        instructions.push(append_archer_account(
            crate::onchain::builders::create_update_mid_price_instruction(
                signer,
                maker_book_pda,
                UpdateMidPriceParams {
                    new_mid_price_ticks: Ticks::new(book_update.new_mid_price_ticks),
                    sequence_number,
                },
            ),
            &identity,
        ));
    }

    instructions.push(append_archer_account(
        crate::onchain::builders::create_update_book_instruction(
            signer,
            *market,
            maker_book_pda,
            UpdateBookParams {
                mid_price_ticks: book_update.new_mid_price_ticks,
                bid_levels: book_update.bid_levels.clone(),
                ask_levels: book_update.ask_levels.clone(),
                sequence_number: sequence_number + 1,
            },
        ),
        &identity,
    ));

    Ok(instructions)
}

/// Build an UpdateBook instruction directly from a BookUpdate.
///
/// Lower-level alternative to [`build_update_instructions`] — only the
/// book update, no mid price change handling.
pub fn build_update_book_ix(
    book_update: &BookUpdate,
    market: &Pubkey,
    identity: impl Into<Identity>,
    sequence_number: u64,
) -> Instruction {
    let identity = identity.into();
    let (maker_book_pda, _) = crate::pda::derive_maker_book(market, &identity.maker());

    append_archer_account(
        crate::onchain::builders::create_update_book_instruction(
            identity.authority(),
            *market,
            maker_book_pda,
            UpdateBookParams {
                mid_price_ticks: book_update.new_mid_price_ticks,
                bid_levels: book_update.bid_levels.clone(),
                ask_levels: book_update.ask_levels.clone(),
                sequence_number,
            },
        ),
        &identity,
    )
}

/// Build an UpdateMidPrice instruction.
pub fn build_update_mid_price_ix(
    market: &Pubkey,
    identity: impl Into<Identity>,
    new_mid_price_ticks: u64,
    sequence_number: u64,
) -> Instruction {
    let identity = identity.into();
    let (maker_book_pda, _) = crate::pda::derive_maker_book(market, &identity.maker());

    append_archer_account(
        crate::onchain::builders::create_update_mid_price_instruction(
            identity.authority(),
            maker_book_pda,
            UpdateMidPriceParams {
                new_mid_price_ticks: Ticks::new(new_mid_price_ticks),
                sequence_number,
            },
        ),
        &identity,
    )
}

/// Build a ClearBook instruction (zero all orders, unlock all balances).
///
/// Emergency function — clears the entire book instantly.
pub fn build_clear_book_ix(
    market: &Pubkey,
    identity: impl Into<Identity>,
    sequence_number: u64,
) -> Instruction {
    let identity = identity.into();
    let (maker_book_pda, _) = crate::pda::derive_maker_book(market, &identity.maker());

    append_archer_account(
        crate::onchain::builders::create_clear_book_instruction(
            identity.authority(),
            maker_book_pda,
            sequence_number,
        ),
        &identity,
    )
}

/// Build a CloseMakerBook instruction.
///
/// Closes the maker book PDA and refunds the rent to the maker. The book must be fully
/// empty first — callers should sequence:
///   1. `build_clear_book_ix`           — zero all levels, unlock balances
///   2. `build_withdraw_ix(MAX, MAX)`   — drain free balance to wallet
///   3. `build_close_maker_book_ix`     — reclaim the PDA rent
pub fn build_close_maker_book_ix(identity: impl Into<Identity>, market: &Pubkey) -> Instruction {
    let identity = identity.into();
    append_authority(
        crate::onchain::builders::create_close_maker_book_instruction(
            identity.maker(),
            *market,
        ),
        &identity,
    )
}

/// Build a SetBookDelegate instruction.
///
/// The delegate can submit UpdateBook and UpdateMidPrice on the maker's behalf.
/// Pass `Pubkey::default()` to revoke delegation.
pub fn build_set_delegate_ix(maker: &Pubkey, market: &Pubkey, delegate: &Pubkey) -> Instruction {
    let (maker_book_pda, _) = crate::pda::derive_maker_book(market, maker);
    crate::onchain::builders::create_set_maker_book_delegate_instruction(
        *maker,
        maker_book_pda,
        *delegate,
    )
}

/// Build an InitializeMakerBook instruction.
///
/// Creates the maker book PDA account. Call once per (market, maker) pair
/// before any other maker operations. `kind`: `crate::onchain::MAKER_KIND_MM` (0) or
/// `crate::onchain::MAKER_KIND_LO` (1).
pub fn build_initialize_maker_book_ix(maker: &Pubkey, market: &Pubkey, kind: u8) -> Instruction {
    crate::onchain::builders::create_initialize_maker_book_instruction(*maker, *market, kind)
}

/// Build a deposit instruction.
///
/// Amounts are in human-readable token units (e.g., 100.5 SOL, 15000.0 USDC).
/// Pass `0.0` to skip depositing that side.
///
/// # Arguments
///
/// - `maker_base_ata` / `maker_quote_ata` — Maker's associated token accounts.
pub fn build_deposit_ix(
    maker: impl Into<Identity>,
    market: &Pubkey,
    base_amount: f64,
    quote_amount: f64,
    maker_base_ata: &Pubkey,
    maker_quote_ata: &Pubkey,
    base_token_program: &Pubkey,
    quote_token_program: &Pubkey,
    config: &MarketConfig,
) -> SdkResult<Instruction> {
    let identity = maker.into();
    let (maker_book_pda, _) = crate::pda::derive_maker_book(market, &identity.maker());

    let base_lots = if base_amount > 0.0 {
        base_amount_to_lots(base_amount, config)?
    } else {
        0
    };

    let quote_lots = if quote_amount > 0.0 {
        quote_amount_to_lots(quote_amount, config)?
    } else {
        0
    };

    Ok(append_authority(
        crate::onchain::builders::create_maker_deposit_funds_instruction(
            MakerDepositFundsParams {
                base_lots: BaseLots::new(base_lots),
                quote_lots: QuoteLots::new(quote_lots),
            },
            identity.maker(),
            maker_book_pda,
            *market,
            config.base_mint,
            config.quote_mint,
            *maker_base_ata,
            *maker_quote_ata,
            config.base_vault,
            config.quote_vault,
            *base_token_program,
            *quote_token_program,
        ),
        &identity,
    ))
}

/// Build a withdrawal instruction.
///
/// Pass `f64::MAX` to withdraw all free balance for that side.
/// Pass `0.0` to skip withdrawing that side.
pub fn build_withdraw_ix(
    maker: impl Into<Identity>,
    market: &Pubkey,
    base_amount: f64,
    quote_amount: f64,
    maker_base_ata: &Pubkey,
    maker_quote_ata: &Pubkey,
    base_token_program: &Pubkey,
    quote_token_program: &Pubkey,
    config: &MarketConfig,
) -> SdkResult<Instruction> {
    let identity = maker.into();
    let (maker_book_pda, _) = crate::pda::derive_maker_book(market, &identity.maker());

    let base_lots = if base_amount == f64::MAX {
        u64::MAX // signal: withdraw all free
    } else if base_amount > 0.0 {
        base_amount_to_lots(base_amount, config)?
    } else {
        0
    };

    let quote_lots = if quote_amount == f64::MAX {
        u64::MAX
    } else if quote_amount > 0.0 {
        quote_amount_to_lots(quote_amount, config)?
    } else {
        0
    };

    Ok(append_authority(
        crate::onchain::builders::create_maker_withdraw_funds_instruction(
            MakerWithdrawFundsParams {
                base_lots: BaseLots::new(base_lots),
                quote_lots: QuoteLots::new(quote_lots),
            },
            identity.maker(),
            maker_book_pda,
            *market,
            config.base_mint,
            config.quote_mint,
            *maker_base_ata,
            *maker_quote_ata,
            config.base_vault,
            config.quote_vault,
            *base_token_program,
            *quote_token_program,
        ),
        &identity,
    ))
}

/// Build an UpdateExpiryInSlots instruction.
///
/// Sets the max slots a maker book may remain un-refreshed before the
/// aggregator skips it. `0` disables the expiry check.
pub fn build_update_expiry_in_slots_ix(
    maker: &Pubkey,
    market: &Pubkey,
    expiry_in_slots: u64,
) -> Instruction {
    let (maker_book_pda, _) = crate::pda::derive_maker_book(market, maker);
    crate::onchain::builders::create_update_expiry_in_slots_instruction(
        *maker,
        maker_book_pda,
        expiry_in_slots,
    )
}

/// Build a ToggleBookSuspension instruction (admin suspends/unsuspends a maker).
pub fn build_toggle_suspension_ix(market: &Pubkey, admin: &Pubkey, maker: &Pubkey) -> Instruction {
    let (maker_book_pda, _) = crate::pda::derive_maker_book(market, maker);
    crate::onchain::builders::create_toggle_maker_book_suspension_instruction(
        *market,
        *admin,
        maker_book_pda,
    )
}
