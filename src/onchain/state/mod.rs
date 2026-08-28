pub mod archer_account;
pub mod maker_book;
pub mod maker_registry;
pub mod market_state;

pub use archer_account::*;
pub use maker_book::*;
pub use maker_registry::*;
pub use market_state::*;

use solana_program::program_error::ProgramError;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MarketStatus {
    Active = 0,
    Paused = 1,
    Closed = 2,
}

impl MarketStatus {
    pub fn from_u8(value: u8) -> Result<Self, ProgramError> {
        match value {
            0 => Ok(Self::Active),
            1 => Ok(Self::Paused),
            2 => Ok(Self::Closed),
            _ => Err(ProgramError::InvalidAccountData),
        }
    }

    pub fn as_u8(&self) -> u8 {
        match self {
            MarketStatus::Active => 0,
            MarketStatus::Paused => 1,
            MarketStatus::Closed => 2,
        }
    }

    #[inline]
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    #[inline]
    pub fn is_paused(&self) -> bool {
        matches!(self, Self::Paused)
    }

    #[inline]
    pub fn is_closed(&self) -> bool {
        matches!(self, Self::Closed)
    }
}
