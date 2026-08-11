//! Price derivation from Raydium CLMM observation rings and Pyth TWAP accounts.
//!
//! Pure functions of bytes the caller names in advance: no clock, no live pool
//! state. Slices rather than `AccountInfo`, so the program, the native tests and
//! the wasm build run the same code.
//!
//! Raydium records the tick *before* the swap that triggers the write, so a
//! segment is credited with the tick that opened it for its whole duration —
//! which is why over-long segments are rejected rather than averaged.
//!
//! Extrapolation is absent and must stay absent: extending the cumulative to the
//! settlement instant would make the answer depend on when settlement ran.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

mod pool;
mod price;
mod pyth;
mod raydium;

#[cfg(test)]
mod tests;

pub use pool::{read_pool_state, write_pool_state, PoolInfo, POOL_STATE_LEN};
pub use price::{invert_price, normalized_price};
pub use pyth::{pyth_twap, PythLimits, PythReading, TWAP_UPDATE_LEN};
pub use raydium::{
    raydium_twap, Boundary, ClmmLimits, TwapReading, GUARANTEED_HISTORY, OBSERVATION_NUM,
    OBSERVATION_STATE_LEN, OBSERVATION_UPDATE_DURATION,
};

use market_math::MathError;

/// Why a feed could not be read.
///
/// Every variant means the feed is unusable now, and a market that cannot read
/// every declared feed voids. Guessing a price is never an option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedError {
    AccountTooSmall,
    WrongAccountType,
    NotInitialized,
    PoolMismatch,
    IndexOutOfRange,
    /// A partly-filled ring. There is no length field, and a zero slot reads as
    /// timestamp 0 at price 1.0, so only full buffers are accepted.
    BufferNotFull,
    NonMonotonic,
    /// The window is not bracketed by real observations. Interpolating within a
    /// segment is exact; extending past the last one is not.
    WindowNotCovered,
    SegmentTooLong,
    /// Too few observations inside the window: the pool was not meaningfully
    /// traded during the period being measured.
    WindowTooQuiet,
    /// A segment's cumulative delta is not a multiple of its duration. Raydium's
    /// arithmetic cannot produce this, so it means the layout changed under us.
    InconsistentCumulative,
    EmptyWindow,
    TickOutOfRange,
    /// A price outside `[2^-40, 2^56]`. Below the floor an ulp is a whole
    /// payout step; above the ceiling composed feeds fall off the type.
    PriceOutOfRange,
    FeedIdMismatch,
    /// The account averages a window other than the one requested. Whoever
    /// cranks creates it, and signed history runs far back, so an unpinned
    /// window would be chosen for profit.
    WindowMismatch,
    TooManySlotsMissed,
    ConfidenceTooWide,
    NonPositivePrice,
    Math(MathError),
}

impl From<MathError> for FeedError {
    fn from(error: MathError) -> Self {
        match error {
            MathError::TickOutOfRange => FeedError::TickOutOfRange,
            other => FeedError::Math(other),
        }
    }
}
