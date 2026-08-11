//! The [`Q64`] fixed-point type: a signed 128-bit value scaled by `2^64`.

use crate::u256::U256;
use crate::MathError;

/// Largest `|exp|` accepted by [`Q64::scale_pow10`].
///
/// Token decimals are at most 18 in practice, so a normalisation exponent never
/// exceeds +/-18; 38 leaves room without letting a malformed spec ask for a
/// rescale that cannot possibly fit.
pub const MAX_POW10_EXP: i32 = 38;

/// A signed fixed-point number with 64 fractional bits.
///
/// The represented value is `raw / 2^64`, so the range is roughly
/// `-9.2e18 ..= 9.2e18` with a resolution of `5.4e-20`.
///
/// `Ord` is derived and is correct: the scale is constant, so the ordering of
/// the raw integers is the ordering of the represented values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Q64(i128);

#[allow(clippy::should_implement_trait)]
impl Q64 {
    /// Number of fractional bits.
    pub const FRACTIONAL_BITS: u32 = 64;

    pub const ZERO: Q64 = Q64(0);
    pub const ONE: Q64 = Q64(1i128 << Q64::FRACTIONAL_BITS);
    pub const MIN: Q64 = Q64(i128::MIN);
    pub const MAX: Q64 = Q64(i128::MAX);

    /// Wraps a raw scaled integer. The caller asserts it is already scaled by
    /// `2^64`.
    pub const fn from_raw(raw: i128) -> Self {
        Q64(raw)
    }

    /// The underlying scaled integer.
    pub const fn raw(self) -> i128 {
        self.0
    }

    /// Exact for every `i64`: `i64::MIN << 64 == i128::MIN`.
    pub const fn from_int(value: i64) -> Self {
        Q64((value as i128) << Q64::FRACTIONAL_BITS)
    }

    /// Exact for every `u64`.
    pub const fn from_uint(value: u64) -> Self {
        Q64((value as i128) << Q64::FRACTIONAL_BITS)
    }

