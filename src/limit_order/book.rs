//! Local snapshot of a user's limit-order book, and the ops that change it.
//!
//! [`LocalBook`] holds the level set the client last observed on chain. Every
//! mutation (`place`, `cancel`, `resize`) both updates the snapshot and returns
//! the [`LevelOp`] that makes the chain agree — carrying the observed size as
//! the op's compare-and-set guard, so a fill that lands in between fails the
//! instruction (`LevelSizeMismatch`, 527) instead of resurrecting the order.
//!
//! LO books are anchored at mid 0, so every level's offset *is* its absolute
//! price in ticks and a [`LimitOrderId`] maps to a level directly.

use crate::onchain::{ArcherUnit, LevelOp, MakerBook, Side, MAX_LEVELS};

use crate::config::MarketConfig;
use crate::error::{ArcherSDKError, SdkResult};
use crate::math::lots::base_amount_to_lots;
use crate::math::ticks::price_to_ticks;

use super::types::{CancelMode, LimitOrderId, NewLimitOrder};

/// The level set the client last saw, keyed by `(side, absolute price)`.
#[derive(Debug, Clone, Default)]
pub struct LocalBook {
    levels: Vec<(LimitOrderId, u64)>,
}

impl LocalBook {
    /// Empty book — what a freshly initialized LO book looks like.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Snapshot an on-chain LO book. Fails if a level's absolute price does
    /// not fit (`OffsetOverflow`), which cannot happen for a book the program
    /// accepted.
    pub fn from_maker_book(book: &MakerBook) -> SdkResult<Self> {
        let anchor = book.mid_price_ticks;
        let mut levels = Vec::with_capacity(MAX_LEVELS * 2);
        for (side, side_levels) in [(Side::Bid, &book.bid_levels), (Side::Ask, &book.ask_levels)] {
            for l in side_levels.iter().filter(|l| l.is_active()) {
                let price = l
                    .absolute_price(anchor)
                    .ok_or(ArcherSDKError::OffsetOverflow {
                        price: l.price_offset_ticks as f64,
                        mid: anchor as f64,
                    })?;
                levels.push((LimitOrderId::new(side, price), l.size_in_base_lots.as_u64()));
            }
        }
        Ok(Self { levels })
    }

    /// Resting size at `id`, if any.
    pub fn size_of(&self, id: LimitOrderId) -> Option<u64> {
        self.levels.iter().find(|(i, _)| *i == id).map(|(_, s)| *s)
    }

    /// Number of active orders on a side.
    pub fn side_count(&self, side: Side) -> usize {
        self.levels.iter().filter(|(i, _)| i.side == side).count()
    }

