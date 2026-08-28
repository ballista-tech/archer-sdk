//! Taker swap instruction builders.
//!
//! Thin wrappers around the program crate's instruction builders.
//! Converts human-readable amounts to atoms and delegates serialization
//! and account layout to [`crate::onchain::builders`].

use crate::onchain::{Side, SwapMode};
use solana_program::instruction::Instruction;
use solana_program::pubkey::Pubkey;

use crate::onchain::{swap_types::SwapParams};

use crate::config::MarketConfig;
use crate::error::SdkResult;
use crate::identity::Identity;
use crate::math::lots::{base_amount_to_atoms, quote_amount_to_atoms};
use crate::pda;

fn attach_event_authority(ix: &mut Instruction) {
    ix.accounts
        .push(solana_program::instruction::AccountMeta::new_readonly(
            pda::event_authority(),
            false,
        ));
}

/// Build a synchronous Fill-or-Kill swap instruction.
///
/// For most use cases, prefer the convenience functions below
/// ([`build_buy_max_amount_in`], [`build_sell_max_amount_in`], etc.).
///
/// # Arguments
///
/// - `amount` — Trade amount in human-readable token units.
/// - `threshold` — Slippage bound in human-readable token units.
/// - `maker_books` — Maker book pubkeys to match against.
/// - `builder_fee_wallet` — Quote token account receiving the builder fee.
/// - `builder_fee_ppm` — Builder fee in ppm of the quote notional traded, paid by
///   the taker on top of the protocol fee. `0` disables it. Must not exceed
///   [`crate::onchain::MAX_BUILDER_FEE_PPM`]; the taker's `threshold` also bounds it.
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
    let (amount_atoms, threshold_atoms) =
        resolve_swap_amounts(side, mode, amount, threshold, config)?;

    let params = SwapParams {
        side: side as u8,
        mode: mode as u8,
        amount: amount_atoms,
        threshold: threshold_atoms,
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
    let amount_atoms = quote_amount_to_atoms(quote_amount, config)?;
    let threshold_atoms = base_amount_to_atoms(min_base_out, config)?;

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
        SwapParams {
            side: Side::Bid as u8,
            mode: SwapMode::MaxAmountIn as u8,
            amount: amount_atoms,
            threshold: threshold_atoms,
            builder_fee_ppm,
        },
    ))
}

/// Buy base with minimum base output (MinAmountOut).
///
/// "I want minimum `base_amount` of base token.
///  I'll pay at most `max_quote_in` of quote token."
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
    let amount_atoms = base_amount_to_atoms(base_amount, config)?;
    let threshold_atoms = quote_amount_to_atoms(max_quote_in, config)?;

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
        SwapParams {
            side: Side::Bid as u8,
            mode: SwapMode::MinAmountOut as u8,
            amount: amount_atoms,
            threshold: threshold_atoms,
            builder_fee_ppm,
        },
    ))
}

/// Sell base with maxmimum base input (MaxAmountIn).
///
/// "I want to sell maximum `base_amount` of base token.
///  Give me at least `min_quote_out` of quote token."
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
    let amount_atoms = base_amount_to_atoms(base_amount, config)?;
    let threshold_atoms = quote_amount_to_atoms(min_quote_out, config)?;

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
        SwapParams {
            side: Side::Ask as u8,
            mode: SwapMode::MaxAmountIn as u8,
            amount: amount_atoms,
            threshold: threshold_atoms,
            builder_fee_ppm,
        },
    ))
}

/// Sell base with minimum quote output (MinAmountOut).
///
/// "I want minimum `quote_amount` of quote token.
///  I'll sell at most `max_base_in` of base token."
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
    let amount_atoms = quote_amount_to_atoms(quote_amount, config)?;
    let threshold_atoms = base_amount_to_atoms(max_base_in, config)?;

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
        SwapParams {
            side: Side::Ask as u8,
            mode: SwapMode::MinAmountOut as u8,
            amount: amount_atoms,
            threshold: threshold_atoms,
            builder_fee_ppm,
        },
    ))
}

/// Convert human-readable swap amounts to atoms based on side and mode.
fn resolve_swap_amounts(
    side: Side,
    mode: SwapMode,
    amount: f64,
    threshold: f64,
    config: &MarketConfig,
) -> SdkResult<(u64, u64)> {
    match (side, mode) {
        // Buy MaxAmountIn: spend quote, threshold is min base out
        (Side::Bid, SwapMode::MaxAmountIn) => {
            let amount_atoms = quote_amount_to_atoms(amount, config)?;
            let threshold_atoms = base_amount_to_atoms(threshold, config)?;
            Ok((amount_atoms, threshold_atoms))
        }
        // Buy MinAmountOut: want base, threshold is max quote in
        (Side::Bid, SwapMode::MinAmountOut) => {
            let amount_atoms = base_amount_to_atoms(amount, config)?;
            let threshold_atoms = quote_amount_to_atoms(threshold, config)?;
            Ok((amount_atoms, threshold_atoms))
        }
        // Sell MaxAmountIn: spend base, threshold is min quote out
        (Side::Ask, SwapMode::MaxAmountIn) => {
            let amount_atoms = base_amount_to_atoms(amount, config)?;
            let threshold_atoms = quote_amount_to_atoms(threshold, config)?;
            Ok((amount_atoms, threshold_atoms))
        }
        // Sell MinAmountOut: want quote, threshold is max base in
        (Side::Ask, SwapMode::MinAmountOut) => {
            let amount_atoms = quote_amount_to_atoms(amount, config)?;
            let threshold_atoms = base_amount_to_atoms(threshold, config)?;
            Ok((amount_atoms, threshold_atoms))
        }
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
    let mut ix = match identity {
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
    };

    attach_event_authority(&mut ix);
    ix
}
