use crate::onchain::{ArcherUnit, BaseLots, MakerLevel, MAX_LEVELS};

use crate::{
    config::MarketConfig,
    error::{ArcherSDKError, SdkResult},
    math::{
        lots::base_amount_to_lots,
        ticks::{fair_price_to_mid_ticks, price_to_offset, price_to_ticks},
        BookUpdate, Quote, TwoSidedQuote,
    },
};

/// Convert a [`TwoSidedQuote`] into a [`BookUpdate`] ready for instruction building.
///
/// This is the primary entry point for market makers quoting explicit price levels.
///
/// # What It Does
///
/// 1. Validates the quote (ordering, no crossed book, level count).
/// 2. Computes optimal `mid_price_ticks` from the midpoint of best bid/ask.
///    Falls back to fair value from whichever side is present if one-sided.
/// 3. Converts each price to a tick offset from mid.
/// 4. Converts each size to base lots.
/// 5. Validates no duplicate offsets (prices too close for tick resolution).
/// 6. Computes estimated locked balances for margin checks.
///
/// # Arguments
///
/// - `quotes` — The two-sided quote with prices and sizes.
/// - `current_mid_price_ticks` — The maker's current on-chain mid price.
///   Used to determine if an `UpdateMidPrice` instruction is needed.
/// - `config` — Market configuration (from [`MarketConfig::from_header`]).
///
/// # Example
///
/// ```rust,ignore
/// let quotes = TwoSidedQuote::new()
///     .with_bid(148.40, 10.0)
///     .with_bid(148.30, 20.0)
///     .with_ask(148.60, 10.0)
///     .with_ask(148.70, 20.0);
///
/// let update = build_book_update(&quotes, current_mid, &config)?;
/// ```
pub fn build_book_update(
    quotes: &TwoSidedQuote,
    current_mid_price_ticks: u64,
    config: &MarketConfig,
) -> SdkResult<BookUpdate> {
    validate_quote_structure(quotes)?;

    let new_mid_price_ticks = compute_mid_price(quotes, config)?;
    let mid_price_changed = new_mid_price_ticks != current_mid_price_ticks;

    let mut bid_levels: Vec<MakerLevel> = Vec::with_capacity(quotes.bids.len());
    let mut estimated_quote_lots_locked: u64 = 0;

    for (i, quote) in quotes.bids.iter().enumerate() {
        let offset = price_to_offset(quote.price, new_mid_price_ticks, config)?;
        let size_lots = base_amount_to_lots(quote.size, config)?;

        if i > 0 {
            let prev_offset = bid_levels[i - 1].price_offset_ticks;
            if offset >= prev_offset {
                return Err(ArcherSDKError::DuplicateOffset {
                    offset,
                    a: i - 1,
                    b: i,
                });
            }
        }

        let abs_price_ticks = (new_mid_price_ticks as i128 + offset as i128).max(0) as u64;
        let quote_lots_for_level = config.quote_lots_ceil(size_lots, abs_price_ticks);
        estimated_quote_lots_locked =
            estimated_quote_lots_locked.saturating_add(quote_lots_for_level);

        bid_levels.push(MakerLevel {
            size_in_base_lots: BaseLots::new(size_lots),
            price_offset_ticks: offset,
        });
    }

    let mut ask_levels: Vec<MakerLevel> = Vec::with_capacity(quotes.asks.len());
    let mut estimated_base_lots_locked: u64 = 0;

    for (i, quote) in quotes.asks.iter().enumerate() {
        let offset = price_to_offset(quote.price, new_mid_price_ticks, config)?;
        let size_lots = base_amount_to_lots(quote.size, config)?;

        if i > 0 {
            let prev_offset = ask_levels[i - 1].price_offset_ticks;
            if offset <= prev_offset {
                return Err(ArcherSDKError::DuplicateOffset {
                    offset,
                    a: i - 1,
                    b: i,
                });
            }
        }

        estimated_base_lots_locked = estimated_base_lots_locked.saturating_add(size_lots);

        ask_levels.push(MakerLevel {
            size_in_base_lots: BaseLots::new(size_lots),
            price_offset_ticks: offset,
        });
    }

    Ok(BookUpdate {
        new_mid_price_ticks,
        bid_levels,
        ask_levels,
        mid_price_changed,
        estimated_base_lots_locked,
        estimated_quote_lots_locked,
    })
}

