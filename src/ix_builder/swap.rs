//! Taker swap instruction builders.
//!
//! Thin wrappers around the program crate's instruction builders.
//! Converts human-readable amounts to lots and delegates serialization
//! and account layout to [`crate::onchain::builders`].
//!
//! # Units and rounding
//!
//! The program reads `SwapParams::amount` and `SwapParams::threshold` in
//! lots: `base_atoms_per_base_lot` on the base side, `quote_atoms_per_quote_lot`
//! on the quote side. Every builder here converts so that rounding can never
//! work against the taker:
//!
//! - a **maximum input** (`amount` in MaxAmountIn, `threshold` in MinAmountOut)
//!   rounds *down*, so the taker never spends more than asked. A positive value
//!   below one lot is an error (`SizeBelowResolution`), since the program
//!   rejects a zero bound.
//! - a **minimum output** (`threshold` in MaxAmountIn, `amount` in MinAmountOut)
//!   rounds *up*, so the taker never receives less than asked. A positive value
//!   below one lot becomes one lot; `0.0` stays `0`.
//!
//! The `f64` is first rounded to the nearest atom, which absorbs binary float
//! noise (`0.067 * 1e9` is `67000000.00000001`, `1.001 * 1e6` is
//! `1000999.9999999999`), and only then divided into lots with integer
//! arithmetic.
//!

use crate::onchain::{Side, SwapMode, MAX_BUILDER_FEE_PPM};
use solana_program::instruction::Instruction;
use solana_program::pubkey::Pubkey;

use crate::onchain::swap_types::SwapParams;

use crate::config::MarketConfig;
use crate::error::{ArcherSDKError, SdkResult};
use crate::identity::Identity;

/// Which way a human amount rounds when it does not land on a lot boundary.
#[derive(Clone, Copy)]
enum Bound {
    /// An upper bound on what the taker pays: round down.
    MaxInput,
    /// A lower bound on what the taker receives: round up.
    MinOutput,
}

/// Convert a human amount to lots, rounding to the nearest atom first.
fn to_lots(
    amount: f64,
    atoms_per_unit: f64,
    atoms_per_lot: u64,
    bound: Bound,
    operation: &'static str,
) -> SdkResult<u64> {
    if !amount.is_finite() || amount < 0.0 {
        return Err(ArcherSDKError::InvalidSize(amount));
    }
    if amount == 0.0 {
        return Ok(0);
    }

    let atoms_f64 = (amount * atoms_per_unit).round();
    if atoms_f64 > u64::MAX as f64 {
        return Err(ArcherSDKError::ArithmeticOverflow { operation });
    }
    let atoms = atoms_f64 as u128;
    let per_lot = atoms_per_lot.max(1) as u128;

    let lots = match bound {
        Bound::MaxInput => {
            let lots = atoms / per_lot;
            if lots == 0 {
                return Err(ArcherSDKError::SizeBelowResolution(amount));
            }
            lots
        }
        Bound::MinOutput => atoms.div_ceil(per_lot).max(1),
    };

    u64::try_from(lots).map_err(|_| ArcherSDKError::ArithmeticOverflow { operation })
}

fn base_lots(amount: f64, bound: Bound, config: &MarketConfig) -> SdkResult<u64> {
    to_lots(
        amount,
        config.base_atoms_divisor(),
        config.base_atoms_per_base_lot,
        bound,
        "swap base amount to lots",
    )
}

fn quote_lots(amount: f64, bound: Bound, config: &MarketConfig) -> SdkResult<u64> {
    to_lots(
        amount,
        config.quote_atoms_divisor(),
        config.quote_atoms_per_quote_lot,
        bound,
        "swap quote amount to lots",
    )
}

