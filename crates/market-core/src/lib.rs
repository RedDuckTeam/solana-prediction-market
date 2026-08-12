//! Market rules and payout arithmetic: pure functions over plain numbers, so
//! the part that decides money can be tested exhaustively rather than through a
//! validator.
//!
//! The invariant, held by property tests over the whole input space: the
//! protocol never owes more than it holds. Every division floors toward the
//! vault, and the residue is swept long after claiming closes.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

mod payout;
mod ramp;
mod schedule;
mod state;

pub use payout::{payout_for, settle, Settlement, Side, Stakes};
pub use ramp::{apply_ramp, validate_ramp};
pub use schedule::{Schedule, MAX_HISTORY_BUDGET};
pub use state::{MarketState, Outcome, VoidReason};

/// Basis points denominator.
pub const BPS_DENOMINATOR: u64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreError {
    /// A share fell outside `[0, 1]`, or a strike was not positive.
    ///
    /// The predicate machine returns a raw score, not a share -- turning one
    /// into the other is [`apply_ramp`]'s job -- so this is a real possibility
    /// rather than an impossible state, and it is checked accordingly.
    ShareOutOfRange,
    /// Fee rate above 100%.
    FeeRateOutOfRange,
    /// An intermediate exceeded its type.
    Overflow,
    /// A side holds no stake, so there is nothing to divide by.
    EmptySide,
    /// Burning more than the side ever held.
    BurnExceedsStake,
    /// The deposit would push a side past its cap.
    CapExceeded,
    /// Averaging window plus grace period exceeds what the observation ring
    /// can guarantee.
    ScheduleTooLong,
    /// A schedule field is zero or otherwise nonsensical.
    ScheduleInvalid,
}
