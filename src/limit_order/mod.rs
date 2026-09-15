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
//! Writes are per level: each action diffs the client's snapshot of the book
//! against its intent and emits one compare-and-set op per touched level
//! (`LevelOp`), guarded by the size the client observed. A fill that lands in
//! between fails the instruction (`LevelSizeMismatch`, 527) rather than
//! silently rewriting the old order; see [`CancelMode`] for the one place the
//! guard is relaxed. Callers think in terms of `place`, `modify`, `cancel`,
//! `cancel_all`, `replace_all` over individual orders.

pub mod actions;
pub mod book;
pub mod discovery;
pub mod types;

pub use book::LocalBook;
pub use types::{
    CancelMode, CrossPolicy, LimitOrder, LimitOrderBookView, LimitOrderId, LimitOrderRung,
    NewLimitOrder, PostOnly,
};
