//! Turning an averaged tick into a price the predicate can compare.

use market_math::{pow_1_0001, Q64};

use crate::FeedError;

/// Accepted price band, as powers of two.
///
/// The top leaves eight bits of Q64.64 headroom, so composing feeds cannot fall
/// off the type. The floor is resolution, not range: near `2^-56` an ulp is
/// 0.39% of the price and a 50 bp band would hold two distinct values, which is
/// the step function the ramp exists to replace. At `2^-40` it holds ~160 000.
const MIN_PRICE_LOG2: i32 = -40;
const MAX_PRICE_LOG2: i32 = 56;

/// Price of `token0` in `token1`, rescaled by the decimal difference because a
/// tick encodes the ratio of raw amounts.
///
/// `invert` returns the reciprocal, which is what lets one pool serve as either
/// leg of a composed feed -- TKN/SOL and SOL/USDC to price TKN in USDC.
pub fn normalized_price(
    average_tick: i32,
    token0_decimals: u8,
    token1_decimals: u8,
    invert: bool,
) -> Result<Q64, FeedError> {
    let raw_ratio = pow_1_0001(average_tick)?;

    let decimal_shift = i32::from(token0_decimals) - i32::from(token1_decimals);
    let price = raw_ratio.scale_pow10(decimal_shift)?;

    let price = if invert { Q64::ONE.div(price)? } else { price };

    if !in_representable_band(price) {
        return Err(FeedError::PriceOutOfRange);
    }
    Ok(price)
}

/// The reciprocal of a price, held to the same representable band as a price
/// read directly.
///
/// This is the one place a price is derived from another price rather than
/// from a source, so it is the one place the band has to be re-checked: a
/// value near the band's floor inverts to one near `2^40`, far past where
/// composing feeds stays inside the type.
pub fn invert_price(price: Q64) -> Result<Q64, FeedError> {
    let inverted = Q64::ONE.div(price)?;
    if !in_representable_band(inverted) {
        return Err(FeedError::PriceOutOfRange);
    }
    Ok(inverted)
}

/// Compared against raw bounds rather than `ilog2`, which floors and would have
/// admitted everything below `2^(MAX+1)`.
pub(crate) fn in_representable_band(price: Q64) -> bool {
    let raw = price.raw();
    let lowest = 1i128 << (Q64::FRACTIONAL_BITS as i32 + MIN_PRICE_LOG2);
    let highest = 1i128 << (Q64::FRACTIONAL_BITS as i32 + MAX_PRICE_LOG2);
    raw >= lowest && raw <= highest
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tick of a SOL/USDC pool around $200.
    ///
    /// token0 is SOL (9 decimals), token1 is USDC (6), so the raw ratio is
    /// 0.2 and the tick is negative -- the ordinary case, and the one where
    /// truncating instead of flooring would cost a basis point.
    const SOL_USDC_TICK: i32 = -16_096;

    fn approximately(price: Q64, expected: f64, tolerance: f64) -> bool {
        let actual = price.raw() as f64 / 2f64.powi(64);
        (actual - expected).abs() <= tolerance
    }

    #[test]
    fn sol_usdc_tick_prices_sol_in_usdc() {
        let price = normalized_price(SOL_USDC_TICK, 9, 6, false).unwrap();
        assert!(
            approximately(price, 200.0, 0.05),
            "expected about 200 USDC, got {price:?}"
        );
    }

    #[test]
    fn inverting_prices_the_other_side_of_the_pair() {
        let price = normalized_price(SOL_USDC_TICK, 9, 6, true).unwrap();
        assert!(
            approximately(price, 1.0 / 200.0, 1e-5),
            "expected about 0.005 SOL, got {price:?}"
        );
    }

    #[test]
    fn inverting_twice_returns_the_original() {
        let forward = normalized_price(SOL_USDC_TICK, 9, 6, false).unwrap();
        let inverted = normalized_price(SOL_USDC_TICK, 9, 6, true).unwrap();
        let back = Q64::ONE.div(inverted).unwrap();

        // Compared against the forward price, not against a round 200: tick
        // -16096 is 199.9837, and pinning the test to a number the tick does
        // not actually encode would only measure how well the tick was chosen.
        let drift = (back.raw() - forward.raw()).unsigned_abs();
        let relative_bound = forward.raw().unsigned_abs() >> 40;
        assert!(
            drift <= relative_bound,
            "round trip drifted by {drift} ulps, bound {relative_bound}: \
             {forward:?} -> {inverted:?} -> {back:?}"
        );
    }

    #[test]
    fn equal_decimals_need_no_rescale() {
        let price = normalized_price(0, 6, 6, false).unwrap();
        assert_eq!(price, Q64::ONE);
    }

    #[test]
    fn extreme_decimal_skew_is_rejected_rather_than_silently_wrong() {
        // A price of ~1e18 is representable but outside the band we are willing
        // to settle against.
        assert_eq!(
            normalized_price(0, 18, 0, false),
            Err(FeedError::PriceOutOfRange)
        );
        assert_eq!(
            normalized_price(0, 0, 18, false),
            Err(FeedError::PriceOutOfRange)
        );
    }

    #[test]
    fn the_upper_bound_is_where_it_says_it_is() {
        // Regression: testing `ilog2` against the exponent accepted everything
        // below 2^(MAX+1), making the band twice as wide as documented. The
        // old test happened to pick 10^18, which cleared both bounds, so the
        // factor of two was invisible.
        assert_eq!(
            normalized_price(0, 17, 0, false),
            Err(FeedError::PriceOutOfRange),
            "1e17 is above 2^56 and must be refused"
        );
        assert!(
            normalized_price(0, 16, 0, false).is_ok(),
            "1e16 is below 2^56"
        );
    }

    #[test]
    fn the_lower_bound_leaves_a_ramp_something_to_resolve() {
        // At the floor of the band the ulp must stay far below a settlement
        // band, or the ramp degenerates into the step function it replaces.
        let smallest = normalized_price(0, 0, 12, false).expect("1e-12 is inside the band");

        // What matters is not the ulp in the abstract but how many distinct
        // payouts a settlement band can still express at the floor of the
        // price range. A 50 bp band spans `raw * 50/10000` ulps.
        let levels = smallest.raw() * 50 / 10_000;
        assert!(
            levels > 10_000,
            "the narrowest band resolves only {levels} levels at the floor price"
        );
        assert_eq!(
            normalized_price(0, 0, 13, false),
            Err(FeedError::PriceOutOfRange),
            "1e-13 is below the band"
        );
    }

    #[test]
    fn ticks_beyond_the_supported_band_are_rejected() {
        assert_eq!(
            normalized_price(400_000, 6, 6, false),
            Err(FeedError::TickOutOfRange)
        );
    }
}