/// Build a symmetric book from a fair price and spread/size pairs.
///
/// This is the simplest way to quote. Provide a fair value price and
/// a list of `(spread_bps, size_in_base_tokens)` tuples. The SDK builds
/// symmetric bid and ask levels at each spread offset.
///
/// # Arguments
///
/// - `fair_price` — Fair value in quote tokens per base token.
/// - `levels` — Slice of `(spread_bps, size)` tuples. Each entry generates
///   one bid and one ask level. `spread_bps` is the distance from fair price
///   in basis points (1 bps = 0.01%).
/// - `config` — Market configuration.
///
/// # Example
///
/// ```rust,ignore
/// // 3 levels: 5bps/10 tokens, 10bps/25 tokens, 25bps/50 tokens
/// let update = build_book_from_spread(
///     148.50,
///     &[(5.0, 10.0), (10.0, 25.0), (25.0, 50.0)],
///     &config,
/// )?;
/// ```
///
/// This produces:
/// - Bids at 148.426, 148.352, 148.129 for 10, 25, 50 tokens
/// - Asks at 148.574, 148.649, 148.871 for 10, 25, 50 tokens
pub fn build_book_from_spread(
    fair_price: f64,
    levels: &[(f64, f64)], // (spread_bps, size_in_base_token)
    config: &MarketConfig,
) -> SdkResult<BookUpdate> {
    if levels.len() > MAX_LEVELS {
        return Err(ArcherSDKError::TooManyLevels {
            count: levels.len(),
        });
    }

    let mut quotes = TwoSidedQuote::new();

    for &(spread_bps, size) in levels {
        if spread_bps < 0.0 || !spread_bps.is_finite() {
            return Err(ArcherSDKError::InvalidPrice(spread_bps));
        }
        if size < 0.0 || !size.is_finite() {
            return Err(ArcherSDKError::InvalidSize(size));
        }

        let offset_fraction = spread_bps / 10_000.0;
        let bid_price = fair_price * (1.0 - offset_fraction);
        let ask_price = fair_price * (1.0 + offset_fraction);

        quotes.bids.push(Quote {
            price: bid_price,
            size,
        });
        quotes.asks.push(Quote {
            price: ask_price,
            size,
        });
    }

    // Use fair_price as the current mid for computing mid_price_changed.
    // Since this is a fresh quote, the mid will be the fair price itself.
    let mid_ticks = fair_price_to_mid_ticks(fair_price, config)?;
    build_book_update(&quotes, mid_ticks, config)
}

/// Build a one-sided book (bids only or asks only).
///
/// Useful for directional strategies or when hedging one side.
pub fn build_one_sided_book(
    side: OneSide,
    levels: &[Quote],
    current_mid_price_ticks: u64,
    config: &MarketConfig,
) -> SdkResult<BookUpdate> {
    let quotes = match side {
        OneSide::Bids => TwoSidedQuote {
            bids: levels.to_vec(),
            asks: Vec::new(),
        },
        OneSide::Asks => TwoSidedQuote {
            bids: Vec::new(),
            asks: levels.to_vec(),
        },
    };
    build_book_update(&quotes, current_mid_price_ticks, config)
}

/// Which side for a one-sided book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OneSide {
    Bids,
    Asks,
}

/// Validate quote structure before conversion.
fn validate_quote_structure(quotes: &TwoSidedQuote) -> SdkResult<()> {
    if quotes.bids.len() > MAX_LEVELS {
        return Err(ArcherSDKError::TooManyLevels {
            count: quotes.bids.len(),
        });
    }
    if quotes.asks.len() > MAX_LEVELS {
        return Err(ArcherSDKError::TooManyLevels {
            count: quotes.asks.len(),
        });
    }

    for i in 1..quotes.bids.len() {
        if quotes.bids[i].price >= quotes.bids[i - 1].price {
            return Err(ArcherSDKError::BidsNotDescending { index: i });
        }
    }

    for i in 1..quotes.asks.len() {
        if quotes.asks[i].price <= quotes.asks[i - 1].price {
            return Err(ArcherSDKError::AsksNotAscending { index: i });
        }
    }

    if let (Some(best_bid), Some(best_ask)) = (quotes.bids.first(), quotes.asks.first()) {
        if best_bid.price >= best_ask.price {
            return Err(ArcherSDKError::CrossedBook {
                bid: best_bid.price,
                ask: best_ask.price,
            });
        }
    }

    for q in quotes.bids.iter().chain(quotes.asks.iter()) {
        if !q.price.is_finite() || q.price <= 0.0 {
            return Err(ArcherSDKError::InvalidPrice(q.price));
        }
        if !q.size.is_finite() || q.size < 0.0 {
            return Err(ArcherSDKError::InvalidSize(q.size));
        }
    }

    Ok(())
}

/// Compute the optimal mid price from a two-sided quote.
///
/// Strategy:
/// - Two-sided: midpoint of best bid and best ask.
/// - Bid-only: best bid price (offset slightly above).
/// - Ask-only: best ask price (offset slightly below).
/// - Empty: error (can't determine mid from nothing).
fn compute_mid_price(quotes: &TwoSidedQuote, config: &MarketConfig) -> SdkResult<u64> {
    match (quotes.bids.first(), quotes.asks.first()) {
        (Some(best_bid), Some(best_ask)) => {
            let mid = (best_bid.price + best_ask.price) / 2.0;
            fair_price_to_mid_ticks(mid, config)
        }
        (Some(best_bid), None) => price_to_ticks(best_bid.price, config),
        (None, Some(best_ask)) => price_to_ticks(best_ask.price, config),
        (None, None) => Err(ArcherSDKError::InvalidPrice(0.0)),
    }
}
