use crate::onchain::{ArcherUnit, MarketStateHeader, MarketStatus};
use solana_program::pubkey::Pubkey;

use crate::error::{ArcherSDKError, SdkResult};

/// Cached market parameters for all price/size/fee conversions
///
/// This struct is the bridge between human readable numbers and on-chain numbers
/// Every conversion function takes a &MarketConfig
///
/// Immutable after construction - if market parameters change on-chain,
/// (e.g. fee update), fetch a new config
#[derive(Debug, Clone)]
pub struct MarketConfig {
    /// The market account pubkey (the PDA itself, not the market_id seed)
    pub market_pubkey: Pubkey,

    /// Base token mint (e.g. SOL mint for a SOL/USDC market)
    pub base_mint: Pubkey,

    /// Quote token mint (e.g. USDC mint for a SOL/USDC market)
    pub quote_mint: Pubkey,

    /// Smallest base token unit per base lot.
    /// E.g., for SOL with 9 decimals and lot size of 1000 atoms: 1000.
    pub base_atoms_per_base_lot: u64,

    /// Smallest quote token unit per quote lot.
    pub quote_atoms_per_quote_lot: u64,

    /// Minimum price increment in quote atoms per base unit.
    /// This defines the tick resolution.
    pub tick_size_in_quote_atoms_per_base_unit: u64,

    /// Decimal scaling factor for base units.
    /// For SOL (9 decimals): 1_000_000_000.
    pub raw_base_units_per_base_unit: u64,

    /// Maker fee in parts per million. Negative = rebate.
    pub maker_fee_ppm: i32,

    /// Taker fee in parts per million. Negative = rebate.
    pub taker_fee_ppm: i32,

    /// Base mint decimals (e.g., 9 for SOL, 8 for BTC).
    pub base_decimals: u8,

    /// Quote mint decimals (e.g., 6 for USDC).
    pub quote_decimals: u8,

    /// Base vault PDA (cached to avoid re-derivation).
    pub base_vault: Pubkey,

    /// Quote vault PDA (cached to avoid re-derivation).
    pub quote_vault: Pubkey,

    /// Base token program
    pub base_token_program: Pubkey,

    /// Quote token program
    pub quote_token_program: Pubkey,

    /// `10^base_decimals` - converts base atoms to token amount
    base_atoms_divisor: f64,

    /// `10^quote_decimals` - converts quote atoms to token amount
    quote_atoms_divisor: f64,

    /// Precomputed: `tick_size / (raw_base_units_per_base_unit * 10^quote_decimals)`.
    /// Multiply by ticks to get a human-readable price.
    /// Divide a human-readable price by this to get ticks.
    ticks_to_price_factor: f64,

    /// Precomputed: `base_atoms_per_base_lot / 10^base_decimals`.
    /// Multiply by lots to get a human-readable base amount.
    lots_to_base_amount_factor: f64,

    /// Precomputed: `quote_atoms_per_quote_lot / 10^quote_decimals`.
    /// Multiply by lots to get a human-readable quote amount.
    lots_to_quote_amount_factor: f64,

    /// Precomputed: `tick_size * base_atoms_per_base_lot`.
    /// Numerator of the on-chain `quote_lots = base_lots * ticks * num / den`
    /// conversion (mirrors `MakerBook::tick_conversion_num`).
    tick_conversion_num: u128,

    /// Precomputed: `10^base_decimals * raw_base_units_per_base_unit
    /// * quote_atoms_per_quote_lot`.
    /// Denominator of the on-chain quote-lot conversion (mirrors
    /// `MakerBook::tick_conversion_den`).
    tick_conversion_den: u128,
}