    /// The greatest integer `<= self`.
    pub const fn floor_to_int(self) -> i64 {
        // An arithmetic shift right of a negative value already floors.
        (self.0 >> Q64::FRACTIONAL_BITS) as i64
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    // These cannot be `std::ops` impls: every one returns `Result`, which is
    // the whole point -- overflow is an error here, never a wrap or a panic.
    pub fn add(self, rhs: Self) -> Result<Self, MathError> {
        self.0
            .checked_add(rhs.0)
            .map(Q64)
            .ok_or(MathError::Overflow)
    }

    pub fn sub(self, rhs: Self) -> Result<Self, MathError> {
        self.0
            .checked_sub(rhs.0)
            .map(Q64)
            .ok_or(MathError::Overflow)
    }

    pub fn neg(self) -> Result<Self, MathError> {
        self.0.checked_neg().map(Q64).ok_or(MathError::Overflow)
    }

    /// Absolute value. Fails only for [`Q64::MIN`], which has no positive
    /// counterpart.
    pub fn abs(self) -> Result<Self, MathError> {
        self.0.checked_abs().map(Q64).ok_or(MathError::Overflow)
    }

    pub fn min(self, rhs: Self) -> Self {
        if self.0 <= rhs.0 {
            self
        } else {
            rhs
        }
    }

    pub fn max(self, rhs: Self) -> Self {
        if self.0 >= rhs.0 {
            self
        } else {
            rhs
        }
    }

    /// `self * rhs`, rounded toward negative infinity.
    ///
    /// The exact product of two Q64.64 values needs 256 bits before the shift,
    /// so it is computed in [`U256`] and narrowed once, at the end.
    pub fn mul(self, rhs: Self) -> Result<Self, MathError> {
        let negative = self.0.is_negative() != rhs.0.is_negative();
        let product = U256::from(self.0.unsigned_abs())
            .checked_mul(U256::from(rhs.0.unsigned_abs()))
            .ok_or(MathError::Overflow)?;

        let magnitude = product >> Q64::FRACTIONAL_BITS;
        let truncated = product & (U256::pow2(Q64::FRACTIONAL_BITS) - U256::one());
        Self::from_signed_magnitude(magnitude, !truncated.is_zero(), negative)
    }

    /// `self / rhs`, rounded toward negative infinity.
    pub fn div(self, rhs: Self) -> Result<Self, MathError> {
        if rhs.0 == 0 {
            return Err(MathError::DivisionByZero);
        }
        let negative = self.0.is_negative() != rhs.0.is_negative();
        // |self| <= 2^127, so the shifted numerator needs at most 191 bits.
        let numerator = U256::from(self.0.unsigned_abs()) << Q64::FRACTIONAL_BITS;
        let denominator = U256::from(rhs.0.unsigned_abs());

        let magnitude = numerator / denominator;
        let remainder = numerator % denominator;
        Self::from_signed_magnitude(magnitude, !remainder.is_zero(), negative)
    }

    /// Floored modulo: the result carries the sign of `rhs`, matching the
    /// floored division used by [`Q64::div`].
    ///
    /// Working on the raw integers is exact -- both operands share the same
    /// scale, so `(a/S) mod (b/S) == (a mod b)/S`.
    pub fn rem(self, rhs: Self) -> Result<Self, MathError> {
        if rhs.0 == 0 {
            return Err(MathError::DivisionByZero);
        }
        // i128::MIN % -1 overflows in the same way i128::MIN / -1 does.
        let truncated = self.0.checked_rem(rhs.0).ok_or(MathError::Overflow)?;
        let floored = if truncated != 0 && (truncated < 0) != (rhs.0 < 0) {
            truncated.checked_add(rhs.0).ok_or(MathError::Overflow)?
        } else {
            truncated
        };
        Ok(Q64(floored))
    }

    /// `self * 10^exp`, rounded toward negative infinity.
    ///
    /// Used to normalise a raw pool price into "base units per quote unit"
    /// once the two mints' decimals are known.
    pub fn scale_pow10(self, exp: i32) -> Result<Self, MathError> {
        if exp.unsigned_abs() > MAX_POW10_EXP as u32 {
            return Err(MathError::ExponentOutOfRange);
        }
        if exp == 0 || self.0 == 0 {
            return Ok(self);
        }
        // 10^38 < 2^127, so the factor always fits in a u128.
        let factor = 10u128
            .checked_pow(exp.unsigned_abs())
            .ok_or(MathError::Overflow)?;
        let negative = self.0.is_negative();
        let magnitude = U256::from(self.0.unsigned_abs());

        if exp > 0 {
            let scaled = magnitude
                .checked_mul(U256::from(factor))
                .ok_or(MathError::Overflow)?;
            Self::from_signed_magnitude(scaled, false, negative)
        } else {
            let divisor = U256::from(factor);
            let scaled = magnitude / divisor;
            let remainder = magnitude % divisor;
            Self::from_signed_magnitude(scaled, !remainder.is_zero(), negative)
        }
    }

    /// Rebuilds a signed value from an unsigned magnitude, flooring when the
    /// result is negative and exactness was lost.
    ///
    /// This is the single place where floor semantics for negative results are
    /// implemented; every rounding operation above funnels through it.
    fn from_signed_magnitude(
        magnitude: U256,
        lost_precision: bool,
        negative: bool,
    ) -> Result<Self, MathError> {
        let magnitude = magnitude.to_u128().ok_or(MathError::Overflow)?;

        if !negative {
            let value = i128::try_from(magnitude).map_err(|_| MathError::Overflow)?;
            return Ok(Q64(value));
        }

        // Flooring a negative result means rounding away from zero.
        let magnitude = if lost_precision {
            magnitude.checked_add(1).ok_or(MathError::Overflow)?
        } else {
            magnitude
        };

        // i128::MIN has magnitude 2^127, which is not representable as a
        // positive i128, so it needs its own branch.
        const MIN_MAGNITUDE: u128 = 1u128 << 127;
        match magnitude {
            MIN_MAGNITUDE => Ok(Q64(i128::MIN)),
            m if m < MIN_MAGNITUDE => Ok(Q64(-(m as i128))),
            _ => Err(MathError::Overflow),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn q(v: f64) -> Q64 {
        Q64::from_raw((v * 2f64.powi(64)) as i128)
    }

    #[test]
    fn one_is_the_multiplicative_identity() {
        for raw in [0i128, 1, -1, 1 << 64, -(1 << 64), i64::MAX as i128] {
            let v = Q64::from_raw(raw);
            assert_eq!(v.mul(Q64::ONE).unwrap(), v);
            assert_eq!(v.div(Q64::ONE).unwrap(), v);
        }
    }

    #[test]
    fn division_floors_toward_negative_infinity() {
        // -1 / 3 is -0.333..., which floors to the next value *below* it.
        let a = Q64::from_int(-1).div(Q64::from_int(3)).unwrap();
        let b = Q64::from_int(1).div(Q64::from_int(3)).unwrap();
        // Floor of the negative is one ulp more negative than the negated floor
        // of the positive, because the exact value is not representable.
        assert_eq!(a.raw(), -b.raw() - 1);
        assert_eq!(a.floor_to_int(), -1);
        assert_eq!(b.floor_to_int(), 0);
    }

    #[test]
    fn multiplication_floors_toward_negative_infinity() {
        let third = Q64::from_int(1).div(Q64::from_int(3)).unwrap();
        let pos = third.mul(third).unwrap();
        let neg = third.neg().unwrap().mul(third).unwrap();
        assert_eq!(neg.raw(), -pos.raw() - 1);
    }

    #[test]
    fn floored_modulo_carries_the_divisor_sign() {
        let cases = [
            (7i64, 3i64, 1i64),
            (-7, 3, 2),
            (7, -3, -2),
            (-7, -3, -1),
            (6, 3, 0),
            (-6, 3, 0),
        ];
        for (a, b, expected) in cases {
            let got = Q64::from_int(a).rem(Q64::from_int(b)).unwrap();
            assert_eq!(got, Q64::from_int(expected), "{a} mod {b}");
        }
    }

    #[test]
    fn division_by_zero_is_an_error_not_a_panic() {
        assert_eq!(Q64::ONE.div(Q64::ZERO), Err(MathError::DivisionByZero));
        assert_eq!(Q64::ONE.rem(Q64::ZERO), Err(MathError::DivisionByZero));
    }

    #[test]
    fn extremes_do_not_panic() {
        assert_eq!(Q64::MIN.abs(), Err(MathError::Overflow));
        assert_eq!(Q64::MIN.neg(), Err(MathError::Overflow));
        assert_eq!(Q64::MAX.add(Q64::ONE), Err(MathError::Overflow));
        // `from_int(-1)` is -2^64, which divides i128::MIN exactly; only a raw
        // -1 reproduces the i128::MIN % -1 overflow.
        assert_eq!(Q64::MIN.rem(Q64::from_int(-1)), Ok(Q64::ZERO));
        assert_eq!(Q64::MIN.rem(Q64::from_raw(-1)), Err(MathError::Overflow));
        assert!(Q64::MAX.mul(Q64::MAX).is_err());
    }

    #[test]
    fn scale_pow10_round_trips_for_exact_powers() {
        let v = Q64::from_int(1_234);
        assert_eq!(v.scale_pow10(6).unwrap(), Q64::from_int(1_234_000_000));
        assert_eq!(
            v.scale_pow10(6).unwrap().scale_pow10(-6).unwrap(),
            Q64::from_int(1_234)
        );
        assert_eq!(
            Q64::ONE.scale_pow10(MAX_POW10_EXP + 1),
            Err(MathError::ExponentOutOfRange)
        );
    }

    proptest! {
        /// Nothing in this crate may ever panic, whatever the inputs.
        #[test]
        fn never_panics(a in any::<i128>(), b in any::<i128>(), exp in -40i32..=40) {
            let (a, b) = (Q64::from_raw(a), Q64::from_raw(b));
            let _ = a.add(b);
            let _ = a.sub(b);
            let _ = a.mul(b);
            let _ = a.div(b);
            let _ = a.rem(b);
            let _ = a.abs();
            let _ = a.neg();
            let _ = a.scale_pow10(exp);
            let _ = a.min(b);
            let _ = a.max(b);
        }

        /// `(a * b) / b == a` up to the one ulp that flooring may shave off.
        #[test]
        fn mul_div_round_trips(
            a in -(1i128 << 100)..(1i128 << 100),
            b in 1i128..(1i128 << 80),
        ) {
            let (a, b) = (Q64::from_raw(a), Q64::from_raw(b));
            if let Ok(product) = a.mul(b) {
                let back = product.div(b).unwrap();
                let delta = a.raw() - back.raw();
                prop_assert!((0..=2).contains(&delta), "a={a:?} b={b:?} back={back:?}");
            }
        }

        /// Division agrees with the definition of floor: `q*b <= a < (q+1)*b`
        /// for positive divisors.
        #[test]
        fn div_matches_floor_definition(
            a in -(1i128 << 96)..(1i128 << 96),
            b in 1i128..(1i128 << 96),
        ) {
            let (a, b) = (Q64::from_raw(a), Q64::from_raw(b));
            let quotient = a.div(b).unwrap();
            let lower = quotient.mul(b).unwrap();
            let upper = quotient.add(Q64::from_raw(1)).unwrap().mul(b).unwrap();
            prop_assert!(lower <= a, "lower bound violated");
            prop_assert!(a < upper || upper <= lower, "upper bound violated");
        }

        /// `a == b * (a / b) + (a mod b)` -- the division/modulo identity, on
        /// raw integers where it must hold exactly.
        #[test]
        fn rem_completes_the_division_identity(a in any::<i128>(), b in any::<i128>()) {
            prop_assume!(b != 0);
            prop_assume!(!(a == i128::MIN && b == -1));
            let r = Q64::from_raw(a).rem(Q64::from_raw(b)).unwrap().raw();
            prop_assert!(r == 0 || (r < 0) == (b < 0), "sign of remainder");
            prop_assert!(r.unsigned_abs() < b.unsigned_abs(), "magnitude of remainder");
        }

        /// Ordering of raw integers is ordering of represented values.
        #[test]
        fn ordering_is_consistent_with_addition(a in -(1i128 << 120)..(1i128 << 120)) {
            let v = Q64::from_raw(a);
            prop_assert!(v < v.add(Q64::from_raw(1)).unwrap());
        }
    }

    #[test]
    fn from_int_matches_manual_scaling() {
        assert_eq!(Q64::from_int(0), Q64::ZERO);
        assert_eq!(Q64::from_int(1), Q64::ONE);
        assert_eq!(Q64::from_int(i64::MIN), Q64::MIN);
        assert_eq!(q(2.5).floor_to_int(), 2);
        assert_eq!(q(-2.5).floor_to_int(), -3);
    }
}
