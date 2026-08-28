use crate::{
    config::MarketConfig,
    error::{ArcherSDKError, SdkResult},
};

/// Convert a base token amount to base lots (floor).
///
/// E.g., 10.5 SOL → base lots.
///
/// # Errors
///
/// - [`ArcherSDKError::InvalidSize`] if amount is negative, NaN, or infinite.
/// - [`ArcherSDKError::SizeBelowResolution`] if the result rounds to zero.
pub fn base_amount_to_lots(amount: f64, config: &MarketConfig) -> SdkResult<u64> {
    validate_size(amount)?;

    if amount == 0.0 {
        return Ok(0);
    }

    let lots_f64 = amount * config.base_to_lots_factor();

    if lots_f64 < 1.0 {
        return Err(ArcherSDKError::SizeBelowResolution(amount));
    }

    if lots_f64 > u64::MAX as f64 {
        return Err(ArcherSDKError::ArithmeticOverflow {
            operation: "base_amount_to_lots",
        });
    }

    Ok(lots_f64.floor() as u64)
}

/// Convert base lots to a human-readable base token amount.
#[inline]
pub fn base_lots_to_amount(lots: u64, config: &MarketConfig) -> f64 {
    lots as f64 * config.lots_to_base_factor()
}

/// Convert a quote token amount to quote lots (floor).
///
/// E.g., 1500.0 USDC → quote lots.
pub fn quote_amount_to_lots(amount: f64, config: &MarketConfig) -> SdkResult<u64> {
    validate_size(amount)?;

    if amount == 0.0 {
        return Ok(0);
    }

    let lots_f64 = amount * config.quote_to_lots_factor();

    if lots_f64 < 1.0 {
        return Err(ArcherSDKError::SizeBelowResolution(amount));
    }

    if lots_f64 > u64::MAX as f64 {
        return Err(ArcherSDKError::ArithmeticOverflow {
            operation: "quote_amount_to_lots",
        });
    }

    Ok(lots_f64.floor() as u64)
}

/// Convert quote lots to a human-readable quote token amount.
#[inline]
pub fn quote_lots_to_amount(lots: u64, config: &MarketConfig) -> f64 {
    lots as f64 * config.lots_to_quote_factor()
}

/// Convert a base token amount to raw atoms (floor).
///
/// This is rarely needed directly — most operations use lots.
/// Useful for deposit/withdrawal which operate in atoms.
pub fn base_amount_to_atoms(amount: f64, config: &MarketConfig) -> SdkResult<u64> {
    validate_size(amount)?;

    if amount == 0.0 {
        return Ok(0);
    }

    let atoms_f64 = amount * config.base_atoms_divisor();

    if atoms_f64 > u64::MAX as f64 {
        return Err(ArcherSDKError::ArithmeticOverflow {
            operation: "base_amount_to_atoms",
        });
    }

    Ok(atoms_f64.floor() as u64)
}

/// Convert raw base atoms to a human-readable amount.
#[inline]
pub fn base_atoms_to_amount(atoms: u64, config: &MarketConfig) -> f64 {
    atoms as f64 / config.base_atoms_divisor()
}

/// Convert a quote token amount to raw atoms (floor).
pub fn quote_amount_to_atoms(amount: f64, config: &MarketConfig) -> SdkResult<u64> {
    validate_size(amount)?;

    if amount == 0.0 {
        return Ok(0);
    }

    let atoms_f64 = amount * config.quote_atoms_divisor();

    if atoms_f64 > u64::MAX as f64 {
        return Err(ArcherSDKError::ArithmeticOverflow {
            operation: "quote_amount_to_atoms",
        });
    }

    Ok(atoms_f64.floor() as u64)
}

/// Convert raw quote atoms to a human-readable amount.
#[inline]
pub fn quote_atoms_to_amount(atoms: u64, config: &MarketConfig) -> f64 {
    atoms as f64 / config.quote_atoms_divisor()
}

/// Get the minimum representable base token amount (1 lot in token terms).
///
/// Useful for UIs to show the minimum order size.
#[inline]
pub fn min_base_amount(config: &MarketConfig) -> f64 {
    config.lots_to_base_factor()
}

/// Get the minimum representable quote token amount (1 lot in token terms).
#[inline]
pub fn min_quote_amount(config: &MarketConfig) -> f64 {
    config.lots_to_quote_factor()
}

fn validate_size(amount: f64) -> SdkResult<()> {
    if !amount.is_finite() || amount < 0.0 {
        return Err(ArcherSDKError::InvalidSize(amount));
    }
    Ok(())
}