/// Build a synchronous Fill-or-Kill swap instruction.
///
/// For most use cases, prefer the convenience functions below
/// ([`build_buy_max_amount_in`], [`build_sell_max_amount_in`], etc.).
///
/// # Arguments
///
/// - `amount` — Trade amount in human-readable token units. The input token
///   in MaxAmountIn mode, the output token in MinAmountOut mode.
/// - `threshold` — Slippage bound in human-readable token units. The minimum
///   output in MaxAmountIn mode, the maximum input in MinAmountOut mode (where
///   the program requires it to be non-zero).
/// - `maker_books` — Maker book pubkeys to match against.
/// - `builder_fee_wallet` — Quote token account receiving the builder fee.
/// - `builder_fee_ppm` — Builder fee in ppm of the quote notional traded, paid by
///   the taker on top of the protocol fee. `0` disables it. Must not exceed
///   [`crate::onchain::MAX_BUILDER_FEE_PPM`]; the taker's `threshold` also bounds it.
///
/// See the module docs for how amounts round into lots.
#[allow(clippy::too_many_arguments)]
pub fn build_swap_ix(
    taker: impl Into<Identity>,
    market: &Pubkey,
    builder_fee_wallet: &Pubkey,
    side: Side,
    mode: SwapMode,
    amount: f64,
    threshold: f64,
    taker_base_ata: &Pubkey,
    taker_quote_ata: &Pubkey,
    base_token_program: &Pubkey,
    quote_token_program: &Pubkey,
    maker_books: &[Pubkey],
    config: &MarketConfig,
    builder_fee_ppm: u32,
) -> SdkResult<Instruction> {
    if builder_fee_ppm > MAX_BUILDER_FEE_PPM {
        return Err(ArcherSDKError::BuilderFeeTooHigh {
            requested: builder_fee_ppm,
            max: MAX_BUILDER_FEE_PPM,
        });
    }

    let (amount_lots, threshold_lots) =
        resolve_swap_amounts(side, mode, amount, threshold, config)?;

    let params = SwapParams {
        side: side as u8,
        mode: mode as u8,
        amount: amount_lots,
        threshold: threshold_lots,
        builder_fee_ppm,
    };

    Ok(build_for_identity(
        &taker.into(),
        market,
        builder_fee_wallet,
        &config.base_mint,
        &config.quote_mint,
        &config.base_vault,
        &config.quote_vault,
        taker_base_ata,
        taker_quote_ata,
        base_token_program,
        quote_token_program,
        maker_books,
        params,
    ))
}

/// Buy base with max quote input (MaxAmountIn).
///
/// "I want to spend maximum `quote_amount` of quote token.
///  Give me at least `min_base_out` of base token."
#[allow(clippy::too_many_arguments)]
pub fn build_buy_max_amount_in(
    taker: impl Into<Identity>,
    market: &Pubkey,
    builder_fee_wallet: &Pubkey,
    quote_amount: f64,
    min_base_out: f64,
    taker_base_ata: &Pubkey,
    taker_quote_ata: &Pubkey,
    base_token_program: &Pubkey,
    quote_token_program: &Pubkey,
    maker_books: &[Pubkey],
    config: &MarketConfig,
    builder_fee_ppm: u32,
) -> SdkResult<Instruction> {
    build_swap_ix(
        taker,
        market,
        builder_fee_wallet,
        Side::Bid,
        SwapMode::MaxAmountIn,
        quote_amount,
        min_base_out,
        taker_base_ata,
        taker_quote_ata,
        base_token_program,
        quote_token_program,
        maker_books,
        config,
        builder_fee_ppm,
    )
}

/// Buy base with minimum base output (MinAmountOut).
///
/// "I want minimum `base_amount` of base token.
///  I'll pay at most `max_quote_in` of quote token."
#[allow(clippy::too_many_arguments)]
pub fn build_buy_min_amount_out(
    taker: impl Into<Identity>,
    market: &Pubkey,
    builder_fee_wallet: &Pubkey,
    base_amount: f64,
    max_quote_in: f64,
    taker_base_ata: &Pubkey,
    taker_quote_ata: &Pubkey,
    base_token_program: &Pubkey,
    quote_token_program: &Pubkey,
    maker_books: &[Pubkey],
    config: &MarketConfig,
    builder_fee_ppm: u32,
) -> SdkResult<Instruction> {
    build_swap_ix(
        taker,
        market,
        builder_fee_wallet,
        Side::Bid,
        SwapMode::MinAmountOut,
        base_amount,
        max_quote_in,
        taker_base_ata,
        taker_quote_ata,
        base_token_program,
        quote_token_program,
        maker_books,
        config,
        builder_fee_ppm,
    )
}

/// Sell base with maxmimum base input (MaxAmountIn).
///
/// "I want to sell maximum `base_amount` of base token.
///  Give me at least `min_quote_out` of quote token."
#[allow(clippy::too_many_arguments)]
pub fn build_sell_max_amount_in(
    taker: impl Into<Identity>,
    market: &Pubkey,
    builder_fee_wallet: &Pubkey,
    base_amount: f64,
    min_quote_out: f64,
    taker_base_ata: &Pubkey,
    taker_quote_ata: &Pubkey,
    base_token_program: &Pubkey,
    quote_token_program: &Pubkey,
    maker_books: &[Pubkey],
    config: &MarketConfig,
    builder_fee_ppm: u32,
) -> SdkResult<Instruction> {
    build_swap_ix(
        taker,
        market,
        builder_fee_wallet,
        Side::Ask,
        SwapMode::MaxAmountIn,
        base_amount,
        min_quote_out,
        taker_base_ata,
        taker_quote_ata,
        base_token_program,
        quote_token_program,
        maker_books,
        config,
        builder_fee_ppm,
    )
}

