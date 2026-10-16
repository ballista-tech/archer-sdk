use crate::onchain::{
    ArcherUnit, MakerBook, MakerLevel, MakerRegistry, MarketState, MarketStateHeader,
    MAX_SEQUENCE_JUMP,
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

    let projected = book.projected_quote_balances(config.maker_fee_ppm);
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

/// The lots a withdrawal may take right now: `base_free`, and the quote free
/// balance after any pending mid-price rebalance is applied, which is the cap
/// the program enforces. Falls back to the raw `quote_free` when the rebalance
/// is unfundable, as [`maker_balances`] does.
pub fn withdrawable_lots(book: &MakerBook, maker_fee_ppm: i32) -> (u64, u64) {
    let quote_free = book
        .projected_quote_balances(maker_fee_ppm)
        .map(|(_, free)| free)
        .unwrap_or(book.quote_free.as_u64());
    (book.base_free.as_u64(), quote_free)
}

/// Count the active (non-zero size) bid levels in a maker book.
pub fn active_bid_levels(book: &MakerBook) -> usize {
    book.bid_levels.iter().filter(|l| l.is_active()).count()
}

/// Count the active (non-zero size) ask levels in a maker book.
pub fn active_ask_levels(book: &MakerBook) -> usize {
    book.ask_levels.iter().filter(|l| l.is_active()).count()
}

/// The best bid price of a maker book in human-readable terms: its first
/// active level, as the matching engine reads it.
///
/// `None` if the side is empty or that level's absolute price is not positive.
pub fn best_bid_price(book: &MakerBook, config: &MarketConfig) -> Option<f64> {
    first_active_price(&book.bid_levels, book.mid_price_ticks, config)
}

/// The best ask price of a maker book in human-readable terms. See
/// [`best_bid_price`].
pub fn best_ask_price(book: &MakerBook, config: &MarketConfig) -> Option<f64> {
    first_active_price(&book.ask_levels, book.mid_price_ticks, config)
}

fn first_active_price(levels: &[MakerLevel], anchor: u64, config: &MarketConfig) -> Option<f64> {
    let level = levels.iter().find(|l| l.is_active())?;
    let ticks = level.absolute_price(anchor)?;
    Some(ticks as f64 * config.ticks_to_price_factor())
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

/// The sequence number the book's next update must carry.
///
/// Every book-writing instruction (`UpdateBook`, `UpdateMidPrice`, `ClearBook`)
/// carries a sequence number the program validates against the book's current
/// one, and the accepted window is bounded on both sides:
pub fn next_sequence_number(book: &MakerBook) -> u64 {
    book.last_updated_sequence_number.saturating_add(1)
}

/// Check a sequence number against the book before building an instruction.
///
/// Callers that take their sequence numbers from [`next_sequence_number`] — or
/// from the `next_sequence_number` field the limit-order actions return — never
/// need this; it is for callers keeping their own counter.
pub fn check_sequence_number(book: &MakerBook, proposed: u64) -> SdkResult<()> {
    let current = book.last_updated_sequence_number;

    if proposed <= current {
        return Err(ArcherSDKError::StaleSequenceNumber { current, proposed });
    }

    if proposed - current > MAX_SEQUENCE_JUMP {
        return Err(ArcherSDKError::SequenceNumberTooFarAhead {
            current,
            proposed,
            max: MAX_SEQUENCE_JUMP,
        });
    }

    Ok(())
}
