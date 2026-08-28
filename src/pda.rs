use crate::onchain::state::DelegatedPlatform;
use crate::onchain::{
    ARCHER_ACCOUNT_SEED_PREFIX, EVENT_AUTHORITY_PUBKEY, EVENT_AUTHORITY_SEED,
    MAKER_BOOK_SEED_PREFIX, MAKER_REGISTRY_SEED_PREFIX, MARKET_SEED_PREFIX,
};
use solana_program::pubkey::Pubkey;

use crate::ARCHER_V1_PROGRAM_ID;

/// Derive the market state PDA
///
/// Seeds: `["market", market_id]`
#[inline]
pub fn derive_market(market_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[MARKET_SEED_PREFIX, market_id.as_ref()],
        &ARCHER_V1_PROGRAM_ID,
    )
}

/// Derive a maker book PDA
///
/// Seeds: `["maker", market_pubkey, maker_pubkey]`
#[inline]
pub fn derive_maker_book(market: &Pubkey, maker: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[MAKER_BOOK_SEED_PREFIX, market.as_ref(), maker.as_ref()],
        &ARCHER_V1_PROGRAM_ID,
    )
}

/// Derive the maker registry PDA for a market
///
/// Seeds: `["maker_registry", market_pubkey]`
#[inline]
pub fn derive_maker_registry(market: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[MAKER_REGISTRY_SEED_PREFIX, market.as_ref()],
        &ARCHER_V1_PROGRAM_ID,
    )
}

pub fn verify_market_pda(expected: &Pubkey, market_id: &Pubkey) -> Option<u8> {
    let (derived, bump) = derive_market(market_id);
    if derived == *expected {
        Some(bump)
    } else {
        None
    }
}

pub fn verify_maker_book_pda(expected: &Pubkey, market: &Pubkey, maker: &Pubkey) -> Option<u8> {
    let (derived, bump) = derive_maker_book(market, maker);
    if derived == *expected {
        Some(bump)
    } else {
        None
    }
}

/// Derive an ArcherAccount PDA.
///
/// Seeds: `["archer-account", owner, platform]` — one account per (wallet,
/// platform), so each platform derives its own address for a user with no
/// coordination and nothing stored at onboarding.
#[inline]
pub fn derive_archer_account(owner: &Pubkey, platform: DelegatedPlatform) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[ARCHER_ACCOUNT_SEED_PREFIX, owner.as_ref(), platform.seed()],
        &ARCHER_V1_PROGRAM_ID,
    )
}

pub fn verify_archer_account_pda(
    expected: &Pubkey,
    owner: &Pubkey,
    platform: DelegatedPlatform,
) -> Option<u8> {
    let (derived, bump) = derive_archer_account(owner, platform);
    if derived == *expected {
        Some(bump)
    } else {
        None
    }
}

#[inline]
pub fn derive_event_authority() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[EVENT_AUTHORITY_SEED], &ARCHER_V1_PROGRAM_ID)
}

#[inline]
pub fn event_authority() -> Pubkey {
    EVENT_AUTHORITY_PUBKEY
}

#[inline]
pub fn verify_event_authority(expected: &Pubkey) -> bool {
    *expected == EVENT_AUTHORITY_PUBKEY
}
