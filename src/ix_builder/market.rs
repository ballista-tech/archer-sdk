//! Market instruction builders
//!
//! Thin wrappers around the program crate's instruction builders.

use solana_program::instruction::Instruction;
use solana_program::pubkey::Pubkey;

use crate::onchain::{
    ArcherUnit, BaseAtomsPerLot, CollectProtocolFeeParams, InitializeMarketParams,
    MarketStateHeader, QuoteAtomsPerBaseUnitPerTick, QuoteAtomsPerLot,
    PERMISSIONLESS_MAKER_FEE_PPM, PERMISSIONLESS_TAKER_FEE_PPM,
};

use crate::error::{ArcherSDKError, SdkResult};

pub fn permissionless_fee_config(params: &mut InitializeMarketParams) {
    params.maker_fee_ppm = PERMISSIONLESS_MAKER_FEE_PPM;
    params.taker_fee_ppm = PERMISSIONLESS_TAKER_FEE_PPM;
}

/// Build an InitializeMarket instruction.
pub fn build_initialize_market_ix(
    params: InitializeMarketParams,
    admin: &Pubkey,
    payer: &Pubkey,
) -> SdkResult<Instruction> {
    MarketStateHeader::validate_permissionless_fee_config(
        params.maker_fee_ppm,
        params.taker_fee_ppm,
    )
    .map_err(|_| ArcherSDKError::FixedFeeConfigRequired {
        maker_ppm: params.maker_fee_ppm,
        taker_ppm: params.taker_fee_ppm,
    })?;

    params
        .validate()
        .map_err(|e| ArcherSDKError::InvalidMarketParams(e as u32))?;

    let mut header: MarketStateHeader = bytemuck::Zeroable::zeroed();
    header.base_decimals = params.base_decimals;
    header.quote_decimals = params.quote_decimals;
    header.base_atoms_per_base_lot = BaseAtomsPerLot::new(params.base_atoms_per_base_lot);
    header.quote_atoms_per_quote_lot = QuoteAtomsPerLot::new(params.quote_atoms_per_quote_lot);
    header.tick_size_in_quote_atoms_per_base_unit =
        QuoteAtomsPerBaseUnitPerTick::new(params.tick_size_in_quote_atoms_per_base_unit);
    header.raw_base_units_per_base_unit = params.raw_base_units_per_base_unit;
    header
        .validate_market_invariants()
        .map_err(|e| ArcherSDKError::InvalidMarketParams(e as u32))?;

    Ok(
        crate::onchain::builders::create_initialize_market_instruction(
            params, *admin, *payer,
        ),
    )
}

/// Build a TransferAdmin instruction.
///
/// Transfers the market's revenue seat — the 80% fee share and the right to call
/// `CollectProtocolFee`. Irreversible, and it conveys no control over how the
/// market runs.
pub fn build_transfer_admin_ix(
    market: &Pubkey,
    admin: &Pubkey,
    new_admin: &Pubkey,
) -> Vec<Instruction> {
    crate::onchain::builders::create_transfer_admin_instruction(*market, *admin, *new_admin)
}

/// Build a CollectProtocolFee instruction.
///
/// Splits accrued fees: 20% to the Archer treasury, 80% to the market admin.
/// Callable by the market's admin.
#[allow(clippy::too_many_arguments)]
pub fn build_collect_protocol_fee_ix(
    params: CollectProtocolFeeParams,
    market: &Pubkey,
    admin: &Pubkey,
    quote_mint: &Pubkey,
    quote_vault: &Pubkey,
    admin_quote_ata: &Pubkey,
    archer_treasury: &Pubkey,
    treasury_quote_token_account: &Pubkey,
    quote_token_program: &Pubkey,
) -> Instruction {
    crate::onchain::builders::create_collect_protocol_fee_instruction(
        params,
        *market,
        *admin,
        *quote_mint,
        *quote_vault,
        *admin_quote_ata,
        *archer_treasury,
        *treasury_quote_token_account,
        *quote_token_program,
    )
}
