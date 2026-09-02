use bytemuck::{Pod, Zeroable};
use solana_program::{program_error::ProgramError, pubkey::Pubkey};

use crate::onchain::{
    ArcherError, ArcherUnit, BaseAtoms, BaseAtomsPerLot, BaseLots, MarketStatus, QuoteAtoms,
    QuoteAtomsPerBaseUnitPerTick, QuoteAtomsPerLot, QuoteLots, Ticks,
};

pub const MARKET_STATE_DISCRIMINATOR: &[u8; 8] = b"ACHRMKT1";

pub const MARKET_SEED_PREFIX: &[u8] = b"market";

pub const ARCHER_EXCHANGE_TREASURY: Pubkey =
    solana_program::pubkey!("ELGWUVJD6NBNLyJ5Xv98PzoSg9Wh2Y8Bwep9JZgm9nuo");
pub const ARCHER_PROTOCOL_FEE_PPM: u64 = 200_000;

pub const MIN_FEE_PPM: i32 = -50_000;
pub const MAX_FEE_PPM: i32 = 100_000;
pub const PPM_DIVISOR: u64 = 1_000_000;

pub const ARCHER_GLOBAL_AUTHORITY: Pubkey =
    solana_program::pubkey!("AuthYRbdcyksthcB1AM8ZKB3CidzChURkLzsGN8pYUmw");

pub const PERMISSIONLESS_MAKER_FEE_PPM: i32 = 400;

pub const PERMISSIONLESS_TAKER_FEE_PPM: i32 = 700;

#[inline]
pub fn is_global_authority(signer: &Pubkey) -> bool {
    *signer == ARCHER_GLOBAL_AUTHORITY
}

/// Market state header - fixed size portion
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct MarketStateHeader {
    /// Account discriminator: "ACHRMKT1"
    pub discriminator: [u8; 8],

    /// Unique market identifier (used in PDA derivation)
    /// Market PDA = ["market", market_id]
    pub market_id: Pubkey,

    /// Base token mint address
    pub base_mint: Pubkey,

    /// Quote token mint address
    pub quote_mint: Pubkey,

    /// Base token vault — ATA of market PDA for base_mint
    pub base_vault: Pubkey,

    /// Quote token vault — ATA of market PDA for quote_mint
    pub quote_vault: Pubkey,

    /// Market admin authority
    pub admin: Pubkey,

    /// No. of base atoms per base lot
    pub base_atoms_per_base_lot: BaseAtomsPerLot,

    /// No. of quote atoms per quote lot
    pub quote_atoms_per_quote_lot: QuoteAtomsPerLot,

    /// Tick size in quote atoms per base unit
    pub tick_size_in_quote_atoms_per_base_unit: QuoteAtomsPerBaseUnitPerTick,

    /// Number of raw base units per base unit (for sub-unit pricing)
    pub raw_base_units_per_base_unit: u64,

    /// Uncollected protocol fees in quote lots
    pub uncollected_fees_quote_lots: u64,

    /// Total collected protocol fees in quote lots
    pub collected_fees_quote_lots: u64,

    /// Maker fee in parts per million (negative = rebate)
    pub maker_fee_ppm: i32,

    /// Taker fee in parts per million
    pub taker_fee_ppm: i32,

    /// Base token decimals
    pub base_decimals: u8,

    /// Quote token decimals
    pub quote_decimals: u8,

    /// Market status (Active / Paused / Closed)
    pub status: u8,

    /// Reserved padding. Never read.
    pub reserved_padding_1: u8,

    /// PDA bump seed for the market account
    /// Stored so we can use create_program_address instead of find_program_address
    /// in subsequent instructions, saving ~1500 CUs per call
    pub market_bump: u8,

    /// Reserved padding. Never read.
    pub reserved_padding_2: [u8; 11],
}

unsafe impl Pod for MarketStateHeader {}
unsafe impl Zeroable for MarketStateHeader {}

impl MarketStateHeader {
    pub const LEN: usize = std::mem::size_of::<MarketStateHeader>();

    pub fn get_vault_ata_address(
        market_pda: &Pubkey,
        mint: &Pubkey,
        token_program: &Pubkey,
    ) -> Pubkey {
        spl_associated_token_account::get_associated_token_address_with_program_id(
            market_pda,
            mint,
            token_program,
        )
    }

