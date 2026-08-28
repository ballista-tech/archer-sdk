use bytemuck::{Pod, Zeroable};
use solana_program::pubkey::Pubkey;

use crate::onchain::ArcherError;

pub const MAKER_REGISTRY_DISCRIMINATOR: &[u8; 8] = b"ACHRREG1";
pub const MAKER_REGISTRY_SEED_PREFIX: &[u8] = b"maker_registry";
pub const MAX_REGISTRY_MAKERS: usize = 64;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct MakerRegistry {
    /// Account discriminator: "ACHRREG1"
    pub discriminator: [u8; 8],

    /// The market this registry belongs to
    pub market: Pubkey,

    /// Who can add/remove makers (only global admin for now)
    pub admin: Pubkey,

    /// Number of registered makers
    pub num_makers: u8,

    /// Reserved padding
    pub reserved_padding_1: [u8; 7],

    /// Registered maker book pubkeys
    pub makers: [Pubkey; MAX_REGISTRY_MAKERS],
}

impl MakerRegistry {
    pub const LEN: usize = core::mem::size_of::<Self>();

    #[inline(always)]
    pub fn load(data: &[u8]) -> Result<&Self, ArcherError> {
        if data.len() < Self::LEN {
            return Err(ArcherError::InvalidMakerRegistry);
        }

        let registry = bytemuck::try_from_bytes::<Self>(&data[..Self::LEN])
            .map_err(|_| ArcherError::InvalidMakerRegistry)?;

        if &registry.discriminator != MAKER_REGISTRY_DISCRIMINATOR {
            return Err(ArcherError::InvalidMakerRegistry);
        }

        Ok(registry)
    }

    #[inline(always)]
    pub fn contains(&self, maker_book: &Pubkey) -> bool {
        let count = self.num_makers as usize;
        for i in 0..count {
            if self.makers[i] == *maker_book {
                return true;
            }
        }
        false
    }

}