/// Sell base with minimum quote output (MinAmountOut).
///
/// "I want minimum `quote_amount` of quote token.
///  I'll sell at most `max_base_in` of base token."
#[allow(clippy::too_many_arguments)]
pub fn build_sell_min_amount_out(
    taker: impl Into<Identity>,
    market: &Pubkey,
    builder_fee_wallet: &Pubkey,
    quote_amount: f64,
    max_base_in: f64,
    taker_base_ata: &Pubkey,
    taker_quote_ata: &Pubkey,
    base_token_program: &Pubkey,
    quote_token_program: &Pubkey,
    maker_books: &[Pubkey],
    config: &MarketConfig,
    builder_fee_ppm: u32,
) -> SdkResult<Instruction> {
    build_swap_ix(
        taker,
        market,
        builder_fee_wallet,
        Side::Ask,
        SwapMode::MinAmountOut,
        quote_amount,
        max_base_in,
        taker_base_ata,
        taker_quote_ata,
        base_token_program,
        quote_token_program,
        maker_books,
        config,
        builder_fee_ppm,
    )
}

/// Convert human-readable swap amounts to lots based on side and mode.
///
/// Returns `(amount_lots, threshold_lots)`. Maximum inputs round down,
/// minimum outputs round up; see the module docs.
fn resolve_swap_amounts(
    side: Side,
    mode: SwapMode,
    amount: f64,
    threshold: f64,
    config: &MarketConfig,
) -> SdkResult<(u64, u64)> {
    match (side, mode) {
        // Buy MaxAmountIn: spend at most `amount` quote, receive at least `threshold` base
        (Side::Bid, SwapMode::MaxAmountIn) => Ok((
            quote_lots(amount, Bound::MaxInput, config)?,
            base_lots(threshold, Bound::MinOutput, config)?,
        )),
        // Buy MinAmountOut: receive at least `amount` base, spend at most `threshold` quote
        (Side::Bid, SwapMode::MinAmountOut) => Ok((
            base_lots(amount, Bound::MinOutput, config)?,
            quote_lots(threshold, Bound::MaxInput, config)?,
        )),
        // Sell MaxAmountIn: spend at most `amount` base, receive at least `threshold` quote
        (Side::Ask, SwapMode::MaxAmountIn) => Ok((
            base_lots(amount, Bound::MaxInput, config)?,
            quote_lots(threshold, Bound::MinOutput, config)?,
        )),
        // Sell MinAmountOut: receive at least `amount` quote, spend at most `threshold` base
        (Side::Ask, SwapMode::MinAmountOut) => Ok((
            quote_lots(amount, Bound::MinOutput, config)?,
            base_lots(threshold, Bound::MaxInput, config)?,
        )),
    }
}

/// Pick the instruction that matches the identity.
///
/// A wallet taker signs for itself, so its funds move under its own signature →
/// `Swap`. An ArcherAccount cannot sign anything, so its owner or delegate signs
/// and the program moves the funds on the account's behalf →
/// `SwapFromAccount`.
///
/// Callers never choose between them. The arguments are identical and the
/// identity decides, which is what lets an individual and a platform call the
/// same four convenience functions above.
#[allow(clippy::too_many_arguments)]
fn build_for_identity(
    identity: &Identity,
    market: &Pubkey,
    builder_fee_wallet: &Pubkey,
    base_mint: &Pubkey,
    quote_mint: &Pubkey,
    base_vault: &Pubkey,
    quote_vault: &Pubkey,
    taker_base_ata: &Pubkey,
    taker_quote_ata: &Pubkey,
    base_token_program: &Pubkey,
    quote_token_program: &Pubkey,
    maker_books: &[Pubkey],
    params: SwapParams,
) -> Instruction {
    match identity {
        Identity::Wallet(taker) => crate::onchain::builders::create_swap_instrucion(
            &crate::ARCHER_V1_PROGRAM_ID,
            taker,
            market,
            builder_fee_wallet,
            base_mint,
            quote_mint,
            base_vault,
            quote_vault,
            taker_base_ata,
            taker_quote_ata,
            base_token_program,
            quote_token_program,
            maker_books,
            params,
        ),
        Identity::ArcherAccount { account, authority } => {
            crate::onchain::builders::create_swap_from_archer_account_instruction(
                &crate::ARCHER_V1_PROGRAM_ID,
                authority,
                account,
                market,
                builder_fee_wallet,
                base_mint,
                quote_mint,
                base_vault,
                quote_vault,
                taker_base_ata,
                taker_quote_ata,
                base_token_program,
                quote_token_program,
                maker_books,
                params,
            )
        }
    }
}
