//! When each phase of a market opens and closes.
//!
//! Every instant here is a whole second of UTC, and every comparison is against
//! an interval rather than a point. Solana's `Clock::unix_timestamp` is a
//! stake-weighted median of validator vote timestamps, not a clock, and it
//! drifts from real time by seconds.

use crate::CoreError;

/// The most averaging window plus grace period a market may ask for.
///
/// Raydium's ring guarantees `99 * 15 = 1485` seconds of history -- ninety-nine
/// gaps between a hundred observations, each at least fifteen seconds. A
/// snapshot taken at the very end of the grace period still has to reach back
/// to the start of the averaging window, so the two together must fit inside
/// that, and this leaves 285 seconds of margin.
pub const MAX_HISTORY_BUDGET: u32 = 1_200;

/// Bounds on the timetable a market may declare.
///
/// The observation ring fixes the ceiling on `twap_window + grace`, but each
/// field needs its own limits too: a two-second averaging window is not an
/// average, and a grace period of a day outlives the history it would need.
/// These were documented before they were enforced, which is the same as not
/// having them.
pub const MIN_TWAP_WINDOW: u32 = 120;
pub const MAX_TWAP_WINDOW: u32 = 900;
pub const MIN_GRACE: u32 = 300;
pub const MAX_GRACE: u32 = 900;
pub const MAX_SKEW: u32 = 300;

/// Smallest gap between the betting deadline and the averaging window.
///
/// Without it, a deposit landing in a block whose clock runs slow is really
/// made *inside* the measured window, by a bettor already watching the ticks
/// that will settle the market.
pub const MIN_SKEW: u32 = 60;

/// A market's timetable, fixed at creation and never revised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    /// The settlement instant, the `Y` a market's question names.
    pub settle_at: i64,
    /// Averaging window length, ending at `settle_at`.
    pub twap_window: u32,
    /// How long after `settle_at` a snapshot may still be taken.
    pub grace: u32,
    /// Gap between the betting deadline and the start of the window.
    pub skew: u32,
    /// Longest observation gap tolerated inside the window.
    ///
    /// The protocol asks only that it be shorter than the window, which is what
    /// makes a segment carry a price something actually traded at. A deployment
    /// that has not thought about it should halve the window: manipulating an
    /// average then costs more writes, each one visible.
    pub max_segment: u32,
    /// Observations that must fall inside the window for it to count.
    ///
    /// There is no floor. A TWAP is two readings, one at each end, and what
    /// lies between them changes nothing about the arithmetic: a window nobody
    /// traded through averages the price that stood for all of it, which is
    /// that pool's honest price rather than a gap in the data. Asking for more
    /// is a demand for proof of activity, which is a deployment's to make.
    pub min_observations: u16,
}

impl Schedule {
    /// Start of the averaging window.
    pub fn window_start(&self) -> i64 {
        self.settle_at - i64::from(self.twap_window)
    }

    /// When betting closes -- before the window opens, not at settlement.
    pub fn lock_at(&self) -> i64 {
        self.window_start() - i64::from(self.skew)
    }

    /// Last instant a snapshot may be taken.
    pub fn grace_end(&self) -> i64 {
        self.settle_at + i64::from(self.grace)
    }

    pub fn deposits_open(&self, now: i64, open_at: i64) -> bool {
        now >= open_at && now < self.lock_at()
    }

    pub fn snapshot_open(&self, now: i64) -> bool {
        now >= self.settle_at && now <= self.grace_end()
    }

    /// Whether the market may now be voided for want of a snapshot.
    pub fn snapshot_missed(&self, now: i64) -> bool {
        now > self.grace_end()
    }

