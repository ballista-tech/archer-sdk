use bytemuck::{Pod, Zeroable};
use solana_program::pubkey::Pubkey;

use crate::onchain::ArcherError;

pub const MAKER_REGISTRY_DISCRIMINATOR: &[u8; 8] = b"ACHRREG1";
pub const MAKER_REGISTRY_SEED_PREFIX: &[u8] = b"maker_registry";
/// Capacity of the `makers` array. Slots from `MAX_SEATED_MAKERS` on are
/// never used; the layout keeps them so the account size does not change.
pub const MAX_REGISTRY_MAKERS: usize = 64;
/// The most books a registry seats.
pub const MAX_SEATED_MAKERS: usize = 48;

/// The per-market list of `MakerBook` addresses aggregators route through.
///
/// 2128 bytes, one per market. Membership changes two ways: an LO book
/// seats itself when it places a level through `UpdateBookLimit`, and the
/// global authority replaces the whole set with `UpdateRegistry` (or one
/// seat with `RegisterMaker` / `DeregisterMaker`). Every change increments
/// `version`; `UpdateRegistry` is a compare-and-set on it.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct MakerRegistry {
    /// Account discriminator: "ACHRREG1"
    pub discriminator: [u8; 8],

    /// The market this registry belongs to
    pub market: Pubkey,

    /// The global authority, recorded at creation. Not read by the program.
    pub admin: Pubkey,

    /// Number of registered makers, at most `MAX_SEATED_MAKERS`
    pub num_makers: u8,

    /// Reserved padding
    pub reserved_padding_1: [u8; 3],

    /// u32 LE, incremented on every change to the maker set. Kept as bytes so
    /// the struct's alignment stays 1 and it loads from any byte slice.
    pub version: [u8; 4],

    /// Registered maker book pubkeys; the first `num_makers` are live
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

    /// The registry's change counter, the `expected_version` an
    /// `UpdateRegistry` built from this read must carry.
    #[inline(always)]
    pub fn version(&self) -> u32 {
        u32::from_le_bytes(self.version)
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

    /// The seated books, in registry order.
    #[inline(always)]
    pub fn get_makers(&self) -> &[Pubkey] {
        let count = (self.num_makers as usize).min(MAX_REGISTRY_MAKERS);
        &self.makers[..count]
    }
}
