use crate::{
    config::MarketConfig,
    error::SdkResult,
    math::{lots::base_amount_to_lots, ticks::price_to_ticks, Quote},
};

/// Estimate maker fee/rebate for a fill, in quote token units.
///
/// Positive = fee charged to maker. Negative = rebate paid to maker.
///
/// # Arguments
///
/// - `fill_base_amount` — Base tokens filled (e.g., 10.0 SOL).
/// - `fill_price` — Execution price in quote per base.
pub fn estimate_maker_fee(fill_base_amount: f64, fill_price: f64, config: &MarketConfig) -> f64 {
    let quote_amount = fill_base_amount * fill_price;
    quote_amount * (config.maker_fee_ppm as f64) / 1_000_000.0
}

/// What a taker pays on top of the notional, in quote token units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TakerFees {
    /// The market's protocol fee. Negative on a market that rebates takers.
    pub protocol: f64,

    /// The builder fee, if the swap carries a builder code.
    pub builder: f64,
}

/// Estimate what a swap costs the taker beyond the notional.
///
/// `builder_fee_ppm` must match the value going into the swap's
/// [`SwapParams`](archer_v1::swap_types::SwapParams); pass `0` when the swap
/// carries no builder code.
pub fn estimate_taker_fees(
    trade_quote_amount: f64,
    builder_fee_ppm: u32,
    config: &MarketConfig,
) -> TakerFees {
    let protocol = trade_quote_amount * (config.taker_fee_ppm as f64) / 1_000_000.0;
    let builder = trade_quote_amount * (builder_fee_ppm as f64) / 1_000_000.0;

    TakerFees {
        protocol,
        builder,
    }
}

/// Estimate the per-unit price a taker actually gets, after all fees.
///
/// Buying base: higher than `price` (the fees are paid on top). Selling base:
/// lower (they come out of the proceeds).
pub fn effective_taker_price(
    price: f64,
    is_buying_base: bool,
    builder_fee_ppm: u32,
    config: &MarketConfig,
) -> f64 {
    let fee_multiplier = (config.taker_fee_ppm as f64 + builder_fee_ppm as f64) / 1_000_000.0;
    if is_buying_base {
        price * (1.0 + fee_multiplier)
    } else {
        price * (1.0 - fee_multiplier)
    }
}

/// Estimate quote tokens needed to support a set of bid levels.
///
/// This calculates how much quote token (e.g., USDC) must be deposited
/// as free balance to post these bids. The on-chain program will lock
/// this amount when the book is updated.
///
/// Returns the estimated required amount in quote token units.
///
/// # Note
///
/// This is a conservative estimate. The on-chain program uses exact
/// lot-level math which may differ slightly due to rounding. Always
/// deposit a small buffer above this estimate.
pub fn estimate_required_quote_margin(bids: &[Quote], config: &MarketConfig) -> SdkResult<f64> {
    let mut total_quote_lots: u64 = 0;

    // Only add fee buffer when maker pays a fee (positive PPM).
    // When maker_fee_ppm is negative (rebate), ignore it —
    // being conservative means not reducing the margin estimate.
    let fee_multiplier_ppm: u64 = config.maker_fee_ppm.max(0) as u64;

    for bid in bids {
        let base_lots = base_amount_to_lots(bid.size, config)?;
        let price_ticks = price_to_ticks(bid.price, config)?;

        let raw_quote_lots = config.quote_lots_ceil(base_lots, price_ticks);

        // fee_lots = raw_quote_lots * maker_fee_ppm / 1_000_000 (ceiling)
        let fee_lots = raw_quote_lots
            .saturating_mul(fee_multiplier_ppm)
            .saturating_add(999_999) // ceiling division
            / 1_000_000;

        total_quote_lots = total_quote_lots
            .saturating_add(raw_quote_lots)
            .saturating_add(fee_lots);
    }

    Ok(total_quote_lots as f64 * config.lots_to_quote_factor())
}

/// Estimate base tokens needed to support a set of ask levels.
///
/// This calculates how much base token (e.g., SOL) must be deposited
/// as free balance to post these asks.
pub fn estimate_required_base_margin(asks: &[Quote], config: &MarketConfig) -> SdkResult<f64> {
    let mut total_base_lots: u64 = 0;

    for ask in asks {
        let base_lots = base_amount_to_lots(ask.size, config)?;
        total_base_lots = total_base_lots.saturating_add(base_lots);
    }

    Ok(total_base_lots as f64 * config.lots_to_base_factor())
}

/// Estimate total margin requirements for a two-sided quote.
///
/// Returns `(required_base, required_quote)` in token units.
pub fn estimate_total_margin(
    bids: &[Quote],
    asks: &[Quote],
    config: &MarketConfig,
) -> SdkResult<(f64, f64)> {
    let quote_margin = estimate_required_quote_margin(bids, config)?;
    let base_margin = estimate_required_base_margin(asks, config)?;
    Ok((base_margin, quote_margin))
}

/// Check if a maker has sufficient deposited balance for their quotes.
///
/// Returns `Ok(())` if sufficient, or an error describing the shortfall.
pub fn check_margin_sufficiency(
    bids: &[Quote],
    asks: &[Quote],
    base_free: f64,
    quote_free: f64,
    config: &MarketConfig,
) -> SdkResult<()> {
    let (base_needed, quote_needed) = estimate_total_margin(bids, asks, config)?;

    if base_free < base_needed {
        return Err(crate::error::ArcherSDKError::InsufficientBalance {
            required: base_needed,
            available: base_free,
            token: "base".to_string(),
        });
    }

    if quote_free < quote_needed {
        return Err(crate::error::ArcherSDKError::InsufficientBalance {
            required: quote_needed,
            available: quote_free,
            token: "quote".to_string(),
        });
    }

    Ok(())
}

/// Calculate the break-even spread in basis points for a round-trip trade.
///
/// A round-trip = taker buys (pays taker_fee) + maker gets filled (pays maker_fee).
/// Break-even spread = taker_fee + maker_fee (both in bps).
/// If maker_fee is negative (rebate), the break-even is lower.
pub fn break_even_spread_bps(config: &MarketConfig) -> f64 {
    let taker_bps = config.taker_fee_ppm as f64 / 100.0; // ppm → bps
    let maker_bps = config.maker_fee_ppm as f64 / 100.0;
    taker_bps + maker_bps
}
