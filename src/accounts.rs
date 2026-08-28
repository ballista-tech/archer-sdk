use crate::onchain::{
    ArcherUnit, MakerBook, MakerRegistry, MarketState, MarketStateHeader,
};

use crate::{
    config::MarketConfig,
    error::{ArcherSDKError, SdkResult},
    math::lots::{base_lots_to_amount, quote_lots_to_amount},
};

/// Maker's current balance state in human-readable token amounts
#[derive(Debug, Clone, Copy)]
pub struct MakerBalances {
    /// Base tokens available for new asks or withdrawal
    pub base_free: f64,
    /// Base tokens reserved for current ask levels
    pub base_locked: f64,
    /// Quote tokens available for new bids or withdrawal
    pub quote_free: f64,
    /// Quote tokens reserved for current bid levels
    pub quote_locked: f64,
    /// Total base (locked + free)
    pub base_total: f64,
    /// Total quote (locked + free)
    pub quote_total: f64,
    /// `true` when the book has a pending `update_mid_price` rebalance the maker
    /// cannot fund. The quote figures above then fall back to the raw account
    /// values, and the aggregator will skip this book until the maker deposits or
    /// reprices back within their balance.
    pub quote_sync_unfundable: bool,
}

/// Deserialize a `MarketStateHeader` from raw account data.
///
/// Delegates to the type's own loader — the cast and discriminator check exist
/// once, here we only translate the error into the SDK's type.
pub fn parse_market_state(data: &[u8]) -> SdkResult<&MarketStateHeader> {
    MarketState::load_header(data).map_err(|_| ArcherSDKError::InvalidDiscriminator {
        expected: "ACHRMKT1",
    })
}

/// Deserialize a `MakerBook` from raw account data.
pub fn parse_maker_book(data: &[u8]) -> SdkResult<&MakerBook> {
    MakerBook::load(data).map_err(|_| ArcherSDKError::InvalidDiscriminator {
        expected: "ACHRMKR1",
    })
}

/// Deserialize a `MakerRegistry` from raw account data.
pub fn parse_maker_registry(data: &[u8]) -> SdkResult<&MakerRegistry> {
    MakerRegistry::load(data).map_err(|_| ArcherSDKError::InvalidDiscriminator {
        expected: "ACHRREG1",
    })
}

/// Extract human-readable balances from a maker book.
pub fn maker_balances(book: &MakerBook, config: &MarketConfig) -> MakerBalances {
    let base_free = base_lots_to_amount(book.base_free.as_u64(), config);
    let base_locked = base_lots_to_amount(book.base_locked.as_u64(), config);

    let projected = book.projected_quote_balances();
    let quote_sync_unfundable = projected.is_err();
    let (quote_locked_lots, quote_free_lots) =
        projected.unwrap_or((book.quote_locked.as_u64(), book.quote_free.as_u64()));

    let quote_free = quote_lots_to_amount(quote_free_lots, config);
    let quote_locked = quote_lots_to_amount(quote_locked_lots, config);

    MakerBalances {
        base_free,
        base_locked,
        quote_free,
        quote_locked,
        base_total: base_free + base_locked,
        quote_total: quote_free + quote_locked,
        quote_sync_unfundable,
    }
}

/// Count the number of active (non-zero size) bid levels in a maker book.
pub fn active_bid_levels(book: &MakerBook) -> usize {
    book.bid_levels
        .iter()
        .take_while(|l| l.size_in_base_lots.as_u64() > 0)
        .count()
}

/// Count the number of active (non-zero size) ask levels in a maker book.
pub fn active_ask_levels(book: &MakerBook) -> usize {
    book.ask_levels
        .iter()
        .take_while(|l| l.size_in_base_lots.as_u64() > 0)
        .count()
}

/// Get the best bid price from a maker book in human-readable terms.
///
/// Returns `None` if the bid side is empty.
pub fn best_bid_price(book: &MakerBook, config: &MarketConfig) -> Option<f64> {
    if book.bid_levels[0].size_in_base_lots.as_u64() == 0 {
        return None;
    }
    let abs_ticks =
        (book.mid_price_ticks as i128 + book.bid_levels[0].price_offset_ticks as i128).max(0);
    Some(abs_ticks as f64 * config.ticks_to_price_factor())
}

/// Get the best ask price from a maker book in human-readable terms.
pub fn best_ask_price(book: &MakerBook, config: &MarketConfig) -> Option<f64> {
    if book.ask_levels[0].size_in_base_lots.as_u64() == 0 {
        return None;
    }
    let abs_ticks =
        (book.mid_price_ticks as i128 + book.ask_levels[0].price_offset_ticks as i128).max(0);
    Some(abs_ticks as f64 * config.ticks_to_price_factor())
}

/// Get the spread in basis points between best bid and best ask.
pub fn spread_bps(book: &MakerBook, config: &MarketConfig) -> Option<f64> {
    let bid = best_bid_price(book, config)?;
    let ask = best_ask_price(book, config)?;
    let mid = (bid + ask) / 2.0;
    if mid == 0.0 {
        return None;
    }
    Some((ask - bid) / mid * 10_000.0)
}
