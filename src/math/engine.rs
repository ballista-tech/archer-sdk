//! Matching-engine arithmetic.
//!
//! Everything else in [`crate::math`] takes `f64` at the edges and is meant for
//! quoting. These functions are for callers that
//! need to know *exactly* what a swap will settle at: fill simulators,
//! aggregator adapters, solvency checks.
//!
//! They return the same [`ArcherError`] the program would, so an overflow here
//! means the instruction would fail on-chain for the same input.

use crate::onchain::{
    ArcherError, ArcherUnit, BaseLots, MarketStateHeader, Side, Ticks, PPM_DIVISOR,
};

/// Quote lots the taker pays (bid) or receives (ask) for `base_lots` at
/// `price_ticks`.
///
/// The bid side rounds **up** (the taker pays the ceiling), the ask side rounds
/// **down** (the taker receives the floor). Mirrors `aggregator::base_to_quote_lots`.
pub fn base_to_quote_lots(
    market: &MarketStateHeader,
    base_lots: u64,
    price_ticks: u64,
    side: Side,
) -> Result<u64, ArcherError> {
    let quote_atoms = market
        .base_lots_to_quote_atoms(BaseLots::new(base_lots), Ticks::new(price_ticks))
        .map_err(|_| ArcherError::QuoteOverflow)?;

    let quote_atoms_u128 = quote_atoms.as_u128();
    let quote_atoms_per_lot = market.quote_atoms_per_quote_lot.as_u128();

    if quote_atoms_per_lot == 0 {
        return Err(ArcherError::QuoteOverflow);
    }

    let quote_lots = match side {
        Side::Bid => {
            let adjustment = quote_atoms_per_lot
                .checked_sub(1)
                .ok_or(ArcherError::QuoteOverflow)?;
            quote_atoms_u128
                .checked_add(adjustment)
                .ok_or(ArcherError::QuoteOverflow)?
                .checked_div(quote_atoms_per_lot)
                .ok_or(ArcherError::QuoteOverflow)?
        }
        Side::Ask => quote_atoms_u128
            .checked_div(quote_atoms_per_lot)
            .ok_or(ArcherError::QuoteOverflow)?,
    };

    if quote_lots > u64::MAX as u128 {
        return Err(ArcherError::QuoteOverflow);
    }

    Ok(quote_lots as u64)
}

/// Base lots that `quote_lots` buys at `price_ticks`; `round_up` selects
/// ceiling division. Mirrors `aggregator::quote_to_base_lots`.
pub fn quote_to_base_lots(
    market: &MarketStateHeader,
    quote_lots: u64,
    price_ticks: u64,
    round_up: bool,
) -> Result<u64, ArcherError> {
    let base_atoms_per_base_unit = market.base_atoms_per_base_unit()?;

    let quote_atoms = (quote_lots as u128)
        .checked_mul(market.quote_atoms_per_quote_lot.as_u128())
        .ok_or(ArcherError::QuoteOverflow)?;

    let numerator = quote_atoms
        .checked_mul(base_atoms_per_base_unit)
        .ok_or(ArcherError::QuoteOverflow)?;

    let tick_size = market.tick_size_in_quote_atoms_per_base_unit.as_u128();
    let base_atoms_per_lot = market.base_atoms_per_base_lot.as_u128();

    let denominator = (price_ticks as u128)
        .checked_mul(tick_size)
        .ok_or(ArcherError::PriceOverflow)?
        .checked_mul(base_atoms_per_lot)
        .ok_or(ArcherError::PriceOverflow)?;

    if denominator == 0 {
        return Err(ArcherError::PriceOverflow);
    }

    let base_lots = if round_up {
        let adjustment = denominator
            .checked_sub(1)
            .ok_or(ArcherError::QuoteOverflow)?;
        numerator
            .checked_add(adjustment)
            .ok_or(ArcherError::QuoteOverflow)?
            .checked_div(denominator)
            .ok_or(ArcherError::QuoteOverflow)?
    } else {
        numerator
            .checked_div(denominator)
            .ok_or(ArcherError::QuoteOverflow)?
    };

    if base_lots > u64::MAX as u128 {
        return Err(ArcherError::QuoteOverflow);
    }

    Ok(base_lots as u64)
}

