//! Local-mutation primitives for a user's limit-order book.
//!
//! [`LocalBook`] is the SDK's scratch representation of a user's intended state.
//! It is **anchor-bound**: at construction it captures the anchor mid the book
//! is operating under, and all subsequent mutations expect IDs whose absolute
//! price ticks can be reduced to an offset under that anchor. Render-time
//! ([`LocalBook::to_maker_levels`]) emits the sorted bid/ask arrays that go
//! into a single `UpdateBook` instruction.

use crate::onchain::{ArcherUnit, BaseLots, MakerBook, MakerLevel, Side, MAX_LEVELS};

use crate::config::MarketConfig;
use crate::error::{ArcherSDKError, SdkResult};
use crate::math::lots::base_amount_to_lots;
use crate::math::ticks::price_to_ticks;

use super::types::{LimitOrderId, NewLimitOrder};

/// User's intended set of limit orders, prior to submission.
///
/// Internally stores levels as `(side, offset_ticks, size_lots)` — the form
/// the program wants — but the public API speaks in absolute-tick
/// [`LimitOrderId`]s. The anchor is captured once at construction and used
/// implicitly for every ID ↔ offset conversion.
#[derive(Debug, Clone)]
pub struct LocalBook {
    anchor_mid_ticks: u64,
    levels: Vec<(Side, i64, u64)>,
}

impl LocalBook {
    /// Empty book under the given anchor.
    pub fn new(anchor_mid_ticks: u64) -> Self {
        Self {
            anchor_mid_ticks,
            levels: Vec::new(),
        }
    }

    /// Build a local book from an on-chain MakerBook (anchor + active levels).
    pub fn from_maker_book(book: &MakerBook) -> Self {
        let mut levels = Vec::new();
        for l in &book.bid_levels {
            let sz = l.size_in_base_lots.as_u64();
            if sz > 0 {
                levels.push((Side::Bid, l.price_offset_ticks, sz));
            }
        }
        for l in &book.ask_levels {
            let sz = l.size_in_base_lots.as_u64();
            if sz > 0 {
                levels.push((Side::Ask, l.price_offset_ticks, sz));
            }
        }
        Self {
            anchor_mid_ticks: book.mid_price_ticks,
            levels,
        }
    }

    /// The anchor mid (in ticks) this book is operating under.
    #[inline]
    pub fn anchor(&self) -> u64 {
        self.anchor_mid_ticks
    }

    /// Number of active orders on a given side.
    pub fn side_count(&self, side: Side) -> usize {
        self.levels.iter().filter(|(s, _, _)| *s == side).count()
    }

    /// Append a fresh order. Errors if an order already exists at that price
    /// (limit-order books don't merge — each level is a distinct order) or if
    /// the side is already at the 16-level cap.
    pub fn place(&mut self, id: LimitOrderId, size_lots: u64) -> SdkResult<()> {
        if size_lots == 0 {
            return Err(ArcherSDKError::InvalidSize(0.0));
        }
        let offset = self.offset_for(id)?;
        if self
            .levels
            .iter()
            .any(|(s, o, _)| *s == id.side && *o == offset)
        {
            return Err(ArcherSDKError::LimitOrderAlreadyExists {
                side: id.side,
                price_ticks: id.price_ticks,
            });
        }
        if self.side_count(id.side) >= MAX_LEVELS {
            return Err(ArcherSDKError::BookFull { side: id.side });
        }
        self.levels.push((id.side, offset, size_lots));
        Ok(())
    }

    /// Remove an order by ID.
    pub fn cancel(&mut self, id: LimitOrderId) -> SdkResult<()> {
        let offset = self.offset_for(id)?;
        let idx = self
            .levels
            .iter()
            .position(|(s, o, _)| *s == id.side && *o == offset)
            .ok_or(ArcherSDKError::LimitOrderNotFound {
                side: id.side,
                price_ticks: id.price_ticks,
            })?;
        self.levels.remove(idx);
        Ok(())
    }

    /// Change the size of an existing order without moving it.
    /// Zero size cancels it.
    pub fn modify_size(&mut self, id: LimitOrderId, new_size_lots: u64) -> SdkResult<()> {
        if new_size_lots == 0 {
            return self.cancel(id);
        }
        let offset = self.offset_for(id)?;
        let entry = self
            .levels
            .iter_mut()
            .find(|(s, o, _)| *s == id.side && *o == offset)
            .ok_or(ArcherSDKError::LimitOrderNotFound {
                side: id.side,
                price_ticks: id.price_ticks,
            })?;
        entry.2 = new_size_lots;
        Ok(())
    }

