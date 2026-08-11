//! 256-bit unsigned integer used only as an intermediate.
//!
//! Q64.64 multiplication needs 256 bits before the shift, and inverting a
//! Q128.128 value needs a 192-by-128-bit division. Both are easy to get subtly
//! wrong by hand, so we use `uint`'s macro-generated implementation, which is
//! the same one the SPL math library builds on.

// Fires inside the macro expansion, not on anything written here.
#![allow(clippy::manual_div_ceil)]

use uint::construct_uint;

construct_uint! {
    pub struct U256(4);
}

impl U256 {
    /// `2^n` for `n < 256`. Panics are impossible for the call sites in this
    /// crate, which all pass compile-time constants below 256.
    pub(crate) fn pow2(n: u32) -> U256 {
        debug_assert!(n < 256);
        U256::one() << n
    }

    /// Narrows to `u128`, or `None` if the value does not fit.
    pub(crate) fn to_u128(self) -> Option<u128> {
        if self > U256::from(u128::MAX) {
            None
        } else {
            Some(self.low_u128())
        }
    }
}
