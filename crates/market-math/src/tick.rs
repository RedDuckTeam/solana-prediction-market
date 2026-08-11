//! Converting a concentrated-liquidity tick into a price.
//!
//! A CLMM tick `t` denotes the price `1.0001^t`. TWAP readers hand us an
//! average tick and need the price it stands for, exactly and identically on
//! chain and in the browser.

use crate::q64::Q64;
use crate::tables::{MAX_TICK, TICK_INV};
use crate::u256::U256;
use crate::MathError;

/// Q128.128 rather than Q64.64, so the 19 chained multiplications lose well
/// under one ulp of the Q64.64 result.
const INTERMEDIATE_BITS: u32 = 128;

/// `1.0001^tick` as a Q64.64 value.
///
/// Accumulated from the `1.0001^(-2^k)` table so the product stays in `(0, 1]`,
/// with one final reciprocal for positive ticks — one truncation instead of one
/// per entry. Same construction as Uniswap V3's `getSqrtRatioAtTick`.
///
/// Rejects beyond ±[`MAX_TICK`], tighter than Raydium's ±443636: past it a
/// Q64.64 price holds under 21 significant bits.
pub fn pow_1_0001(tick: i32) -> Result<Q64, MathError> {
    if tick.unsigned_abs() > MAX_TICK.unsigned_abs() {
        return Err(MathError::TickOutOfRange);
    }

    // ratio = 1.0001^(-|tick|) in Q128.128.
    let magnitude = tick.unsigned_abs();
    let mut ratio = U256::pow2(INTERMEDIATE_BITS);
    for (bit, factor) in TICK_INV.iter().enumerate() {
        if magnitude & (1u32 << bit) != 0 {
            ratio = (ratio * U256::from(*factor)) >> INTERMEDIATE_BITS;
        }
    }

    let raw = if tick <= 0 {
        // Q128.128 -> Q64.64 is a plain shift; the value is already <= 1.
        ratio >> (INTERMEDIATE_BITS - Q64::FRACTIONAL_BITS)
    } else {
        // We want 2^64 / (ratio / 2^128), i.e. 2^192 / ratio. With
        // |tick| <= MAX_TICK the result is at most ~1.97e32, well inside u128.
        U256::pow2(INTERMEDIATE_BITS + Q64::FRACTIONAL_BITS) / ratio
    };

    let raw = raw.to_u128().ok_or(MathError::Overflow)?;
    i128::try_from(raw)
        .map(Q64::from_raw)
        .map_err(|_| MathError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pow_vectors::POW_VECTORS;
    use proptest::prelude::*;

    /// The error here is relative, not absolute, so the tolerance is too: a
    /// larger price simply has more ulps.
    ///
    /// `2^-80` sits about forty times above the worst relative error measured
    /// against a 120-digit reference over the whole tick range (1.9e-26). Any
    /// looser and a real precision regression would pass unnoticed.
    const RELATIVE_TOLERANCE_BITS: u32 = 80;

    fn tolerance(expected: i128) -> i128 {
        // The +1 covers the final truncation, which is a whole ulp regardless
        // of how small the value is.
        1 + (expected >> RELATIVE_TOLERANCE_BITS)
    }

    #[test]
    fn matches_arbitrary_precision_reference() {
        for (tick, expected) in POW_VECTORS {
            let got = pow_1_0001(tick).expect("tick is within range").raw();
            let delta = got - expected;
            let tolerance = tolerance(expected);
            assert!(
                delta.abs() <= tolerance,
                "tick {tick}: got {got}, expected {expected}, \
                 delta {delta} exceeds tolerance {tolerance}"
            );
        }
    }

    #[test]
    fn the_table_spans_every_accepted_tick() {
        // `pow_1_0001` consumes one table entry per bit of the magnitude, so a
        // `MAX_TICK` wider than the table would silently drop the high bits and
        // return a wrong price with no error at all. Raydium's own limit,
        // 443636, is still inside; the next doubling would not be.
        let widest_representable = 1u32 << TICK_INV.len();
        assert!(
            MAX_TICK.unsigned_abs() < widest_representable,
            "MAX_TICK {MAX_TICK} needs more than {} table entries",
            TICK_INV.len()
        );
    }

    #[test]
    fn tick_zero_is_exactly_one() {
        assert_eq!(pow_1_0001(0).unwrap(), Q64::ONE);
    }

    #[test]
    fn out_of_range_ticks_are_rejected() {
        assert_eq!(pow_1_0001(MAX_TICK + 1), Err(MathError::TickOutOfRange));
        assert_eq!(pow_1_0001(-MAX_TICK - 1), Err(MathError::TickOutOfRange));
        assert_eq!(pow_1_0001(i32::MIN), Err(MathError::TickOutOfRange));
        assert!(pow_1_0001(MAX_TICK).is_ok());
        assert!(pow_1_0001(-MAX_TICK).is_ok());
    }

    #[test]
    fn reciprocal_ticks_multiply_to_one() {
        for tick in [1i32, 17, 1000, 60_000, 262_144, MAX_TICK] {
            let up = pow_1_0001(tick).unwrap();
            let down = pow_1_0001(-tick).unwrap();
            let product = up.mul(down).unwrap();
            let delta = (product.raw() - Q64::ONE.raw()).unsigned_abs();

            // The dominant error is one ulp on whichever factor is smaller:
            // a Q64.64 value near `x` resolves to `2^-64`, i.e. a relative
            // `2^-64 / x`. Allow four such ulps to cover both factors plus the
            // flooring inside `mul`.
            let smaller = up.raw().min(down.raw()).unsigned_abs();
            let budget = 4 * Q64::ONE.raw().unsigned_abs();
            assert!(
                delta * smaller <= budget,
                "tick {tick}: product {product:?} is {delta} ulps from one, \
                 budget allows {}",
                budget / smaller
            );
        }
    }

    /// Every adjacent pair in the accepted range, not a sample of them.
    ///
    /// Six hundred thousand evaluations is a second of wall clock, and it
    /// turns "we did not find a counterexample" into "there is none". Sampling
    /// missed the topmost pair entirely.
    #[test]
    fn strictly_monotonic_across_the_entire_range() {
        let mut previous = pow_1_0001(-MAX_TICK).expect("in range");
        for tick in (-MAX_TICK + 1)..=MAX_TICK {
            let current = pow_1_0001(tick).expect("in range");
            assert!(
                previous < current,
                "tick {tick}: {previous:?} is not below {current:?}"
            );
            previous = current;
        }
    }

    proptest! {
        /// Never panics, never returns a non-positive price.
        #[test]
        fn always_positive_within_range(tick in -MAX_TICK..=MAX_TICK) {
            let price = pow_1_0001(tick).unwrap();
            prop_assert!(price.raw() > 0, "tick {tick} produced {price:?}");
        }

        #[test]
        fn never_panics_for_any_i32(tick in any::<i32>()) {
            let _ = pow_1_0001(tick);
        }
    }
}
