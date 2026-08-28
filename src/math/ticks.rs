use crate::{
    config::MarketConfig,
    error::{ArcherSDKError, SdkResult},
};

/// Convert a human-readable price to ticks.
///
/// The price is in quote tokens per base token (e.g., 148.50 USDC per SOL).
/// Returns the nearest tick value, rounded to the nearest integer.
///
/// # Errors
///
/// - [`ArcherSDKError::InvalidPrice`] if price is negative, zero, NaN, or infinite.
/// - [`ArcherSDKError::PriceBelowResolution`] if the price rounds to zero ticks.
/// - [`ArcherSDKError::ArithmeticOverflow`] if the tick value exceeds `u64::MAX`.
pub fn price_to_ticks(price: f64, config: &MarketConfig) -> SdkResult<u64> {
    validate_price(price)?;

    let ticks_f64 = price * config.price_to_ticks_factor();

    if ticks_f64 < 0.5 {
        return Err(ArcherSDKError::PriceBelowResolution(price));
    }

    if ticks_f64 > u64::MAX as f64 {
        return Err(ArcherSDKError::ArithmeticOverflow {
            operation: "price_to_ticks",
        });
    }

    let ticks = ticks_f64.round() as u64;
    if ticks == 0 {
        return Err(ArcherSDKError::PriceBelowResolution(price));
    }

    Ok(ticks)
}

/// Convert a tick value to a human-readable price.
///
/// Returns the price in quote tokens per base token.
/// This is a lossless operation (no rounding).
#[inline]
pub fn ticks_to_price(ticks: u64, config: &MarketConfig) -> f64 {
    ticks as f64 * config.ticks_to_price_factor()
}

/// Convert a price to a tick offset from a given mid price.
///
/// Returns a signed offset: negative for prices below mid, positive for above.
///
/// # Errors
///
/// - [`ArcherSDKError::InvalidPrice`] if price is invalid.
/// - [`ArcherSDKError::PriceBelowResolution`] if price rounds to zero ticks.
/// - [`ArcherSDKError::OffsetOverflow`] if the offset doesn't fit in `i64`.
pub fn price_to_offset(price: f64, mid_price_ticks: u64, config: &MarketConfig) -> SdkResult<i64> {
    let price_ticks = price_to_ticks(price, config)?;
    let offset = (price_ticks as i128) - (mid_price_ticks as i128);

    i64::try_from(offset).map_err(|_| ArcherSDKError::OffsetOverflow {
        price,
        mid: ticks_to_price(mid_price_ticks, config),
    })
}

/// Convert a tick offset and mid price back to a human-readable price.
#[inline]
pub fn offset_to_price(offset: i64, mid_price_ticks: u64, config: &MarketConfig) -> f64 {
    let absolute_ticks = (mid_price_ticks as i128 + offset as i128).max(0) as u64;
    ticks_to_price(absolute_ticks, config)
}

/// Compute the optimal mid price in ticks from a fair value price.
///
/// The mid price is the reference point for all level offsets.
/// Typically set to the midpoint of best bid and best ask,
/// or directly from an external fair value.
///
/// Rounds to the nearest tick.
pub fn fair_price_to_mid_ticks(fair_price: f64, config: &MarketConfig) -> SdkResult<u64> {
    price_to_ticks(fair_price, config)
}

/// Compute the mid price from the best bid and best ask.
///
/// mid = (best_bid + best_ask) / 2, converted to ticks.
/// This is the most common way to set mid_price_ticks.
pub fn mid_from_bbo(best_bid: f64, best_ask: f64, config: &MarketConfig) -> SdkResult<u64> {
    validate_price(best_bid)?;
    validate_price(best_ask)?;

    if best_bid >= best_ask {
        return Err(ArcherSDKError::CrossedBook {
            bid: best_bid,
            ask: best_ask,
        });
    }

    let mid = (best_bid + best_ask) / 2.0;
    price_to_ticks(mid, config)
}

/// Compute the tick difference between two prices.
///
/// Useful for measuring spread in tick units.
pub fn price_spread_in_ticks(
    bid_price: f64,
    ask_price: f64,
    config: &MarketConfig,
) -> SdkResult<u64> {
    let bid_ticks = price_to_ticks(bid_price, config)?;
    let ask_ticks = price_to_ticks(ask_price, config)?;

    Ok(ask_ticks.saturating_sub(bid_ticks))
}

/// Compute the number of ticks corresponding to a basis point offset from a price.
///
/// E.g., 5 bps from 148.50 → how many ticks is that?
pub fn bps_to_ticks(price: f64, bps: f64, config: &MarketConfig) -> SdkResult<u64> {
    let offset_price = price * bps / 10_000.0;
    let offset_ticks = (offset_price * config.price_to_ticks_factor())
        .round()
        .abs() as u64;
    Ok(offset_ticks.max(1)) // at least 1 tick for any non-zero bps
}

fn validate_price(price: f64) -> SdkResult<()> {
    if !price.is_finite() || price <= 0.0 {
        return Err(ArcherSDKError::InvalidPrice(price));
    }
    Ok(())
}
