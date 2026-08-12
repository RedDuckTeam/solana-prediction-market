//! The settlement ramp: predicate score -> fraction of the pot owed to YES.
//!
//! Compared against the strike over a band, not at a point. Under a step payout
//! the gain from pushing a price across the strike is the whole losing pool
//! while the cost falls to zero as the price nears it, so profit over cost
//! diverges. Over a band the gain is linear and the cost convex, so it is
//! bounded. Outside the band this is exactly a binary market.
//!
//! Applied by the protocol, never the predicate: a band in bytecode could not
//! be checked on chain, so a creator could publish a step function.

use market_math::Q64;

use crate::{CoreError, BPS_DENOMINATOR};

/// Fraction of the pot owed to YES, in `[0, 1]`.
///
/// `ramp_bps` is the half-width of the band, in basis points of the strike: at
/// 50, a market on a 100 USDC strike splits the pot continuously between 99.50
/// and 100.50 and pays out fully outside that.
pub fn apply_ramp(score: Q64, strike: Q64, ramp_bps: u16) -> Result<Q64, CoreError> {
    if strike <= Q64::ZERO {
        return Err(CoreError::ShareOutOfRange);
    }
    if ramp_bps == 0 || u64::from(ramp_bps) > BPS_DENOMINATOR {
        return Err(CoreError::FeeRateOutOfRange);
    }

    let half_width = strike
        .mul(Q64::from_uint(u64::from(ramp_bps)))
        .and_then(|scaled| scaled.div(Q64::from_uint(BPS_DENOMINATOR)))
        .map_err(|_| CoreError::Overflow)?;

    // A strike small enough that the band rounds away would make every market
    // on it a step function again.
    if half_width <= Q64::ZERO {
        return Err(CoreError::ShareOutOfRange);
    }

    let lower = strike.sub(half_width).map_err(|_| CoreError::Overflow)?;
    if score <= lower {
        return Ok(Q64::ZERO);
    }
    let upper = strike.add(half_width).map_err(|_| CoreError::Overflow)?;
    if score >= upper {
        return Ok(Q64::ONE);
    }

    let offset = score.sub(lower).map_err(|_| CoreError::Overflow)?;
    let width = upper.sub(lower).map_err(|_| CoreError::Overflow)?;
    let share = offset.div(width).map_err(|_| CoreError::Overflow)?;

    // The comparisons above already bound this; clamping is belt and braces
    // against a rounding edge putting a market outside its own invariant.
    Ok(share.max(Q64::ZERO).min(Q64::ONE))
}

