//! Rust SDK for the [Archer Protocol](https://archer.exchange).
//!
//! Archer aggregates sovereign market-maker orderbooks into a single atomic
//! execution layer. This crate is what you build against: account layouts,
//! instruction builders, the quoting math, and an optional RPC client.
//!
//! # Layout
//!
//! - [`onchain`] — a transcription of the program's public surface: account
//!   layouts, the instruction enum and its parameters, error codes, events and
//!   constants. Re-exported at the crate root, so [`MarketStateHeader`] and
//!   friends are reachable directly.
//! - [`ix_builder`] — instruction builders taking human-readable amounts.
//! - [`math`] — tick, lot and fee conversions, and book construction.
//! - [`limit_order`] — a limit-order abstraction over maker books.
//! - [`pda`] — address derivation.
//! - [`client`] — an async RPC client, behind the `client` feature.

use solana_program::pubkey::Pubkey;

pub mod onchain;

pub mod accounts;
pub mod archer_account;
#[cfg(feature = "client")]
pub mod client;
pub mod config;
pub mod error;
pub mod identity;
pub mod ix_builder;
pub mod limit_order;
pub mod math;
pub mod pda;
pub mod types;

pub use onchain::*;

pub const ARCHER_V1_PROGRAM_ID: Pubkey =
    solana_program::pubkey!("Archer8kgiavM61GyusMzaaS2ft5sALtNsD1HxkUPMhy");

/// Protocol constants, gathered for discoverability. Every one is read-only:
/// knowing the global authority's address does not let a caller act as it.
pub mod constants {
    pub use crate::onchain::{
        ARCHER_EXCHANGE_TREASURY, ARCHER_GLOBAL_AUTHORITY, ARCHER_PROTOCOL_FEE_PPM,
        EVENT_AUTHORITY_BUMP, EVENT_AUTHORITY_PUBKEY, EVENT_AUTHORITY_SEED, MAX_BUILDER_FEE_PPM,
        MAX_FEE_PPM, MAX_LEVELS, MAX_MAKER_BOOKS_PER_AUCTION, MAX_REGISTRY_MAKERS,
        MAX_SEQUENCE_JUMP, MIN_FEE_PPM, PERMISSIONLESS_MAKER_FEE_PPM,
        PERMISSIONLESS_TAKER_FEE_PPM, PPM_DIVISOR,
    };
}

pub mod prelude {
    #[cfg(feature = "client")]
    pub use crate::client::ArcherClient;
    pub use crate::config::MarketConfig;
    pub use crate::constants;
    pub use crate::error::{ArcherSDKError, SdkResult};
    pub use crate::identity::Identity;
    pub use crate::ix_builder;
    pub use crate::limit_order::{
        actions::{
            build_cancel, build_cancel_all, build_close_book, build_modify, build_place,
            build_replace_all, compute_required_collateral, CollateralArgs, LimitOrderActionResult,
        },
        discovery::{build_ladder, build_ladder_for_side, unique_books_in_order},
        LimitOrder, LimitOrderBookView, LimitOrderId, LimitOrderRung, LocalBook, NewLimitOrder,
    };
    pub use crate::math::{
        levels::build_book_from_spread, levels::build_book_update, BookUpdate, Quote, TwoSidedQuote,
    };
    pub use crate::onchain::state::DelegatedPlatform;
    pub use crate::pda;
    pub use crate::ARCHER_V1_PROGRAM_ID;
}