    pub fn get_status(&self) -> Result<MarketStatus, ProgramError> {
        MarketStatus::from_u8(self.status)
    }

    #[inline]
    pub fn is_active(&self) -> bool {
        self.status == MarketStatus::Active as u8
    }

    #[inline]
    pub fn is_paused(&self) -> bool {
        self.status == MarketStatus::Paused as u8
    }

    #[inline]
    pub fn is_closed(&self) -> bool {
        self.status == MarketStatus::Closed as u8
    }

    pub fn validate_fee_ppm(fee_ppm: i32) -> Result<(), ArcherError> {
        if fee_ppm < MIN_FEE_PPM || fee_ppm > MAX_FEE_PPM {
            return Err(ArcherError::InvalidFee);
        }
        Ok(())
    }

    pub fn validate_taker_fee_ppm(fee_ppm: i32) -> Result<(), ArcherError> {
        Self::validate_fee_ppm(fee_ppm)?;
        if fee_ppm < 0 {
            return Err(ArcherError::NegativeTakerFeeNotAllowed);
        }
        Ok(())
    }

    pub fn validate_permissionless_fee_config(
        maker_fee_ppm: i32,
        taker_fee_ppm: i32,
    ) -> Result<(), ArcherError> {
        if maker_fee_ppm != PERMISSIONLESS_MAKER_FEE_PPM
            || taker_fee_ppm != PERMISSIONLESS_TAKER_FEE_PPM
        {
            return Err(ArcherError::InvalidPermissionlessFeeConfig);
        }
        Ok(())
    }

    pub fn ppm_to_bps(ppm: i32) -> i32 {
        ppm / 100
    }

    pub fn bps_to_ppm(bps: i32) -> i32 {
        bps * 100
    }

    pub fn ppm_to_percentage(ppm: i32) -> (i32, u32) {
        let abs_ppm = ppm.abs();
        let whole = abs_ppm / 10_000;
        let frac = (abs_ppm % 10_000) as u32;

        if ppm < 0 {
            (-whole, frac)
        } else {
            (whole, frac)
        }
    }

    pub fn get_maker_fee_bps(&self) -> i32 {
        Self::ppm_to_bps(self.maker_fee_ppm)
    }

    pub fn get_taker_fee_bps(&self) -> i32 {
        Self::ppm_to_bps(self.taker_fee_ppm)
    }

    pub fn validate_base_lot_size(
        base_atoms_per_base_lot: u64,
        base_decimals: u8,
    ) -> Result<(), ArcherError> {
        if base_atoms_per_base_lot == 0 {
            return Err(ArcherError::InvalidLotSize);
        }

        if base_atoms_per_base_lot > 1_000_000_000_000_000 {
            return Err(ArcherError::InvalidLotSize);
        }

        let base_unit = 10u64
            .checked_pow(base_decimals as u32)
            .ok_or(ArcherError::ArithmeticOverflow)?;

        if base_unit % base_atoms_per_base_lot != 0 && base_atoms_per_base_lot % base_unit != 0 {
            let gcd = Self::gcd(base_unit, base_atoms_per_base_lot);
            if gcd == 1 && base_atoms_per_base_lot != 1 {
                return Err(ArcherError::InvalidLotSize);
            }
        }

        Ok(())
    }

    pub fn validate_quote_lot_size(
        quote_atoms_per_quote_lot: u64,
        quote_decimals: u8,
    ) -> Result<(), ArcherError> {
        if quote_atoms_per_quote_lot == 0 {
            return Err(ArcherError::InvalidLotSize);
        }

        if quote_atoms_per_quote_lot > 1_000_000_000_000_000 {
            return Err(ArcherError::InvalidLotSize);
        }

        let quote_unit = 10u64
            .checked_pow(quote_decimals as u32)
            .ok_or(ArcherError::ArithmeticOverflow)?;

        if quote_unit % quote_atoms_per_quote_lot != 0
            && quote_atoms_per_quote_lot % quote_unit != 0
        {
            let gcd = Self::gcd(quote_unit, quote_atoms_per_quote_lot);
            if gcd == 1 && quote_atoms_per_quote_lot != 1 {
                return Err(ArcherError::InvalidLotSize);
            }
        }

        Ok(())
    }

