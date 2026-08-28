//! Instruction construction for all Archer protocol operations.
//!
//! These modules wrap the program crate's instruction builders with
//! human-readable amount conversion. The SDK handles f64 → atom/lot
//! conversion and PDA derivation; serialization delegates to the
//! program crate's existing builders.
//!
//! # Modules
//!
//! - [`maker`] — Maker operations (deposit, withdraw, update book, clear, delegate).
//! - [`swap`] — Taker swap instructions (sync FOK)
//!
//! # Re-exports
//!
//! The program crate's parameter types are re-exported here so callers
//! are re-exported here so callers can name them without reaching into
//! [`crate::onchain`].

pub mod maker;
pub mod market;
pub mod swap;

// Re-export program crate types that SDK callers need for instruction building.
pub use crate::onchain::{
    builders::UpdateBookParams,
    builders::UpdateMidPriceParams,

    // Swap params
    swap_types::SwapParams,
    CollectProtocolFeeParams,
    InitializeMarketParams,
    // Maker params
    MakerDepositFundsParams,
    MakerWithdrawFundsParams,
};