/// Signed fee in quote lots for a fill of `quote_lots` at `fee_ppm`.
///
/// A positive fee rounds **up** (the payer covers the dust); a negative fee — a
/// rebate — truncates toward zero. Mirrors `aggregator::calculate_fee`.
pub fn calculate_fee(quote_lots: u64, fee_ppm: i32) -> Result<i64, ArcherError> {
    let quote = quote_lots as i128;
    let fee_rate = fee_ppm as i128;
    let divisor = PPM_DIVISOR as i128;

    let fee_raw = quote
        .checked_mul(fee_rate)
        .ok_or(ArcherError::FeeOverflow)?;

    let fee = if fee_raw > 0 {
        fee_raw
            .checked_add(divisor.checked_sub(1).ok_or(ArcherError::FeeOverflow)?)
            .ok_or(ArcherError::FeeOverflow)?
            .checked_div(divisor)
            .ok_or(ArcherError::FeeOverflow)?
    } else if fee_raw < 0 {
        fee_raw
            .checked_div(divisor)
            .ok_or(ArcherError::FeeOverflow)?
    } else {
        0
    };

    i64::try_from(fee).map_err(|_| ArcherError::FeeOverflow)
}

/// `total · share / total_shares`, floored. This is how a level's fill is split
/// across makers resting at the same price. Mirrors `aggregator::calculate_pro_rata`.
pub fn calculate_pro_rata(total: u64, share: u64, total_shares: u64) -> Result<u64, ArcherError> {
    if total_shares == 0 {
        return Err(ArcherError::ArithmeticOverflow);
    }

    let numerator = (total as u128)
        .checked_mul(share as u128)
        .ok_or(ArcherError::ArithmeticOverflow)?;

    let result = numerator
        .checked_div(total_shares as u128)
        .ok_or(ArcherError::ArithmeticOverflow)?;

    if result > u64::MAX as u128 {
        return Err(ArcherError::ArithmeticOverflow);
    }

    Ok(result as u64)
}

