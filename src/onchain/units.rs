//! Archer Units System
//!
//! A type-safe numeric units system.
//! This implementation provides compile time guarantees that prevent unit mixing errors
//! in financial calculations.
//!
//! # Design Principles
//!
//! - **Type Safety**: Different unit types cannot be accidentally mixed
//! - **Zero Cost**: Uses `#[repr(transparent)]` for zero runtime overhead
//! - **Compile-Time Enforcement**: Invalid operations won't compile
//!
//! # Unit Hierarchy
//!
//! ```text
//! ATOMS    → Raw token amounts (blockchain precision)
//!   ↕
//! LOTS     → Standardized trading increments (orderbook precision)
//!   ↕
//! UNITS    → Human-readable amounts (UI display)
//! ```
//!

use borsh::{BorshDeserialize, BorshSerialize};
use bytemuck::{Pod, Zeroable};
use std::fmt::{self, Display};
use std::iter::Sum;
use std::ops::{Add, AddAssign, Div, Mul, Rem, Sub, SubAssign};

pub use bytemuck;


/// Core trait for all unit types in the Archer system.
///
/// This trait provides the fundamental interface for creating and accessing
/// the underlying u64 values of type-safe unit wrappers.
pub trait ArcherUnit: Copy + Clone + PartialEq + Eq + PartialOrd + Ord {
    /// Create a new instance with the given value
    fn new(value: u64) -> Self;

    /// Get the underlying u64 value
    fn as_u64(&self) -> u64;

    /// Convert to u128 for intermediate calculations
    fn as_u128(&self) -> u128 {
        self.as_u64() as u128
    }

    /// Caution: Use only for fee calculations, not for amounts
    fn as_i64(&self) -> i64;

    /// Caution: Use only for fee calculations, not for amounts
    fn as_i128(&self) -> i128 {
        self.as_i64() as i128
    }

    /// Saturating subtraction (returns zero on underflow)
    fn saturating_sub(self, other: Self) -> Self {
        Self::new(self.as_u64().saturating_sub(other.as_u64()))
    }

    /// Saturating addition (returns max on overflow)
    fn saturating_add(self, other: Self) -> Self {
        Self::new(self.as_u64().saturating_add(other.as_u64()))
    }

    /// Checked addition (returns None on overflow)
    fn checked_add(self, other: Self) -> Option<Self> {
        self.as_u64().checked_add(other.as_u64()).map(Self::new)
    }

    /// Checked subtraction (returns None on underflow)
    fn checked_sub(self, other: Self) -> Option<Self> {
        self.as_u64().checked_sub(other.as_u64()).map(Self::new)
    }

    /// Checked multiplication (returns None on overflow)
    fn checked_mul_u64(self, scalar: u64) -> Option<Self> {
        self.as_u64().checked_mul(scalar).map(Self::new)
    }

    /// Unchecked division with type conversion
    ///
    /// # Warning
    /// This performs unchecked division and type conversion. Use only when
    /// you know the divisor is non-zero and the result type is correct.
    fn unchecked_div<Divisor: ArcherUnit, Quotient: ArcherUnit>(
        self,
        divisor: Divisor,
    ) -> Quotient {
        assert!(divisor.as_u64() != 0, "division by zero");
        Quotient::new(self.as_u64() / divisor.as_u64())
    }
}