    /// All orders, in snapshot order.
    pub fn orders(&self) -> impl Iterator<Item = (LimitOrderId, u64)> + '_ {
        self.levels.iter().copied()
    }

    /// Best (highest) bid and best (lowest) ask, in ticks.
    pub fn best_prices(&self) -> (Option<u64>, Option<u64>) {
        let best_bid = self
            .levels
            .iter()
            .filter(|(i, _)| i.side == Side::Bid)
            .map(|(i, _)| i.price_ticks)
            .max();
        let best_ask = self
            .levels
            .iter()
            .filter(|(i, _)| i.side == Side::Ask)
            .map(|(i, _)| i.price_ticks)
            .min();
        (best_bid, best_ask)
    }

    /// The program rejects a book whose best bid reaches its best ask
    /// (`CrossingOrderLevels`); catch it before spending a transaction.
    pub fn check_not_crossed(&self) -> SdkResult<()> {
        if let (Some(bid), Some(ask)) = self.best_prices() {
            if bid >= ask {
                return Err(ArcherSDKError::CrossedBookOffsets {
                    bid_offset: bid as i64,
                    ask_offset: ask as i64,
                });
            }
        }
        Ok(())
    }

    /// Rest a new order. Errors if one already rests at that price (levels
    /// are distinct orders, they do not merge) or the side is full. The op
    /// expects an empty slot, so it fails on chain if something appeared there.
    pub fn place(&mut self, id: LimitOrderId, size_lots: u64) -> SdkResult<LevelOp> {
        if size_lots == 0 {
            return Err(ArcherSDKError::InvalidSize(0.0));
        }
        if id.price_ticks == 0 {
            return Err(ArcherSDKError::PriceBelowResolution(0.0));
        }
        if self.size_of(id).is_some() {
            return Err(ArcherSDKError::LimitOrderAlreadyExists {
                side: id.side,
                price_ticks: id.price_ticks,
            });
        }
        if self.side_count(id.side) >= MAX_LEVELS {
            return Err(ArcherSDKError::BookFull { side: id.side });
        }
        self.levels.push((id, size_lots));
        Ok(LevelOp::place(id.side, id.price_ticks, size_lots))
    }

    /// Remove an order. Under [`CancelMode::Any`] the op cancels whatever
    /// still rests (a partial fill in between is fine — nothing can be
    /// resurrected by writing zero); under [`CancelMode::Strict`] it carries
    /// the observed size and fails on chain if that changed.
    pub fn cancel(&mut self, id: LimitOrderId, mode: CancelMode) -> SdkResult<LevelOp> {
        let observed = self.remove(id)?;
        Ok(match mode {
            CancelMode::Any => LevelOp::cancel_any(id.side, id.price_ticks),
            CancelMode::Strict => LevelOp::resize(id.side, id.price_ticks, observed, 0),
        })
    }

    /// Change an order's size in place. Always strict: writing a new size
    /// over a level that was partially filled in between would re-expose the
    /// filled lots, so the op fails on chain instead. Zero size is a strict
    /// cancel.
    pub fn resize(&mut self, id: LimitOrderId, new_size_lots: u64) -> SdkResult<LevelOp> {
        if new_size_lots == 0 {
            return self.cancel(id, CancelMode::Strict);
        }
        let entry = self
            .levels
            .iter_mut()
            .find(|(i, _)| *i == id)
            .ok_or(ArcherSDKError::LimitOrderNotFound {
                side: id.side,
                price_ticks: id.price_ticks,
            })?;
        let observed = entry.1;
        entry.1 = new_size_lots;
        Ok(LevelOp::resize(id.side, id.price_ticks, observed, new_size_lots))
    }

    /// Ops that turn this snapshot into exactly `desired`: cancels for levels
    /// only in the snapshot, resizes for levels in both with a different size,
    /// places for levels only in `desired`. Emitted in that order so no
    /// intermediate state exceeds 16 levels a side or crosses if `desired`
    /// itself does not. Levels already at the desired size emit nothing.
    /// `desired` must not name the same `(side, price)` twice.
    pub fn diff_to(
        &mut self,
        desired: &[(LimitOrderId, u64)],
        mode: CancelMode,
    ) -> SdkResult<Vec<LevelOp>> {
        for (n, (id, size)) in desired.iter().enumerate() {
            if *size == 0 {
                return Err(ArcherSDKError::InvalidSize(0.0));
            }
            if desired[..n].iter().any(|(other, _)| other == id) {
                return Err(ArcherSDKError::LimitOrderAlreadyExists {
                    side: id.side,
                    price_ticks: id.price_ticks,
                });
            }
        }

        let mut ops = Vec::with_capacity(self.levels.len() + desired.len());

        let to_cancel: Vec<LimitOrderId> = self
            .levels
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| !desired.iter().any(|(d, _)| d == id))
            .collect();
        for id in to_cancel {
            ops.push(self.cancel(id, mode)?);
        }

        for (id, size) in desired {
            match self.size_of(*id) {
                Some(have) if have == *size => {}
                Some(_) => ops.push(self.resize(*id, *size)?),
                None => {}
            }
        }

        for (id, size) in desired {
            if self.size_of(*id).is_none() {
                ops.push(self.place(*id, *size)?);
            }
        }

        Ok(ops)
    }

    fn remove(&mut self, id: LimitOrderId) -> SdkResult<u64> {
        let idx = self
            .levels
            .iter()
            .position(|(i, _)| *i == id)
            .ok_or(ArcherSDKError::LimitOrderNotFound {
                side: id.side,
                price_ticks: id.price_ticks,
            })?;
        Ok(self.levels.remove(idx).1)
    }
}