/// The quote budget the engine actually matches against for a
/// `MaxAmountIn` + `Bid` swap of `amount_quote_lots`.
///
/// "Spend exactly X quote" has to leave room for the taker fee and any builder
/// fee, both charged on top of the notional, so the program scales the budget
/// down by `PPM / (PPM + taker_fee_ppm + builder_fee_ppm)` before matching. A
/// negative taker fee contributes nothing (rebates are not pre-applied). With
/// no add-on fee the budget is unchanged. Mirrors `swap.rs`.
pub fn max_amount_in_bid_budget(
    amount_quote_lots: u64,
    taker_fee_ppm: i32,
    builder_fee_ppm: u32,
) -> Result<u64, ArcherError> {
    let total_add_on_ppm = (taker_fee_ppm.max(0) as u128)
        .checked_add(builder_fee_ppm as u128)
        .ok_or(ArcherError::ArithmeticOverflow)?;

    if total_add_on_ppm == 0 {
        return Ok(amount_quote_lots);
    }

    let ppm = PPM_DIVISOR as u128;
    let denominator = ppm
        .checked_add(total_add_on_ppm)
        .ok_or(ArcherError::ArithmeticOverflow)?;
    let adjusted = (amount_quote_lots as u128)
        .checked_mul(ppm)
        .ok_or(ArcherError::ArithmeticOverflow)?
        .checked_div(denominator)
        .ok_or(ArcherError::ArithmeticOverflow)?;

    Ok(adjusted as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onchain::{BaseAtomsPerLot, QuoteAtomsPerBaseUnitPerTick, QuoteAtomsPerLot};
    use bytemuck::Zeroable;

    /// SOL/USDC-shaped market: 9/6 decimals, 0.001 SOL base lots, 1-atom quote
    /// lots, 10-atom ticks, R = 1. 150 USDC/SOL is 15_000_000 ticks.
    fn sol_usdc(quote_atoms_per_quote_lot: u64) -> MarketStateHeader {
        let mut m = MarketStateHeader::zeroed();
        m.base_decimals = 9;
        m.quote_decimals = 6;
        m.raw_base_units_per_base_unit = 1;
        m.base_atoms_per_base_lot = BaseAtomsPerLot::new(1_000_000);
        m.quote_atoms_per_quote_lot = QuoteAtomsPerLot::new(quote_atoms_per_quote_lot);
        m.tick_size_in_quote_atoms_per_base_unit = QuoteAtomsPerBaseUnitPerTick::new(10);
        m
    }

    const PRICE_150: u64 = 15_000_000;

    #[test]
    fn base_to_quote_is_exact_when_lots_divide() {
        // 0.001 SOL at 150 USDC/SOL = 0.15 USDC = 150_000 atoms = 150_000 lots.
        let m = sol_usdc(1);
        assert_eq!(
            base_to_quote_lots(&m, 1, PRICE_150, Side::Bid).unwrap(),
            150_000
        );
        assert_eq!(
            base_to_quote_lots(&m, 1, PRICE_150, Side::Ask).unwrap(),
            150_000
        );
    }

    #[test]
    fn base_to_quote_rounds_against_the_taker() {
        // 150_000 atoms / 7 atoms per lot = 21428.57…
        let m = sol_usdc(7);
        assert_eq!(
            base_to_quote_lots(&m, 1, PRICE_150, Side::Bid).unwrap(),
            21_429
        );
        assert_eq!(
            base_to_quote_lots(&m, 1, PRICE_150, Side::Ask).unwrap(),
            21_428
        );
    }

    #[test]
    fn quote_to_base_floor_and_ceil() {
        let m = sol_usdc(1);
        // Exactly one base lot's worth of quote.
        assert_eq!(
            quote_to_base_lots(&m, 150_000, PRICE_150, false).unwrap(),
            1
        );
        assert_eq!(quote_to_base_lots(&m, 150_000, PRICE_150, true).unwrap(), 1);
        // Two thirds of a lot: floor buys nothing, ceil rounds to one.
        assert_eq!(
            quote_to_base_lots(&m, 100_000, PRICE_150, false).unwrap(),
            0
        );
        assert_eq!(quote_to_base_lots(&m, 100_000, PRICE_150, true).unwrap(), 1);
    }

    #[test]
    fn quote_to_base_rejects_zero_price() {
        let m = sol_usdc(1);
        assert_eq!(
            quote_to_base_lots(&m, 1, 0, false),
            Err(ArcherError::PriceOverflow)
        );
    }

    #[test]
    fn conversions_round_trip_through_the_program_rounding() {
        let m = sol_usdc(1);
        for base_lots in [1u64, 7, 123, 10_000] {
            let quote = base_to_quote_lots(&m, base_lots, PRICE_150, Side::Bid).unwrap();
            assert_eq!(
                quote_to_base_lots(&m, quote, PRICE_150, false).unwrap(),
                base_lots
            );
        }
    }

    #[test]
    fn fee_ceils_for_positive_ppm() {
        // 33 lots × 1000 ppm = 0.033 → 1.
        assert_eq!(calculate_fee(33, 1_000).unwrap(), 1);
        // Exactly one lot: no rounding.
        assert_eq!(calculate_fee(1_000_000, 1).unwrap(), 1);
        // One lot and a bit → 2.
        assert_eq!(calculate_fee(1_000_001, 1).unwrap(), 2);
    }

    #[test]
    fn fee_truncates_toward_zero_for_rebates() {
        assert_eq!(calculate_fee(33, -1_000).unwrap(), 0);
        assert_eq!(calculate_fee(33_000, -1_000).unwrap(), -33);
    }

    #[test]
    fn fee_is_zero_for_zero_inputs() {
        assert_eq!(calculate_fee(1_000, 0).unwrap(), 0);
        assert_eq!(calculate_fee(0, 1_000).unwrap(), 0);
    }

    #[test]
    fn pro_rata_floors_and_rejects_zero_shares() {
        assert_eq!(calculate_pro_rata(10, 1, 3).unwrap(), 3);
        assert_eq!(calculate_pro_rata(10, 2, 3).unwrap(), 6);
        assert_eq!(calculate_pro_rata(10, 3, 3).unwrap(), 10);
        assert_eq!(
            calculate_pro_rata(10, 1, 0),
            Err(ArcherError::ArithmeticOverflow)
        );
    }

    #[test]
    fn bid_budget_scales_for_taker_and_builder_fees() {
        // 1_000_000 · 1e6 / 1_000_100 = 999_900.0099… → 999_900
        assert_eq!(
            max_amount_in_bid_budget(1_000_000, 100, 0).unwrap(),
            999_900
        );
        // Builder fee is added to the taker fee in the denominator.
        assert_eq!(
            max_amount_in_bid_budget(1_000_000, 100, 50).unwrap(),
            999_850
        );
        // No add-on fee: untouched. Negative taker fees are not pre-applied.
        assert_eq!(
            max_amount_in_bid_budget(1_000_000, 0, 0).unwrap(),
            1_000_000
        );
        assert_eq!(
            max_amount_in_bid_budget(1_000_000, -200, 0).unwrap(),
            1_000_000
        );
    }
}