/// Macro to define a basic unit type with standard implementations.
///
/// This macro generates:
/// - Struct definition with `#[repr(transparent)]`
/// - ArcherUnit trait implementation
/// - Standard arithmetic operations (Add, Sub, Mul)
/// - Display and conversion traits
/// - Useful constants (ZERO, ONE, MAX, MIN)
macro_rules! define_unit {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialOrd,
            Ord,
            PartialEq,
            Eq,
            Zeroable,
            Pod,
            BorshDeserialize,
            BorshSerialize,
        )]
        #[repr(transparent)]
        pub struct $name {
            inner: u64,
        }

        impl ArcherUnit for $name {
            #[inline(always)]
            fn new(value: u64) -> Self {
                $name { inner: value }
            }

            #[inline(always)]
            fn as_u64(&self) -> u64 {
                self.inner
            }

            #[inline(always)]
            fn as_i64(&self) -> i64 {
                i64::try_from(self.inner).expect("value exceeds i64::MAX")
            }
        }

        impl $name {
            pub const ZERO: Self = $name { inner: 0 };

            pub const ONE: Self = $name { inner: 1 };

            pub const MAX: Self = $name { inner: u64::MAX };

            pub const MIN: Self = $name { inner: u64::MIN };

            #[inline(always)]
            pub const fn is_zero(&self) -> bool {
                self.inner == 0
            }

            #[inline(always)]
            pub const fn is_non_zero(&self) -> bool {
                self.inner != 0
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "{}", self.inner)
            }
        }

        impl Default for $name {
            #[inline(always)]
            fn default() -> Self {
                Self::ZERO
            }
        }

        impl Mul<u64> for $name {
            type Output = Self;

            #[inline]
            fn mul(self, scalar: u64) -> Self {
                $name::new(self.inner.checked_mul(scalar).expect("overflow"))
            }
        }

        impl Mul<$name> for u64 {
            type Output = $name;

            #[inline]
            fn mul(self, unit: $name) -> $name {
                $name::new(self.checked_mul(unit.inner).expect("overflow"))
            }
        }

        impl Add for $name {
            type Output = Self;

            #[inline]
            fn add(self, other: Self) -> Self {
                $name::new(self.inner.checked_add(other.inner).expect("overflow"))
            }
        }

        impl AddAssign for $name {
            #[inline]
            fn add_assign(&mut self, other: Self) {
                self.inner = self.inner.checked_add(other.inner).expect("overflow");
            }
        }

        impl Sub for $name {
            type Output = Self;

            #[inline]
            fn sub(self, other: Self) -> Self {
                $name::new(self.inner.checked_sub(other.inner).expect("underflow"))
            }
        }

        impl SubAssign for $name {
            #[inline]
            fn sub_assign(&mut self, other: Self) {
                self.inner = self.inner.checked_sub(other.inner).expect("underflow");
            }
        }

        impl Sum<$name> for $name {
            fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
                iter.fold($name::ZERO, |acc, x| acc + x)
            }
        }

        impl From<$name> for u64 {
            #[inline(always)]
            fn from(x: $name) -> u64 {
                x.inner
            }
        }

        impl From<$name> for u128 {
            #[inline(always)]
            fn from(x: $name) -> u128 {
                x.inner as u128
            }
        }

        

        #[cfg(test)]
        impl PartialEq<$name> for u64 {
            fn eq(&self, other: &$name) -> bool {
                *self == other.inner
            }
        }
    };
}

