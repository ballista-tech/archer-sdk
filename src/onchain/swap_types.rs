use borsh::{BorshDeserialize, BorshSerialize};

use crate::onchain::{ArcherError, PPM_DIVISOR};

pub const MAX_MAKER_BOOKS_PER_AUCTION: usize = 64;

pub const MAX_BUILDER_FEE_PPM: u32 = 10_000;



#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Side {
    Bid = 0,
    Ask = 1,
}

impl Side {
    #[inline(always)]
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Bid),
            1 => Some(Self::Ask),
            _ => None,
        }
    }

    #[inline(always)]
    pub fn is_buy(&self) -> bool {
        matches!(self, Self::Bid)
    }

}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SwapMode {
    MaxAmountIn = 0,
    MinAmountOut = 1,
}

impl SwapMode {
    #[inline(always)]
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::MaxAmountIn),
            1 => Some(Self::MinAmountOut),
            _ => None,
        }
    }
}

#[derive(Copy, Clone, Debug, BorshDeserialize, BorshSerialize)]
pub struct SwapParams {
    /// Quantity of tokens to trade
    /// For MaxAmountIn, this represents the max. amount user is willing to spend
    /// For MinAmountOut, this represents the min. amount user is willing to accept
    pub amount: u64,

    /// Threshold quantity of tokens to accept
    /// For MaxAmountIn, this represents the min. amount user is willing to accept
    /// For MinAmountOut, this represents the max. amount user is willing to spend
    pub threshold: u64,

    /// Side: Bid/Ask
    pub side: u8,

    /// Mode: MaxAmountIn/MinAmountOut
    pub mode: u8,

    /// Builder ("frontend") fee in parts-per-million of the quote notional
    /// traded, paid by the taker on top of the protocol taker fee and routed to
    /// `builder_fee_wallet`. `0` disables it. Capped at [`MAX_BUILDER_FEE_PPM`].
    ///
    /// This is an add-on: it never reduces protocol or maker revenue.
    pub builder_fee_ppm: u32,
}

impl SwapParams {
    pub const LEN: usize = std::mem::size_of::<Self>();

    #[inline(always)]
    pub fn get_side(&self) -> Option<Side> {
        Side::from_u8(self.side)
    }

    #[inline(always)]
    pub fn get_mode(&self) -> Option<SwapMode> {
        SwapMode::from_u8(self.mode)
    }

    #[inline(always)]
    pub fn validate(&self) -> bool {
        self.get_side().is_some()
            && self.get_mode().is_some()
            && self.amount > 0
            && (self.get_mode() != Some(SwapMode::MinAmountOut) || self.threshold > 0)
            && self.builder_fee_ppm <= MAX_BUILDER_FEE_PPM
    }
}

#[inline]
pub fn calculate_builder_fee_quote_lots(
    quote_lots_traded: u64,
    builder_fee_ppm: u32,
) -> Result<u64, ArcherError> {
    if builder_fee_ppm == 0 || quote_lots_traded == 0 {
        return Ok(0);
    }

    let fee = (quote_lots_traded as u128)
        .checked_mul(builder_fee_ppm as u128)
        .ok_or(ArcherError::ArithmeticOverflow)?
        .checked_div(PPM_DIVISOR as u128)
        .ok_or(ArcherError::ArithmeticOverflow)?;

    u64::try_from(fee).map_err(|_| ArcherError::ArithmeticOverflow)
}

