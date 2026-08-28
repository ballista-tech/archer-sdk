use crate::onchain::{ArcherUnit, MakerBook, Side};
use solana_program::pubkey::Pubkey;

use crate::config::MarketConfig;
use crate::math::lots::base_lots_to_amount;
use crate::math::ticks::offset_to_price;

/// Stable client-side identifier for a limit order.
///
/// Derived from the order's side and its absolute price in ticks.
/// Changes if this order is repriced — repricing is semantically
/// cancel + place at the new price.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitOrderId {
    pub side: Side,
    pub price_ticks: u64,
}

impl LimitOrderId {
    #[inline]
    pub const fn new(side: Side, price_ticks: u64) -> Self {
        Self { side, price_ticks }
    }
}

/// A single limit order, in human-readable form.
#[derive(Debug, Clone, Copy)]
pub struct LimitOrder {
    pub id: LimitOrderId,
    pub price: f64,
    pub size: f64,
    pub size_lots: u64,
}

/// Input to the `place` family of actions.
#[derive(Debug, Clone, Copy)]
pub struct NewLimitOrder {
    pub side: Side,
    /// Price in quote per base, in human units (e.g. 148.50).
    pub price: f64,
    /// Size in base tokens (e.g. 10.5 SOL).
    pub size: f64,
}

impl NewLimitOrder {
    #[inline]
    pub const fn bid(price: f64, size: f64) -> Self {
        Self {
            side: Side::Bid,
            price,
            size,
        }
    }

    #[inline]
    pub const fn ask(price: f64, size: f64) -> Self {
        Self {
            side: Side::Ask,
            price,
            size,
        }
    }
}

/// A single fillable rung discovered on the market — one active level of one
/// MakerBook, projected to human-readable price/size.
#[derive(Debug, Clone, Copy)]
pub struct LimitOrderRung {
    pub maker_book: Pubkey,
    pub owner: Pubkey,
    pub side: Side,
    pub price: f64,
    pub size: f64,
    pub size_lots: u64,
}

/// Read-only projection of a MakerBook into limit-order terms.
#[derive(Debug, Clone)]
pub struct LimitOrderBookView {
    pub maker_book: Pubkey,
    pub owner: Pubkey,
    /// Immutable anchor mid that all offsets are measured against.
    pub anchor_mid_price_ticks: u64,
    pub last_updated_sequence_number: u64,
    pub orders: Vec<LimitOrder>,
}

impl LimitOrderBookView {
    /// Project a fetched `MakerBook` into a flat list of human-readable orders.
    pub fn from_maker_book(
        maker_book_pubkey: Pubkey,
        book: &MakerBook,
        config: &MarketConfig,
    ) -> Self {
        let mut orders = Vec::with_capacity(32);
        let anchor = book.mid_price_ticks;

        for level in &book.bid_levels {
            if level.size_in_base_lots.as_u64() == 0 {
                continue;
            }
            let size_lots = level.size_in_base_lots.as_u64();
            let abs_ticks = (anchor as i128 + level.price_offset_ticks as i128).max(0) as u64;
            orders.push(LimitOrder {
                id: LimitOrderId::new(Side::Bid, abs_ticks),
                price: offset_to_price(level.price_offset_ticks, anchor, config),
                size: base_lots_to_amount(size_lots, config),
                size_lots,
            });
        }
        for level in &book.ask_levels {
            if level.size_in_base_lots.as_u64() == 0 {
                continue;
            }
            let size_lots = level.size_in_base_lots.as_u64();
            let abs_ticks = (anchor as i128 + level.price_offset_ticks as i128).max(0) as u64;
            orders.push(LimitOrder {
                id: LimitOrderId::new(Side::Ask, abs_ticks),
                price: offset_to_price(level.price_offset_ticks, anchor, config),
                size: base_lots_to_amount(size_lots, config),
                size_lots,
            });
        }

        Self {
            maker_book: maker_book_pubkey,
            owner: book.maker,
            anchor_mid_price_ticks: anchor,
            last_updated_sequence_number: book.last_updated_sequence_number,
            orders,
        }
    }

    /// Find an order by its ID.
    pub fn find(&self, id: LimitOrderId) -> Option<&LimitOrder> {
        self.orders.iter().find(|o| o.id == id)
    }

    /// Iterator over bid-side orders only.
    pub fn bids(&self) -> impl Iterator<Item = &LimitOrder> {
        self.orders.iter().filter(|o| o.id.side == Side::Bid)
    }

    /// Iterator over ask-side orders only.
    pub fn asks(&self) -> impl Iterator<Item = &LimitOrder> {
        self.orders.iter().filter(|o| o.id.side == Side::Ask)
    }
}