/// Macro to allow multiplication between two different unit types.
///
/// This macro defines the valid unit conversions through multiplication
/// and the corresponding division operations. It ensures type safety by
/// only allowing operations that make dimensional sense.
///
/// # Example
/// ```ignore
/// allow_multiply!(BaseUnits, BaseLotsPerBaseUnit => BaseLots);
/// // Now you can do: BaseUnits * BaseLotsPerBaseUnit = BaseLots
/// // And: BaseLots / BaseUnits = BaseLotsPerBaseUnit
/// // And: BaseLots / BaseLotsPerBaseUnit = BaseUnits
/// ```
macro_rules! allow_multiply {
    ($type_a:ty, $type_b:ty => $result:ty) => {
        // Forward multiplication: A * B = Result
        impl Mul<$type_b> for $type_a {
            type Output = $result;

            #[inline]
            fn mul(self, other: $type_b) -> $result {
                <$result>::new(self.as_u64().checked_mul(other.as_u64()).expect("overflow"))
            }
        }

        // Reverse multiplication: B * A = Result
        impl Mul<$type_a> for $type_b {
            type Output = $result;

            #[inline]
            fn mul(self, other: $type_a) -> $result {
                <$result>::new(self.as_u64().checked_mul(other.as_u64()).expect("overflow"))
            }
        }

        // Division: Result / A = B
        impl Div<$type_a> for $result {
            type Output = $type_b;

            #[inline]
            #[track_caller]
            fn div(self, other: $type_a) -> $type_b {
                let divisor = other.as_u64();
                assert!(divisor != 0, "division by zero");
                #[cfg(not(kani))]
                if cfg!(debug_assertions) {
                    if self.as_u64() % divisor != 0 {
                        solana_program::msg!(
                            "WARNING: Non-clean division: {} / {} has remainder",
                            self.as_u64(),
                            divisor
                        );
                    }
                }
                <$type_b>::new(
                    self.as_u64()
                        .checked_div(divisor)
                        .expect("division by zero"),
                )
            }
        }

        // Division: Result / B = A
        impl Div<$type_b> for $result {
            type Output = $type_a;

            #[inline]
            #[track_caller]
            fn div(self, other: $type_b) -> $type_a {
                let divisor = other.as_u64();
                assert!(divisor != 0, "division by zero");
                #[cfg(not(kani))]
                if cfg!(debug_assertions) {
                    if self.as_u64() % divisor != 0 {
                        solana_program::msg!(
                            "WARNING: Non-clean division: {} / {} has remainder",
                            self.as_u64(),
                            divisor
                        );
                    }
                }
                <$type_a>::new(
                    self.as_u64()
                        .checked_div(divisor)
                        .expect("division by zero"),
                )
            }
        }
    };
}

macro_rules! allow_modulo {
    ($dividend:ty, $divisor:ty) => {
        impl Rem<$divisor> for $dividend {
            type Output = u64;

            #[inline]
            fn rem(self, other: $divisor) -> u64 {
                self.as_u64() % other.as_u64()
            }
        }
    };
}

define_unit!(
    BaseAtoms,
    "Raw base token amount at blockchain precision.\
     Example: 1 SOL = 1,000,000,000 BaseAtoms (9 decimals)"
);

define_unit!(
    BaseLots,
    "Standardized base token trading increment.\
     Used in the orderbook for efficient price-size representation.
     Example: 1 BaseLot might equal 1,000,000 BaseAtoms"
);

define_unit!(
    BaseUnits,
    "User-facing base token amount.\
     Typically represents whole tokens as displayed in the UI.
     Example: 5 BaseUnits = 5 SOL"
);

define_unit!(
    QuoteAtoms,
    "Raw quote token amount at blockchain precision.\
     Example: 1 USDC = 1,000,000 QuoteAtoms (6 decimals)"
);

define_unit!(
    QuoteLots,
    "Standardized quote token trading increment.\
     Used in the orderbook for efficient price-size representation.
     Example: 1 QuoteLot might equal 1,000 QuoteAtoms"
);

define_unit!(
    QuoteUnits,
    "User-facing quote token amount.\
     Typically represents whole tokens as displayed in the UI.
     Example: 100 QuoteUnits = 100 USDC"
);

define_unit!(
    BaseAtomsPerLot,
    "Conversion factor: how many atoms in one base lot.\
     Used to convert between orderbook precision (lots) and
     blockchain precision (atoms) for base token."
);

define_unit!(
    QuoteAtomsPerLot,
    "Conversion factor: how many atoms in one quote lot.\
     Used to convert between orderbook precision (lots) and
     blockchain precision (atoms) for quote token."
);

define_unit!(
    BaseLotsPerUnit,
    "Conversion factor: how many lots in one base unit.\
     Used to convert between user-facing amounts (units) and
     orderbook precision (lots) for base token."
);

define_unit!(
    QuoteLotsPerUnit,
    "Conversion factor: how many lots in one quote unit.\
     Used to convert between user-facing amounts (units) and
     orderbook precision (lots) for quote token."
);

define_unit!(
    BaseAtomsPerUnit,
    "Conversion factor: how many atoms in one base unit.\
     Combines atoms-per-lot and lots-per-unit conversions.
     Example: For SOL with 9 decimals, this is 1,000,000,000"
);

