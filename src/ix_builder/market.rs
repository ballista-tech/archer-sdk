//! Market instruction builders
//!
//! Thin wrappers around the program crate's instruction builders.

use solana_program::instruction::Instruction;
use solana_program::pubkey::Pubkey;

use crate::onchain::{
    ArcherUnit, BaseAtomsPerLot, ChangeMarketStatusParams, CollectProtocolFeeParams, InitializeMarketParams, MarketStateHeader, MarketStatus, PERMISSIONLESS_MAKER_FEE_PPM, PERMISSIONLESS_TAKER_FEE_PPM, QuoteAtomsPerBaseUnitPerTick, QuoteAtomsPerLot, UpdateMakerFeeParams, UpdateTakerFeeParams,
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
) -> Instruction {
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

/// Build a ChangeMarketStatus instruction (Archer authority only)
///
/// Valid transitions: Active <-> Paused, and Active/Paused -> Closed. Closed is
/// terminal — the program rejects every transition out of it.
pub fn build_change_market_status_ix(
    market: &Pubkey,
    authority: &Pubkey,
    new_status: MarketStatus,
) -> Instruction {
    crate::onchain::builders::create_change_market_status_instruction(
        ChangeMarketStatusParams { new_status: new_status as u8 },
        *market,
        *authority,
    )
}

/// Build an UpdateMakerFee instruction (Archer authority only)
///
/// The market must be paused. `maker_fee_ppm` may be negative — a maker rebate —
/// but `maker_fee_ppm + taker_fee_ppm` must stay >= 0
pub fn build_update_maker_fee_ix(
    market: &Pubkey,
    authority: &Pubkey,
    maker_fee_ppm: i32,
    current_taker_fee_ppm: i32,
) -> SdkResult<Instruction> {
    MarketStateHeader::validate_fee_ppm(maker_fee_ppm)
        .map_err(|e| ArcherSDKError::InvalidMarketParams(e as u32))?;

    if maker_fee_ppm.saturating_add(current_taker_fee_ppm) < 0 {
        return Err(ArcherSDKError::InvalidMarketParams(
            crate::onchain::ArcherError::InvalidFee as u32,
        ));
    }

    Ok(crate::onchain::builders::create_update_maker_fee_instruction(
        UpdateMakerFeeParams { maker_fee_ppm },
        *market,
        *authority,
    ))
}

/// Build an UpdateTakerFee instruction (Archer authority only)
///
/// The market must be paused. The taker fee may **never** be negative: a taker
/// rebate is paid out of the makers' fills, so only the maker side may be a
/// rebate. `maker_fee_ppm + taker_fee_ppm` must also stay >= 0.
pub fn build_update_taker_fee_ix(
    market: &Pubkey,
    authority: &Pubkey,
    taker_fee_ppm: i32,
    current_maker_fee_ppm: i32,
) -> SdkResult<Instruction> {
    MarketStateHeader::validate_taker_fee_ppm(taker_fee_ppm)
        .map_err(|e| ArcherSDKError::InvalidMarketParams(e as u32))?;

    if current_maker_fee_ppm.saturating_add(taker_fee_ppm) < 0 {
        return Err(ArcherSDKError::InvalidMarketParams(
            crate::onchain::ArcherError::InvalidFee as u32,
        ));
    }

    Ok(crate::onchain::builders::create_update_taker_fee_instruction(
        UpdateTakerFeeParams { taker_fee_ppm },
        *market,
        *authority,
    ))
}

/// Build an InitializeMakerRegistry instruction (Archer authority only)
///
/// Creates the market's registry (up to `MAX_REGISTRY_MAKERS` books). The
/// authority also pays the rent. The registry PDA is derived for you.
pub fn build_initialize_maker_registry_ix(market: &Pubkey, authority: &Pubkey) -> Instruction {
    crate::onchain::builders::create_initialize_maker_registry_instruction(*authority, *market)
}

/// Build a RegisterMaker instruction (Archer authority only)
///
/// A swap that omits a registered book is rejected with `IncompleteMakerBooks`,
/// so registering a book commits every taker on this market to including it.
pub fn build_register_maker_ix(
    market: &Pubkey,
    authority: &Pubkey,
    maker_book: &Pubkey,
) -> Instruction {
    crate::onchain::builders::create_register_maker_instruction(*authority, *market, *maker_book)
}

/// Build a DeregisterMaker instruction.
pub fn build_deregister_maker_ix(
    market: &Pubkey,
    authority: &Pubkey,
    maker_book: &Pubkey,
) -> Instruction {
    crate::onchain::builders::create_deregister_maker_instruction(*authority, *market, *maker_book)
}
