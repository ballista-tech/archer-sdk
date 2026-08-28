//! Filler-side helpers for discovering limit orders on a market.
//!
//! Searchers, arbers, and aggregators use these to build the maker-book
//! list passed into [`crate::ix_builder::swap`]. The output is a flat,
//! price-sorted ladder of rungs across every MakerBook on the market —
//! whether or not the owner thinks of themselves as a "limit-order user."

use crate::onchain::{ArcherUnit, MakerBook, Side};
use solana_program::pubkey::Pubkey;

use crate::config::MarketConfig;
use crate::math::lots::base_lots_to_amount;
use crate::math::ticks::offset_to_price;

use super::types::LimitOrderRung;

/// Flatten every active level on every supplied MakerBook into a sorted ladder.
///
/// Bids sorted descending by price (best first); asks sorted ascending by
/// price (best first). The two halves are concatenated: `[bids..., asks...]`.
pub fn build_ladder(books: &[(Pubkey, MakerBook)], config: &MarketConfig) -> Vec<LimitOrderRung> {
    let mut bids = Vec::new();
    let mut asks = Vec::new();

    for (pk, book) in books {
        let anchor = book.mid_price_ticks;
        if anchor == 0 {
            continue;
        }

        for level in &book.bid_levels {
            let size_lots = level.size_in_base_lots.as_u64();
            if size_lots == 0 {
                continue;
            }
            bids.push(LimitOrderRung {
                maker_book: *pk,
                owner: book.maker,
                side: Side::Bid,
                price: offset_to_price(level.price_offset_ticks, anchor, config),
                size: base_lots_to_amount(size_lots, config),
                size_lots,
            });
        }
        for level in &book.ask_levels {
            let size_lots = level.size_in_base_lots.as_u64();
            if size_lots == 0 {
                continue;
            }
            asks.push(LimitOrderRung {
                maker_book: *pk,
                owner: book.maker,
                side: Side::Ask,
                price: offset_to_price(level.price_offset_ticks, anchor, config),
                size: base_lots_to_amount(size_lots, config),
                size_lots,
            });
        }
    }

    bids.sort_by(|a, b| {
        b.price
            .partial_cmp(&a.price)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    asks.sort_by(|a, b| {
        a.price
            .partial_cmp(&b.price)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut out = Vec::with_capacity(bids.len() + asks.len());
    out.extend(bids);
    out.extend(asks);
    out
}

/// One-sided ladder (best-first).
pub fn build_ladder_for_side(
    books: &[(Pubkey, MakerBook)],
    side: Side,
    config: &MarketConfig,
) -> Vec<LimitOrderRung> {
    build_ladder(books, config)
        .into_iter()
        .filter(|r| r.side == side)
        .collect()
}

/// De-duplicate `maker_book` pubkeys from an ordered rung slice, preserving
/// the first occurrence's order. Useful for converting a ladder into a list
/// of books to pass to a `Swap` instruction without exceeding the 64-book cap.
///
/// `max_books` of 0 returns an empty Vec.
pub fn unique_books_in_order(rungs: &[LimitOrderRung], max_books: usize) -> Vec<Pubkey> {
    if max_books == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(max_books.min(rungs.len()));
    for rung in rungs {
        if out.contains(&rung.maker_book) {
            continue;
        }
        out.push(rung.maker_book);
        if out.len() >= max_books {
            break;
        }
    }
    out
}

/// Drop maker books that are suspended (`MakerBookStatus::Suspended`). These
/// are also skipped by the matching engine; pruning them client-side avoids
/// wasting an account slot in the Swap accounts list.
pub fn filter_active(books: Vec<(Pubkey, MakerBook)>) -> Vec<(Pubkey, MakerBook)> {
    use crate::onchain::MakerBookStatus;
    books
        .into_iter()
        .filter(|(_, b)| {
            b.get_status()
                .map(|s| matches!(s, MakerBookStatus::Active))
                .unwrap_or(false)
        })
        .collect()
}