define_unit!(
    QuoteAtomsPerUnit,
    "Conversion factor: how many atoms in one quote unit.\
     Combines atoms-per-lot and lots-per-unit conversions.
     Example: For USDC with 6 decimals, this is 1,000,000"
);

define_unit!(
    Ticks,
    "Discrete price level increment.\
     Prices in the auction can only occur at tick boundaries.
     Example: If tick size is 0.01 USDC/SOL, then 150 USDC/SOL = 15,000 Ticks"
);

define_unit!(
    QuoteLotsPerBaseUnit,
    "Price in quote lots per base unit.\
     Represents the actual price at which trades execute.
     Example: 15,000 QuoteLotsPerBaseUnit might mean 150 USDC per SOL"
);

define_unit!(
    QuoteLotsPerBaseUnitPerTick,
    "Tick size in quote lots per base unit.\
     Defines the minimum price increment for the market.
     Example: 1 QuoteLotsPerBaseUnitPerTick means price moves in increments of 0.01"
);

define_unit!(
    QuoteAtomsPerBaseUnitPerTick,
    "Tick size in quote atoms per base unit.\
     Defines the minimum price increment for the market."
);

define_unit!(
    AdjustedQuoteLots,
    "Intermediate calculation value for quote lots.\
     Used in multi-step conversions where quote lots need adjustment
     before final division. Helps maintain precision in complex calculations."
);

allow_multiply!(BaseUnits, BaseLotsPerUnit => BaseLots);
allow_multiply!(BaseLots, BaseAtomsPerLot => BaseAtoms);
allow_multiply!(BaseAtomsPerLot, BaseLotsPerUnit => BaseAtomsPerUnit);

allow_multiply!(QuoteUnits, QuoteLotsPerUnit => QuoteLots);
allow_multiply!(QuoteLots, QuoteAtomsPerLot => QuoteAtoms);
allow_multiply!(QuoteAtomsPerLot, QuoteLotsPerUnit => QuoteAtomsPerUnit);

allow_multiply!(Ticks, QuoteLotsPerBaseUnitPerTick => QuoteLotsPerBaseUnit);

allow_multiply!(QuoteLotsPerBaseUnit, BaseLots => AdjustedQuoteLots);
allow_multiply!(QuoteLotsPerBaseUnit, QuoteAtomsPerLot => QuoteAtomsPerBaseUnitPerTick);
allow_multiply!(QuoteLots, BaseLotsPerUnit => AdjustedQuoteLots);

allow_modulo!(AdjustedQuoteLots, BaseLotsPerUnit);
allow_modulo!(BaseAtomsPerUnit, BaseLotsPerUnit);
allow_modulo!(QuoteAtomsPerUnit, QuoteLotsPerUnit);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_conversions() {
        // 5 units * 100 lots/unit = 500 lots
        let units = BaseUnits::new(5);
        let lots_per_unit = BaseLotsPerUnit::new(100);
        let lots = units * lots_per_unit;
        assert_eq!(lots, BaseLots::new(500));

        // 500 lots * 1,000,000 atoms/lot = 500,000,000 atoms
        let atoms_per_lot = BaseAtomsPerLot::new(1_000_000);
        let atoms = lots * atoms_per_lot;
        assert_eq!(atoms, BaseAtoms::new(500_000_000));
    }

    #[test]
    fn test_price_calc() {
        // 500 lots * price 150 = adjusted 75,000
        let base_lots = BaseLots::new(500);
        let price = QuoteLotsPerBaseUnit::new(150);
        let adjusted = price * base_lots;
        assert_eq!(adjusted, AdjustedQuoteLots::new(75_000));

        // 75,000 / 100 lots/unit = 750 quote lots
        let lots_per_unit = BaseLotsPerUnit::new(100);
        let quote_lots = adjusted / lots_per_unit;
        assert_eq!(quote_lots, QuoteLots::new(750));
    }

    #[test]
    fn test_type_safety() {
        let base = BaseLots::new(100);
        // let quote = QuoteLots::new(100);

        // This compiles - same type
        let _ = base + base;

        // This would NOT compile - different types
        // let _ = base + quote; // Error!
    }
}