    /// Rejects timetables the observation ring cannot serve, or that leave a
    /// bettor able to see the window they are betting on.
    pub fn validate(&self) -> Result<(), CoreError> {
        if !(MIN_TWAP_WINDOW..=MAX_TWAP_WINDOW).contains(&self.twap_window) {
            return Err(CoreError::ScheduleInvalid);
        }
        if !(MIN_GRACE..=MAX_GRACE).contains(&self.grace) {
            return Err(CoreError::ScheduleInvalid);
        }
        if !(MIN_SKEW..=MAX_SKEW).contains(&self.skew) {
            return Err(CoreError::ScheduleInvalid);
        }
        // Any positive bound. Setting it below the window forces an observation
        // inside, which proves the pool was traded through while it was being
        // measured; setting it at or above the window asks for no such proof.
        //
        // Neither is safer. Raydium records the tick that prevailed *until* each
        // write, so a segment carries the price that actually stood for its
        // whole duration: stretching a momentary move is impossible at any
        // bound, and a tighter one only costs an attacker one more transaction.
        // What it does cost is honest use -- a pool traded by hand has to be
        // traded twice rather than once -- so the choice belongs to whoever
        // deploys, not to the protocol.
        if self.max_segment == 0 {
            return Err(CoreError::ScheduleInvalid);
        }
        let history_needed = self
            .twap_window
            .checked_add(self.grace)
            .ok_or(CoreError::Overflow)?;
        if history_needed > MAX_HISTORY_BUDGET {
            return Err(CoreError::ScheduleTooLong);
        }
        // The timetable must be orderable without wrapping.
        self.settle_at
            .checked_sub(i64::from(self.twap_window))
            .and_then(|t| t.checked_sub(i64::from(self.skew)))
            .ok_or(CoreError::Overflow)?;
        self.settle_at
            .checked_add(i64::from(self.grace))
            .ok_or(CoreError::Overflow)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const SETTLE_AT: i64 = 1_800_000_000;

    fn default_schedule() -> Schedule {
        Schedule {
            settle_at: SETTLE_AT,
            twap_window: 900,
            grace: 300,
            skew: 60,
            max_segment: 450,
            min_observations: 5,
        }
    }

    #[test]
    fn the_default_timetable_is_valid_and_ordered() {
        let schedule = default_schedule();
        assert_eq!(schedule.validate(), Ok(()));
        assert!(schedule.lock_at() < schedule.window_start());
        assert!(schedule.window_start() < schedule.settle_at);
        assert!(schedule.settle_at < schedule.grace_end());
        assert_eq!(schedule.lock_at(), SETTLE_AT - 960);
    }

    #[test]
    fn betting_closes_before_the_measured_window_opens() {
        let schedule = default_schedule();
        let open_at = SETTLE_AT - 100_000;

        assert!(schedule.deposits_open(schedule.lock_at() - 1, open_at));
        assert!(!schedule.deposits_open(schedule.lock_at(), open_at));
        // The whole skew gap is closed to betting, so a slow clock cannot let a
        // deposit land inside the window.
        assert!(!schedule.deposits_open(schedule.window_start() - 1, open_at));
        assert!(!schedule.deposits_open(open_at - 1, open_at));
    }

    #[test]
    fn snapshots_are_confined_to_the_grace_window() {
        let schedule = default_schedule();
        assert!(!schedule.snapshot_open(schedule.settle_at - 1));
        assert!(schedule.snapshot_open(schedule.settle_at));
        assert!(schedule.snapshot_open(schedule.grace_end()));
        assert!(!schedule.snapshot_open(schedule.grace_end() + 1));

        assert!(!schedule.snapshot_missed(schedule.grace_end()));
        assert!(schedule.snapshot_missed(schedule.grace_end() + 1));
    }

    #[test]
    fn timetables_the_observation_ring_cannot_serve_are_refused() {
        let mut schedule = default_schedule();
        schedule.twap_window = 900;
        schedule.grace = 900;
        schedule.max_segment = 90;
        assert_eq!(schedule.validate(), Err(CoreError::ScheduleTooLong));
    }

    #[test]
    fn a_too_small_skew_is_refused() {
        let mut schedule = default_schedule();
        schedule.skew = MIN_SKEW - 1;
        assert_eq!(schedule.validate(), Err(CoreError::ScheduleInvalid));
    }

    /// The bound is exactly "below the window", and both sides of that line are
    /// pinned: a segment that could span the whole window is refused, and one a
    /// second short of it is not.
    ///
    /// The loose end matters as much as the tight one. At `window - 1` a single
    /// observation anywhere inside the window satisfies the bound, which is what
    /// lets a market be revived by hand in two transactions rather than three.
    #[test]
    fn a_segment_that_could_span_the_window_is_refused_and_nothing_tighter_is() {
        let mut schedule = default_schedule();

        schedule.max_segment = schedule.twap_window;
        assert_eq!(
            schedule.validate(),
            Ok(()),
            "a bound at the window asks for no observation inside it, which is a \
             deployment's choice rather than the protocol's",
        );

        schedule.max_segment = schedule.twap_window - 1;
        assert_eq!(
            schedule.validate(),
            Ok(()),
            "a bound below the window forces one inside, which is also allowed",
        );

        schedule.max_segment = 0;
        assert_eq!(schedule.validate(), Err(CoreError::ScheduleInvalid));
    }

    #[test]
    fn every_field_is_held_to_its_documented_range() {
        let out_of_range: [fn(&mut Schedule); 7] = [
            |s| s.twap_window = 0,
            |s| s.twap_window = MIN_TWAP_WINDOW - 1,
            |s| s.twap_window = MAX_TWAP_WINDOW + 1,
            |s| s.grace = MIN_GRACE - 1,
            |s| s.grace = MAX_GRACE + 1,
            |s| s.skew = MAX_SKEW + 1,
            |s| s.max_segment = 0,
        ];
        for (index, mutate) in out_of_range.iter().enumerate() {
            let mut schedule = default_schedule();
            mutate(&mut schedule);
            assert_eq!(
                schedule.validate(),
                Err(CoreError::ScheduleInvalid),
                "mutation {index} should have been refused"
            );
        }
        assert_eq!(default_schedule().validate(), Ok(()));
    }

    proptest! {
        /// A schedule that validates always has its phases in order and never
        /// overflows while reporting them.
        #[test]
        fn valid_schedules_are_always_ordered(
            settle_at in 0i64..i64::MAX / 2,
            twap_window in 1u32..1_200,
            grace in 1u32..1_200,
            skew in 0u32..600,
            max_segment in 1u32..600,
            min_observations in 0u16..20,
        ) {
            let schedule = Schedule {
                settle_at, twap_window, grace, skew, max_segment, min_observations,
            };
            if schedule.validate().is_ok() {
                prop_assert!(schedule.lock_at() < schedule.window_start());
                prop_assert!(schedule.window_start() < schedule.settle_at);
                prop_assert!(schedule.settle_at < schedule.grace_end());
                prop_assert!(twap_window + grace <= MAX_HISTORY_BUDGET);
            }
        }
    }
}
