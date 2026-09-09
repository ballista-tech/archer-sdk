//! Limit-order abstractions on top of Archer's MakerBook.
//!
//! A user's [`MakerBook`](crate::onchain::MakerBook) is reinterpreted here as a personal
//! limit-order container: up to 16 bids and 16 asks per `(market, owner)` pair.
//! LO books (`kind == MAKER_KIND_LO`) have `mid_price_ticks` pinned at 0 by the
//! program, so every level's `price_offset_ticks` *is* its absolute price in
//! ticks — a stable, price-keyed order id.
//!
//! Every limit-order write is an `UpdateBookLimit` (see [`PostOnly`]): the
//! program enforces post-only against the market's registered makers at
//! placement. `UpdateBook` is the market-maker path.
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
pub use types::{
    CrossPolicy, LimitOrder, LimitOrderBookView, LimitOrderId, LimitOrderRung, NewLimitOrder,
    PostOnly,
};