    /// Discard everything.
    pub fn clear(&mut self) {
        self.levels.clear();
    }

    /// Render to sorted (bids, asks) `MakerLevel` arrays ready for
    /// `UpdateBookParams`. Bids are descending-price (highest offset first);
    /// asks are ascending-price (lowest offset first). Fails if either side
    /// exceeds 16 levels or if the book is crossed.
    pub fn to_maker_levels(&self) -> SdkResult<(Vec<MakerLevel>, Vec<MakerLevel>)> {
        let mut bids: Vec<MakerLevel> = self
            .levels
            .iter()
            .filter(|(s, _, _)| *s == Side::Bid)
            .map(|(_, o, sz)| MakerLevel {
                size_in_base_lots: BaseLots::new(*sz),
                price_offset_ticks: *o,
            })
            .collect();
        let mut asks: Vec<MakerLevel> = self
            .levels
            .iter()
            .filter(|(s, _, _)| *s == Side::Ask)
            .map(|(_, o, sz)| MakerLevel {
                size_in_base_lots: BaseLots::new(*sz),
                price_offset_ticks: *o,
            })
            .collect();

        // Bids: strictly descending price → strictly descending offset.
        bids.sort_by(|a, b| b.price_offset_ticks.cmp(&a.price_offset_ticks));
        // Asks: strictly ascending price → strictly ascending offset.
        asks.sort_by(|a, b| a.price_offset_ticks.cmp(&b.price_offset_ticks));

        if bids.len() > MAX_LEVELS {
            return Err(ArcherSDKError::TooManyLevels { count: bids.len() });
        }
        if asks.len() > MAX_LEVELS {
            return Err(ArcherSDKError::TooManyLevels { count: asks.len() });
        }

        if let (Some(bid), Some(ask)) = (bids.first(), asks.first()) {
            if bid.price_offset_ticks >= ask.price_offset_ticks {
                return Err(ArcherSDKError::CrossedBookOffsets {
                    bid_offset: bid.price_offset_ticks,
                    ask_offset: ask.price_offset_ticks,
                });
            }
        }

        Ok((bids, asks))
    }

    /// Compute the i64 offset of an ID under this book's anchor.
    ///
    /// O(1) — one subtraction, two `try_from` checks. Surfaces
    /// `OffsetOverflow` if the ID's absolute price is so far from the anchor
    /// that the difference doesn't fit in `i64`. With the program's own
    /// invariant `mid > 0` and `mid + offset > 0` for every active level,
    /// this is only triggered by genuinely pathological inputs.
    fn offset_for(&self, id: LimitOrderId) -> SdkResult<i64> {
        if self.anchor_mid_ticks == 0 {
            return Err(ArcherSDKError::AnchorMidUninitialized);
        }
        let price_i64 =
            i64::try_from(id.price_ticks).map_err(|_| ArcherSDKError::OffsetOverflow {
                price: id.price_ticks as f64,
                mid: self.anchor_mid_ticks as f64,
            })?;
        let anchor_i64 =
            i64::try_from(self.anchor_mid_ticks).map_err(|_| ArcherSDKError::OffsetOverflow {
                price: id.price_ticks as f64,
                mid: self.anchor_mid_ticks as f64,
            })?;
        price_i64
            .checked_sub(anchor_i64)
            .ok_or(ArcherSDKError::OffsetOverflow {
                price: id.price_ticks as f64,
                mid: self.anchor_mid_ticks as f64,
            })
    }
}

/// Resolve a user-supplied [`NewLimitOrder`] (human price + size) into its
/// canonical `(LimitOrderId, size_lots)` form.
///
/// Anchor-independent: the ID is the absolute tick price; whatever anchor the
/// resulting order ends up under is resolved later by [`LocalBook::place`].
pub fn resolve_new_order(
    new: &NewLimitOrder,
    config: &MarketConfig,
) -> SdkResult<(LimitOrderId, u64)> {
    let price_ticks = price_to_ticks(new.price, config)?;
    let size_lots = base_amount_to_lots(new.size, config)?;
    Ok((LimitOrderId::new(new.side, price_ticks), size_lots))
}