    pub fn validate_tick_size(
        tick_size_in_quote_atoms_per_base_unit: u64,
        quote_atoms_per_quote_lot: u64,
    ) -> Result<(), ArcherError> {
        if tick_size_in_quote_atoms_per_base_unit == 0 {
            return Err(ArcherError::InvalidTickSize);
        }

        if tick_size_in_quote_atoms_per_base_unit > 1_000_000_000_000_000 {
            return Err(ArcherError::InvalidTickSize);
        }

        let gcd = Self::gcd(
            tick_size_in_quote_atoms_per_base_unit,
            quote_atoms_per_quote_lot,
        );

        if tick_size_in_quote_atoms_per_base_unit % quote_atoms_per_quote_lot != 0
            && quote_atoms_per_quote_lot % tick_size_in_quote_atoms_per_base_unit != 0
        {
            let min_val = tick_size_in_quote_atoms_per_base_unit.min(quote_atoms_per_quote_lot);

            if gcd < min_val / 100 && gcd < 1000 {
                return Err(ArcherError::InvalidTickSize);
            }
        }

        Ok(())
    }

    pub fn validate_raw_base_units(
        raw_base_units_per_base_unit: u64,
        base_decimals: u8,
    ) -> Result<(), ArcherError> {
        if raw_base_units_per_base_unit == 0 {
            return Err(ArcherError::InvalidParams);
        }

        if raw_base_units_per_base_unit != 1 {
            let mut val = raw_base_units_per_base_unit;
            while val > 1 && val % 10 == 0 {
                val /= 10;
            }
            if val != 1 {
                return Err(ArcherError::InvalidParams);
            }
        }

        let max_raw_units = 10u64
            .checked_pow(base_decimals as u32)
            .ok_or(ArcherError::ArithmeticOverflow)?;

        if raw_base_units_per_base_unit > max_raw_units {
            return Err(ArcherError::InvalidParams);
        }

        Ok(())
    }

    pub fn validate_market_invariants(&self) -> Result<(), ArcherError> {
        let base_atoms_per_base_unit = self.base_atoms_per_base_unit()?;

        let min_fill_numerator = (self.base_atoms_per_base_lot.as_u128())
            .checked_mul(self.tick_size_in_quote_atoms_per_base_unit.as_u128())
            .ok_or(ArcherError::ArithmeticOverflow)?;

        let min_quote_atoms = min_fill_numerator
            .checked_div(base_atoms_per_base_unit)
            .ok_or(ArcherError::ArithmeticOverflow)?;

        if min_quote_atoms == 0 {
            return Err(ArcherError::InvalidParams);
        }

        let min_fill_denominator = base_atoms_per_base_unit
            .checked_mul(self.quote_atoms_per_quote_lot.as_u128())
            .ok_or(ArcherError::ArithmeticOverflow)?;

        if min_fill_denominator == 0 || min_fill_numerator % min_fill_denominator != 0 {
            return Err(ArcherError::InvalidParams);
        }

        let max_ticks: u128 = 1_000_000_000_000_000_000;

        let _ = (self.base_atoms_per_base_lot.as_u128())
            .checked_mul(max_ticks)
            .ok_or(ArcherError::ArithmeticOverflow)?
            .checked_mul(self.tick_size_in_quote_atoms_per_base_unit.as_u128())
            .ok_or(ArcherError::ArithmeticOverflow)?
            .checked_div(base_atoms_per_base_unit)
            .ok_or(ArcherError::ArithmeticOverflow)?;

        let max_fee_calc = (u64::MAX as i128)
            .checked_mul(MAX_FEE_PPM as i128)
            .ok_or(ArcherError::ArithmeticOverflow)?
            .checked_div(PPM_DIVISOR as i128)
            .ok_or(ArcherError::ArithmeticOverflow)?;

        if max_fee_calc > i64::MAX as i128 {
            return Err(ArcherError::ArithmeticOverflow);
        }

        Ok(())
    }