/// Rejects, at creation, a strike and band that could only ever abort.
///
/// The strike is the worst case: a score inside the band exercises every step,
/// where one outside returns early. Otherwise impossible parameters surface as a
/// failed resolution, and a creator should not be able to guarantee a void.
pub fn validate_ramp(strike: Q64, ramp_bps: u16) -> Result<(), CoreError> {
    apply_ramp(strike, strike, ramp_bps).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn price(units: u64) -> Q64 {
        Q64::from_uint(units)
    }

    const HALF: Q64 = Q64::from_raw(1i128 << 63);

    #[test]
    fn the_strike_itself_splits_the_pot_evenly() {
        assert_eq!(apply_ramp(price(100), price(100), 50), Ok(HALF));
    }

    #[test]
    fn outside_the_band_it_is_an_ordinary_binary_market() {
        let strike = price(100);
        // 50 bps of 100 is 0.5, so the band is [99.5, 100.5].
        assert_eq!(apply_ramp(price(99), strike, 50), Ok(Q64::ZERO));
        assert_eq!(apply_ramp(price(101), strike, 50), Ok(Q64::ONE));
        assert_eq!(apply_ramp(price(1), strike, 50), Ok(Q64::ZERO));
        assert_eq!(apply_ramp(price(100_000), strike, 50), Ok(Q64::ONE));
    }

    #[test]
    fn the_band_edges_are_exactly_zero_and_one() {
        let strike = price(200);
        let half_width = price(1); // 50 bps of 200
        assert_eq!(
            apply_ramp(strike.sub(half_width).unwrap(), strike, 50),
            Ok(Q64::ZERO)
        );
        assert_eq!(
            apply_ramp(strike.add(half_width).unwrap(), strike, 50),
            Ok(Q64::ONE)
        );
    }

    #[test]
    fn a_quarter_of_the_way_across_pays_a_quarter() {
        // Strike 200, band [199, 201]; 199.5 is a quarter of the way up.
        let strike = price(200);
        let score = price(1_995).div(price(10)).unwrap();
        let quarter = Q64::ONE.div(price(4)).unwrap();
        let share = apply_ramp(score, strike, 50).unwrap();
        assert!(
            (share.raw() - quarter.raw()).unsigned_abs() <= 2,
            "expected about a quarter, got {share:?}"
        );
    }

    #[test]
    fn a_wider_band_moves_the_payout_less_per_unit_of_price() {
        // The whole point: doubling the band halves a manipulator's gain per
        // unit of price pushed, while the cost of pushing it is unchanged.
        let strike = price(100);
        let score = price(1_002).div(price(10)).unwrap(); // 100.2
        let narrow = apply_ramp(score, strike, 50).unwrap();
        let wide = apply_ramp(score, strike, 100).unwrap();

        let narrow_gain = narrow.sub(HALF).unwrap();
        let wide_gain = wide.sub(HALF).unwrap();
        assert!(wide_gain < narrow_gain);
        assert!(
            (narrow_gain.raw() - wide_gain.raw() * 2).unsigned_abs() <= 4,
            "halving should be proportional: {narrow_gain:?} vs {wide_gain:?}"
        );
    }

    #[test]
    fn parameters_that_could_only_abort_are_caught_at_creation() {
        assert_eq!(validate_ramp(price(100), 50), Ok(()));
        assert_eq!(
            validate_ramp(Q64::ZERO, 50),
            Err(CoreError::ShareOutOfRange)
        );
        assert_eq!(
            validate_ramp(price(100), 0),
            Err(CoreError::FeeRateOutOfRange)
        );

        // A strike near the top of Q64.64 overflows when multiplied by the
        // band; the market is refused rather than resolving into a void.
        let enormous = Q64::from_raw(i128::MAX / 2);
        assert_eq!(validate_ramp(enormous, 10_000), Err(CoreError::Overflow));

        // Whatever validation accepts, no score can then abort.
        for strike_units in [1u64, 100, 1_000_000, 1_000_000_000] {
            let strike = price(strike_units);
            if validate_ramp(strike, 50).is_ok() {
                for score_units in [0u64, 1, strike_units, strike_units * 2, u32::MAX as u64] {
                    assert!(
                        apply_ramp(price(score_units), strike, 50).is_ok(),
                        "strike {strike_units} accepted but score {score_units} aborted"
                    );
                }
            }
        }
    }

    #[test]
    fn degenerate_parameters_are_refused() {
        assert_eq!(
            apply_ramp(price(1), Q64::ZERO, 50),
            Err(CoreError::ShareOutOfRange)
        );
        assert_eq!(
            apply_ramp(price(1), price(100), 0),
            Err(CoreError::FeeRateOutOfRange)
        );
        assert_eq!(
            apply_ramp(price(1), price(100), 10_001),
            Err(CoreError::FeeRateOutOfRange)
        );
        // A strike so small the band underflows would be a step function.
        assert_eq!(
            apply_ramp(Q64::from_raw(1), Q64::from_raw(1), 50),
            Err(CoreError::ShareOutOfRange)
        );
    }

    proptest! {
        /// A score inside the band pays strictly between the two extremes.
        ///
        /// The score is *constructed* to land inside rather than drawn at
        /// random: a uniform draw over `i128` misses the band every single
        /// time, so the earlier version of this test never once executed the
        /// division it exists to check.
        #[test]
        fn a_score_inside_the_band_pays_strictly_between(
            strike_units in 1_000u64..1_000_000_000,
            ramp_bps in 10u16..=10_000,
            offset_permille in -999i64..=999,
        ) {
            let strike = price(strike_units);
            let half_width = strike
                .mul(Q64::from_uint(u64::from(ramp_bps))).unwrap()
                .div(Q64::from_uint(10_000)).unwrap();
            let offset = half_width
                .mul(Q64::from_int(offset_permille)).unwrap()
                .div(Q64::from_int(1_000)).unwrap();
            let score = strike.add(offset).unwrap();

            let share = apply_ramp(score, strike, ramp_bps).unwrap();
            prop_assert!(
                share > Q64::ZERO && share < Q64::ONE,
                "score {offset_permille} permille off a strike of {strike_units} \
                 with a {ramp_bps} bp band paid {share:?}"
            );
        }

        /// And whatever it is handed, the answer is still a share.
        #[test]
        fn always_within_the_unit_interval(
            score_raw in any::<i128>(),
            strike_units in 1u64..1_000_000_000,
            ramp_bps in 1u16..=10_000,
        ) {
            let share = apply_ramp(
                Q64::from_raw(score_raw),
                price(strike_units),
                ramp_bps,
            );
            if let Ok(share) = share {
                prop_assert!(share >= Q64::ZERO && share <= Q64::ONE);
            }
        }

        /// A higher price never pays YES less. Without this a manipulator could
        /// profit by pushing the price the "wrong" way.
        #[test]
        fn monotonic_in_the_score(
            strike_units in 1u64..1_000_000,
            ramp_bps in 1u16..=10_000,
            lower_units in 0u64..2_000_000,
            step in 1u64..1_000,
        ) {
            let strike = price(strike_units);
            let low = apply_ramp(price(lower_units), strike, ramp_bps).unwrap();
            let high = apply_ramp(price(lower_units + step), strike, ramp_bps).unwrap();
            prop_assert!(high >= low, "{low:?} -> {high:?}");
        }

        #[test]
        fn never_panics(
            score_raw in any::<i128>(),
            strike_raw in any::<i128>(),
            ramp_bps in any::<u16>(),
        ) {
            let _ = apply_ramp(Q64::from_raw(score_raw), Q64::from_raw(strike_raw), ramp_bps);
        }
    }
}