/// Resolve a user-supplied [`NewLimitOrder`] (human price + size) into its
/// canonical `(LimitOrderId, size_lots)` form.
pub fn resolve_new_order(
    new: &NewLimitOrder,
    config: &MarketConfig,
) -> SdkResult<(LimitOrderId, u64)> {
    let price_ticks = price_to_ticks(new.price, config)?;
    let size_lots = base_amount_to_lots(new.size, config)?;
    Ok((LimitOrderId::new(new.side, price_ticks), size_lots))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onchain::LEVEL_OP_EXPECTED_ANY;

    fn id(side: Side, p: u64) -> LimitOrderId {
        LimitOrderId::new(side, p)
    }

    fn book(levels: &[(Side, u64, u64)]) -> LocalBook {
        LocalBook {
            levels: levels.iter().map(|(s, p, z)| (id(*s, *p), *z)).collect(),
        }
    }

    #[test]
    fn place_expects_an_empty_slot() {
        let mut b = LocalBook::empty();
        let op = b.place(id(Side::Bid, 99_900), 10).unwrap();
        assert_eq!(op, LevelOp::new(Side::Bid, 99_900, 0, 10));
        assert!(matches!(
            b.place(id(Side::Bid, 99_900), 5),
            Err(ArcherSDKError::LimitOrderAlreadyExists { .. })
        ));
    }

    #[test]
    fn cancel_mode_sets_the_guard() {
        let mut b = book(&[(Side::Ask, 100_100, 100)]);
        let any = b.clone().cancel(id(Side::Ask, 100_100), CancelMode::Any).unwrap();
        assert_eq!(any.expected_size, LEVEL_OP_EXPECTED_ANY);
        assert_eq!(any.new_size, 0);
        let strict = b.cancel(id(Side::Ask, 100_100), CancelMode::Strict).unwrap();
        assert_eq!(strict.expected_size, 100);
        assert!(b.size_of(id(Side::Ask, 100_100)).is_none());
        assert!(matches!(
            b.cancel(id(Side::Ask, 100_100), CancelMode::Any),
            Err(ArcherSDKError::LimitOrderNotFound { .. })
        ));
    }

    #[test]
    fn resize_is_always_strict() {
        let mut b = book(&[(Side::Ask, 100_100, 100)]);
        let op = b.resize(id(Side::Ask, 100_100), 70).unwrap();
        assert_eq!(op, LevelOp::new(Side::Ask, 100_100, 100, 70));
        // A second resize guards on the size the first one wrote.
        let op = b.resize(id(Side::Ask, 100_100), 0).unwrap();
        assert_eq!(op, LevelOp::new(Side::Ask, 100_100, 70, 0));
    }

    #[test]
    fn diff_emits_cancels_then_resizes_then_places() {
        let mut b = book(&[
            (Side::Bid, 99_900, 10),
            (Side::Bid, 99_800, 10),
            (Side::Ask, 100_100, 10),
        ]);
        let desired = [
            (id(Side::Bid, 99_900), 10), // unchanged
            (id(Side::Bid, 99_800), 4),  // shrink
            (id(Side::Ask, 100_200), 3), // new
        ];
        let ops = b.diff_to(&desired, CancelMode::Any).unwrap();
        assert_eq!(
            ops,
            vec![
                LevelOp::cancel_any(Side::Ask, 100_100),
                LevelOp::resize(Side::Bid, 99_800, 10, 4),
                LevelOp::place(Side::Ask, 100_200, 3),
            ]
        );
        let mut after: Vec<_> = b.orders().collect();
        after.sort_by_key(|(i, _)| (i.side as u8, i.price_ticks));
        assert_eq!(
            after,
            vec![(id(Side::Bid, 99_800), 4), (id(Side::Bid, 99_900), 10), (id(Side::Ask, 100_200), 3)]
        );
    }

    #[test]
    fn diff_rejects_duplicate_targets_and_zero_sizes() {
        let mut b = LocalBook::empty();
        assert!(matches!(
            b.diff_to(&[(id(Side::Bid, 1), 1), (id(Side::Bid, 1), 2)], CancelMode::Any),
            Err(ArcherSDKError::LimitOrderAlreadyExists { .. })
        ));
        assert!(matches!(
            b.diff_to(&[(id(Side::Bid, 1), 0)], CancelMode::Any),
            Err(ArcherSDKError::InvalidSize(_))
        ));
    }

    #[test]
    fn side_cap_and_self_cross_are_caught_locally() {
        let mut b = LocalBook::empty();
        for p in 1..=MAX_LEVELS as u64 {
            b.place(id(Side::Bid, p), 1).unwrap();
        }
        assert!(matches!(
            b.place(id(Side::Bid, 100), 1),
            Err(ArcherSDKError::BookFull { side: Side::Bid })
        ));
        b.place(id(Side::Ask, 16), 1).unwrap();
        assert!(b.check_not_crossed().is_err());
    }
}