    pub fn validate_all_params(
        base_decimals: u8,
        quote_decimals: u8,
        base_atoms_per_base_lot: u64,
        quote_atoms_per_quote_lot: u64,
        tick_size_in_quote_atoms_per_base_unit: u64,
        raw_base_units_per_base_unit: u64,
        maker_fee_ppm: i32,
        taker_fee_ppm: i32,
    ) -> Result<(), ArcherError> {
        if base_decimals > 18 {
            return Err(ArcherError::InvalidParams);
        }
        if quote_decimals > 18 {
            return Err(ArcherError::InvalidParams);
        }

        Self::validate_base_lot_size(base_atoms_per_base_lot, base_decimals)?;
        Self::validate_quote_lot_size(quote_atoms_per_quote_lot, quote_decimals)?;
        Self::validate_tick_size(
            tick_size_in_quote_atoms_per_base_unit,
            quote_atoms_per_quote_lot,
        )?;
        Self::validate_raw_base_units(raw_base_units_per_base_unit, base_decimals)?;

        Self::validate_fee_ppm(maker_fee_ppm)?;
        Self::validate_taker_fee_ppm(taker_fee_ppm)?;

        Ok(())
    }

    pub fn base_atoms_per_base_unit(&self) -> Result<u128, ArcherError> {
        10u128
            .checked_pow(self.base_decimals as u32)
            .ok_or(ArcherError::ArithmeticOverflow)?
            .checked_mul(self.raw_base_units_per_base_unit as u128)
            .ok_or(ArcherError::ArithmeticOverflow)
    }

    pub fn base_lots_to_quote_atoms(
        &self,
        base_lots: BaseLots,
        price_in_ticks: Ticks,
    ) -> Result<QuoteAtoms, ArcherError> {
        let base_atoms = (base_lots.as_u128())
            .checked_mul(self.base_atoms_per_base_lot.as_u128())
            .ok_or(ArcherError::ArithmeticOverflow)?;

        let base_atoms_per_base_unit = self.base_atoms_per_base_unit()?;

        let quote_atoms = base_atoms
            .checked_mul(price_in_ticks.as_u128())
            .ok_or(ArcherError::ArithmeticOverflow)?
            .checked_mul(self.tick_size_in_quote_atoms_per_base_unit.as_u128())
            .ok_or(ArcherError::ArithmeticOverflow)?
            .checked_div(base_atoms_per_base_unit)
            .ok_or(ArcherError::ArithmeticOverflow)?;

        if quote_atoms > u64::MAX as u128 {
            return Err(ArcherError::ArithmeticOverflow);
        }

        Ok(QuoteAtoms::new(quote_atoms as u64))
    }

    pub fn quote_atoms_to_lots(&self, quote_atoms: QuoteAtoms) -> QuoteLots {
        quote_atoms / self.quote_atoms_per_quote_lot
    }

    pub fn quote_atoms_to_lots_ceil(&self, quote_atoms: QuoteAtoms) -> QuoteLots {
        let divisor = self.quote_atoms_per_quote_lot.as_u64();
        let atoms = quote_atoms.as_u64();
        QuoteLots::new(atoms.checked_add(divisor - 1).unwrap_or(u64::MAX) / divisor)
    }

    pub fn quote_lots_to_atoms(&self, quote_lots: QuoteLots) -> QuoteAtoms {
        quote_lots * self.quote_atoms_per_quote_lot
    }

    pub fn base_atoms_to_lots(&self, base_atoms: BaseAtoms) -> BaseLots {
        base_atoms / self.base_atoms_per_base_lot
    }

    pub fn base_lots_to_atoms(&self, base_lots: BaseLots) -> BaseAtoms {
        base_lots * self.base_atoms_per_base_lot
    }

    fn gcd(mut a: u64, mut b: u64) -> u64 {
        while b != 0 {
            let temp = b;
            b = a % b;
            a = temp;
        }
        a
    }
}

/// Market state accessor
pub struct MarketState;

impl MarketState {
    pub fn load_header(data: &[u8]) -> Result<&MarketStateHeader, ArcherError> {
        if data.len() < MarketStateHeader::LEN {
            return Err(ArcherError::InvalidAccount);
        }

        let header = bytemuck::try_from_bytes::<MarketStateHeader>(&data[..MarketStateHeader::LEN])
            .map_err(|_| ArcherError::InvalidAccount)?;

        if &header.discriminator != MARKET_STATE_DISCRIMINATOR {
            return Err(ArcherError::InvalidAccount);
        }

        Ok(header)
    }

}
