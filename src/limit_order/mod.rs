//! Limit-order abstractions on top of Archer's MakerBook.
//!
//! A user's [`MakerBook`](crate::onchain::MakerBook) is reinterpreted here as a personal
//! limit-order container: up to 16 bids and 16 asks per `(market, owner)` pair,
//! priced by an immutable "anchor" `mid_price_ticks` chosen at the first place.
//!
//! Modifying any single order rewrites the whole book on chain — the program
//! requires `UpdateBook` to carry the complete `[bid_levels; ask_levels]` arrays
//! — but the API here hides that. Callers think in terms of `place`, `modify`,
//! `cancel`, `cancel_all`, `replace_all` over individual orders.

pub mod actions;
pub mod book;
pub mod discovery;
pub mod types;

pub use book::LocalBook;
pub use types::{LimitOrder, LimitOrderBookView, LimitOrderId, LimitOrderRung, NewLimitOrder};