impl MarketConfig {
    /// Construct a `MarketConfig` from an on-chain `MarketStateHeader`
    /// and the token mint decimals.
    ///
    /// `market_pubkey` is the PDA address of the market account itself.
    /// `base_decimals` / `quote_decimals` come from the SPL mint accounts.
    pub fn from_header(
        market_pubkey: Pubkey,
        header: &MarketStateHeader,
        base_decimals: u8,
        quote_decimals: u8,
        base_token_program: Pubkey,
        quote_token_program: Pubkey,
    ) -> Self {
        let base_atoms_divisor = 10f64.powi(base_decimals as i32);
        let quote_atoms_divisor = 10f64.powi(quote_decimals as i32);

        // price_in_quote_per_base = ticks * tick_size_in_quote_atoms_per_base_unit
        //                           / (raw_base_units_per_base_unit * 10^quote_decimals)
        //
        // raw_base_units_per_base_unit scales the base unit, so it divides the
        // price — matching the on-chain `base_lots_to_quote_atoms` formula
        // (`quote_atoms = base_atoms * ticks * tick_size
        //   / (10^base_decimals * raw_base_units_per_base_unit)`).
        let ticks_to_price_factor = (header.tick_size_in_quote_atoms_per_base_unit.as_u64() as f64)
            / (header.raw_base_units_per_base_unit as f64 * quote_atoms_divisor);

        let lots_to_base_amount_factor =
            header.base_atoms_per_base_lot.as_u64() as f64 / base_atoms_divisor;

        let lots_to_quote_amount_factor =
            header.quote_atoms_per_quote_lot.as_u64() as f64 / quote_atoms_divisor;

        // quote_lots = ceil(base_lots * ticks * num / den), matching the
        // on-chain `MakerBook` tick-conversion cache. raw_base_units_per_base_unit
        // sits in the denominator (it widens the base unit).
        let tick_conversion_num = (header.tick_size_in_quote_atoms_per_base_unit.as_u64() as u128)
            .saturating_mul(header.base_atoms_per_base_lot.as_u64() as u128);
        let tick_conversion_den = 10u128
            .pow(base_decimals as u32)
            .saturating_mul(header.raw_base_units_per_base_unit as u128)
            .saturating_mul(header.quote_atoms_per_quote_lot.as_u64() as u128);

        let base_vault = MarketStateHeader::get_vault_ata_address(
            &market_pubkey,
            &header.base_mint,
            &base_token_program,
        );
        let quote_vault = MarketStateHeader::get_vault_ata_address(
            &market_pubkey,
            &header.quote_mint,
            &quote_token_program,
        );

        Self {
            market_pubkey,
            base_mint: header.base_mint,
            quote_mint: header.quote_mint,
            base_atoms_per_base_lot: header.base_atoms_per_base_lot.as_u64(),
            quote_atoms_per_quote_lot: header.quote_atoms_per_quote_lot.as_u64(),
            tick_size_in_quote_atoms_per_base_unit: header
                .tick_size_in_quote_atoms_per_base_unit
                .as_u64(),
            raw_base_units_per_base_unit: header.raw_base_units_per_base_unit,
            maker_fee_ppm: header.maker_fee_ppm,
            taker_fee_ppm: header.taker_fee_ppm,
            base_decimals,
            quote_decimals,
            base_vault,
            quote_vault,
            base_token_program,
            quote_token_program,
            base_atoms_divisor,
            quote_atoms_divisor,
            ticks_to_price_factor,
            lots_to_base_amount_factor,
            lots_to_quote_amount_factor,
            tick_conversion_num,
            tick_conversion_den,
        }
    }

    /// Quote lots locked on-chain for `base_lots` posted at `price_ticks`.
    ///
    /// Uses ceiling division to match `MakerBook::compute_quote_lots_ceiling`
    /// exactly — `quote_lots = ceil(base_lots * price_ticks * num / den)`.
    pub fn quote_lots_ceil(&self, base_lots: u64, price_ticks: u64) -> u64 {
        if self.tick_conversion_den == 0 {
            return 0;
        }
        let numerator = (base_lots as u128)
            .saturating_mul(price_ticks as u128)
            .saturating_mul(self.tick_conversion_num);
        let quote_lots =
            numerator.saturating_add(self.tick_conversion_den - 1) / self.tick_conversion_den;
        quote_lots.min(u64::MAX as u128) as u64
    }

    /// Price factor: multiply ticks by this to get human-readable price.
    #[inline]
    pub fn ticks_to_price_factor(&self) -> f64 {
        self.ticks_to_price_factor
    }

    /// Inverse price factor: multiply human price by this to get ticks.
    #[inline]
    pub fn price_to_ticks_factor(&self) -> f64 {
        1.0 / self.ticks_to_price_factor
    }

    /// Lots-to-base-amount factor.
    #[inline]
    pub fn lots_to_base_factor(&self) -> f64 {
        self.lots_to_base_amount_factor
    }

    /// Base-amount-to-lots factor.
    #[inline]
    pub fn base_to_lots_factor(&self) -> f64 {
        1.0 / self.lots_to_base_amount_factor
    }

    /// Lots-to-quote-amount factor.
    #[inline]
    pub fn lots_to_quote_factor(&self) -> f64 {
        self.lots_to_quote_amount_factor
    }

    /// Quote-amount-to-lots factor.
    #[inline]
    pub fn quote_to_lots_factor(&self) -> f64 {
        1.0 / self.lots_to_quote_amount_factor
    }

    /// Base atoms per human token unit (10^decimals).
    #[inline]
    pub fn base_atoms_divisor(&self) -> f64 {
        self.base_atoms_divisor
    }

    /// Quote atoms per human token unit (10^decimals).
    #[inline]
    pub fn quote_atoms_divisor(&self) -> f64 {
        self.quote_atoms_divisor
    }

    /// Check that the market is in Active state.
    pub fn require_active(&self, status: u8) -> SdkResult<()> {
        if status != MarketStatus::Active as u8 {
            return Err(ArcherSDKError::MarketNotActive(status));
        }
        Ok(())
    }
}
