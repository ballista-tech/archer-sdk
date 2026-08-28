use bytemuck::{Pod, Zeroable};
use solana_program::pubkey::Pubkey;

use crate::onchain::ArcherError;

pub const ARCHER_ACCOUNT_DISCRIMINATOR: &[u8; 8] = b"ACHRACC1";

pub const ARCHER_ACCOUNT_SEED_PREFIX: &[u8] = b"archer-account";

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DelegatedPlatform {
    SelfManaged = 0,
    TreadFi = 1,
}

impl DelegatedPlatform {
    #[inline(always)]
    pub const fn seed(self) -> &'static [u8] {
        match self {
            Self::SelfManaged => b"self",
            Self::TreadFi => b"treadfi",
        }
    }

    #[inline(always)]
    pub fn from_u8(value: u8) -> Result<Self, ArcherError> {
        match value {
            0 => Ok(Self::SelfManaged),
            1 => Ok(Self::TreadFi),
            _ => Err(ArcherError::InvalidDelegatedPlatform),
        }
    }

    #[inline(always)]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

impl Default for DelegatedPlatform {
    fn default() -> Self {
        Self::SelfManaged
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct ArcherAccount {
    /// Account discriminator: "ACHRACC1"
    pub discriminator: [u8; 8],

    /// The wallet this account belongs to. The only key that can move value out.
    pub owner: Pubkey,

    /// Currently authorized delegate. `Pubkey::default()` means none.
    pub delegate: Pubkey,

    /// Owner-set ceiling on `builder_fee_ppm` for taker swaps funded by this
    /// account. `0` (the default) forbids any builder fee.
    pub max_builder_fee_ppm: u32,

    /// [`DelegatedPlatform`] discriminant. Also the third PDA seed.
    pub platform: u8,

    pub bump: u8,

    pub reserved_padding_1: u16,

    /// Zeroed at initialization. Headroom for anything deferred.
    pub reserved_padding_2: [u64; 12],
}

impl ArcherAccount {
    pub const LEN: usize = core::mem::size_of::<Self>();

    #[inline(always)]
    pub fn load(data: &[u8]) -> Result<&Self, ArcherError> {
        if data.len() < Self::LEN {
            return Err(ArcherError::InvalidArcherAccount);
        }

        let account = bytemuck::try_from_bytes::<Self>(&data[..Self::LEN])
            .map_err(|_| ArcherError::InvalidArcherAccount)?;

        if &account.discriminator != ARCHER_ACCOUNT_DISCRIMINATOR {
            return Err(ArcherError::InvalidArcherAccount);
        }

        Ok(account)
    }

    #[inline(always)]
    pub fn get_platform(&self) -> Result<DelegatedPlatform, ArcherError> {
        DelegatedPlatform::from_u8(self.platform)
    }

    #[inline(always)]
    pub fn is_delegate_set(&self) -> bool {
        self.delegate != Pubkey::default()
    }

}

const _: () = {
    assert!(
        ArcherAccount::LEN == 176,
        "ArcherAccount layout changed — this is a live account size"
    );
    assert!(ArcherAccount::LEN % 8 == 0, "ArcherAccount must be 8-aligned");
};

