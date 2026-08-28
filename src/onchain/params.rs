//! Instruction parameter types — the wire format of Archer's instructions.
//!
//! Copied from the program, where they sit beside their handlers. A client needs
//! them to build a transaction; the handlers are the program's business.

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

use crate::onchain::{
    errors::ArcherError, is_global_authority, BaseLots, MarketStateHeader,
    MarketStatus, QuoteLots,
};


#[derive(BorshDeserialize, BorshSerialize, Debug)]
pub struct MakerWithdrawFundsParams {
    /// Base token authority
    pub base_lots: BaseLots,

    /// Quote token authority
    pub quote_lots: QuoteLots,
}


#[derive(BorshDeserialize, BorshSerialize, Debug)]
pub struct MakerDepositFundsParams {
    /// Base token authority
    pub base_lots: BaseLots,

    /// Quote token authority
    pub quote_lots: QuoteLots,
}


#[derive(BorshDeserialize, BorshSerialize, Debug, Clone)]
pub struct UpdateExpiryInSlotsParams {
    /// Max slots a book may remain un-refreshed before the aggregator skips it.
    /// `0` disables the expiry check.
    pub expiry_in_slots: u64,
}


#[derive(BorshDeserialize, BorshSerialize, Debug, Clone)]
pub struct UpdateTakerFeeParams {
    /// New taker fee in parts per million
    pub taker_fee_ppm: i32,
}


#[derive(BorshDeserialize, BorshSerialize, Debug, Clone)]
pub struct ChangeMarketStatusParams {
    /// New market status
    pub new_status: u8,
}
impl ChangeMarketStatusParams {
    pub fn validate(&self, current_status: MarketStatus) -> Result<(), crate::onchain::errors::ArcherError> {
        let new_status = MarketStatus::from_u8(self.new_status)
            .map_err(|_| crate::onchain::errors::ArcherError::InvalidStatusTransition)?;

        match (current_status, new_status) {
            (MarketStatus::Closed, _) => Err(crate::onchain::errors::ArcherError::MarketNotActive),
            _ => Ok(()),
        }
    }
}


#[derive(BorshDeserialize, BorshSerialize, Debug, Clone)]
pub struct UpdateMakerFeeParams {
    /// New maker fee in parts per million
    pub maker_fee_ppm: i32,
}


#[derive(BorshDeserialize, BorshSerialize, Debug, Clone)]
pub struct CollectProtocolFeeParams {
    /// Amount of quote atoms to collect
    pub amount: u64,
}

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
pub struct InitializeMarketParams {
    /// Market ID pubkey
    pub market_id: Pubkey,

    /// Base token mint
    pub base_mint: Pubkey,

    /// Base token program (SPL or Token 2022)
    pub base_token_program: Pubkey,

    /// Quote token mint
    pub quote_mint: Pubkey,

    /// Quote token program (SPL or Token 2022)
    pub quote_token_program: Pubkey,

    /// Minimum tradeable quantity of base token
    pub base_atoms_per_base_lot: u64,

    /// Minimum tradeable quantity of quote token
    pub quote_atoms_per_quote_lot: u64,

    /// Tick size. All prices are a function of this tick size (E.g. 1000 USDC atoms per SOL as the min. qty tradeable)
    pub tick_size_in_quote_atoms_per_base_unit: u64,

    /// Base units scaler
    pub raw_base_units_per_base_unit: u64,

    /// Base token decimals
    pub base_decimals: u8,

    /// Quote token decimals
    pub quote_decimals: u8,

    /// Maker fee in parts per million. Negative are rebates
    pub maker_fee_ppm: i32,

    /// Taker fee in parts per million. Negative are rebates
    pub taker_fee_ppm: i32,
}
impl InitializeMarketParams {
    pub fn validate(&self) -> Result<(), ArcherError> {
        if self.market_id == Pubkey::default() {
            return Err(ArcherError::InvalidMarketId);
        }

        MarketStateHeader::validate_all_params(
            self.base_decimals,
            self.quote_decimals,
            self.base_atoms_per_base_lot,
            self.quote_atoms_per_quote_lot,
            self.tick_size_in_quote_atoms_per_base_unit,
            self.raw_base_units_per_base_unit,
            self.maker_fee_ppm,
            self.taker_fee_ppm,
        )
    }

    pub fn validate_fee_config(&self, admin: &Pubkey) -> Result<(), ArcherError> {
        if self.maker_fee_ppm.saturating_add(self.taker_fee_ppm) < 0 {
            return Err(ArcherError::InvalidFee);
        }

        if is_global_authority(admin) {
            return Ok(());
        }

        MarketStateHeader::validate_permissionless_fee_config(
            self.maker_fee_ppm,
            self.taker_fee_ppm,
        )
    }
}


#[derive(BorshDeserialize, BorshSerialize, Debug)]
pub struct SetArcherAccountDelegateParams {
    /// Ceiling on `builder_fee_ppm` for taker swaps funded by this account.
    /// `0` forbids any builder fee.
    pub max_builder_fee_ppm: u32,
}


#[derive(BorshDeserialize, BorshSerialize, Debug)]
pub struct ArcherAccountWithdrawParams {
    /// Token atoms to move out of the account's token account. `0` skips the
    /// token leg, in which case its accounts need not be supplied.
    pub token_amount: u64,

    /// Lamports to move to the owner. Bounded by the rent-exempt minimum.
    pub lamports: u64,
}
