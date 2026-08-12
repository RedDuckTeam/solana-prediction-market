//! The market lifecycle, as a table rather than as scattered `require!`s.
//!
//! Keeping the transitions in one place means the illegal ones can be
//! enumerated in a test instead of hoped about, and the void paths -- the ones
//! that usually go untested and therefore usually break -- get the same
//! treatment as the happy path.

use market_math::Q64;

use crate::CoreError;

/// Where a market is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketState {
    /// Created, inside the cooldown that lets anyone read the spec before
    /// money can enter it.
    Created,
    /// Accepting deposits.
    Open,
    /// Past the betting deadline, awaiting a snapshot.
    Locked,
    /// Prices fixed; the predicate has not run yet.
    Snapshotted,
    /// Settled. Holders may claim.
    Resolved,
    /// Abandoned. Everyone is refunded at par and no fee is charged.
    Void,
}

impl MarketState {
    pub fn is_final(self) -> bool {
        matches!(self, MarketState::Resolved | MarketState::Void)
    }

    /// Applies `event`, or explains why it does not apply.
    ///
    /// Timing is the caller's business; this only encodes which orderings make
    /// sense at all.
    pub fn apply(self, event: Event) -> Result<MarketState, CoreError> {
        use Event::*;
        use MarketState::*;
        Ok(match (self, event) {
            (Created, CooldownElapsed) => Open,
            (Open, Locked_) => Locked,
            (Locked, SnapshotTaken) => Snapshotted,
            (Snapshotted, Resolved_) => Resolved,

            // A market can be abandoned from anywhere it has not already
            // settled, including straight from Open when the betting deadline
            // passes with one side empty.
            (Created | Open | Locked | Snapshotted, Voided(_)) => Void,

            _ => return Err(CoreError::ScheduleInvalid),
        })
    }
}

/// Something that happened to a market.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// The post-creation cooldown expired.
    CooldownElapsed,
    /// The betting deadline passed.
    Locked_,
    /// Feed prices were read and frozen.
    SnapshotTaken,
    /// The predicate ran and the pot was split.
    Resolved_,
    /// The market was abandoned.
    Voided(VoidReason),
}

/// Why a market was abandoned.
///
/// Every reason except a missing crank is decided by chain state at the
/// settlement instant, so it cannot be brought about after the outcome is
/// known. A missing crank is defeated by any honest participant, since both
/// cranks are permissionless and funded from the creator's bond.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoidReason {
    /// Nobody took a snapshot before the grace period ended.
    SnapshotMissed,
    /// One side attracted no stake, so there was no counterparty.
    EmptySide,
    /// A declared feed could not be read at settlement.
    FeedInvalid,
    /// The predicate aborted: overflow, division by zero, or a step limit.
    ///
    /// A faulty predicate refunds everyone. It must never be payable to a side,
    /// or writing a predicate that aborts on the branch you dislike becomes a
    /// strategy.
    PredicateAborted,
}

/// How a market ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The fraction of the pot owed to YES, in `[0, 1]`.
    Share(Q64),
    Void(VoidReason),
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_STATES: [MarketState; 6] = [
        MarketState::Created,
        MarketState::Open,
        MarketState::Locked,
        MarketState::Snapshotted,
        MarketState::Resolved,
        MarketState::Void,
    ];

    const ALL_EVENTS: [Event; 5] = [
        Event::CooldownElapsed,
        Event::Locked_,
        Event::SnapshotTaken,
        Event::Resolved_,
        Event::Voided(VoidReason::SnapshotMissed),
    ];

    #[test]
    fn the_happy_path_runs_end_to_end() {
        let final_state = [
            Event::CooldownElapsed,
            Event::Locked_,
            Event::SnapshotTaken,
            Event::Resolved_,
        ]
        .into_iter()
        .try_fold(MarketState::Created, MarketState::apply)
        .expect("the happy path is legal");
        assert_eq!(final_state, MarketState::Resolved);
    }

    #[test]
    fn every_void_reason_can_actually_be_reached() {
        let reachable = [
            (MarketState::Locked, VoidReason::SnapshotMissed),
            (MarketState::Open, VoidReason::EmptySide),
            (MarketState::Locked, VoidReason::FeedInvalid),
            (MarketState::Snapshotted, VoidReason::PredicateAborted),
        ];
        for (from, reason) in reachable {
            assert_eq!(from.apply(Event::Voided(reason)), Ok(MarketState::Void));
        }
    }

    #[test]
    fn settled_markets_never_move_again() {
        for state in [MarketState::Resolved, MarketState::Void] {
            assert!(state.is_final());
            for event in ALL_EVENTS {
                assert_eq!(
                    state.apply(event),
                    Err(CoreError::ScheduleInvalid),
                    "{state:?} should be immovable under {event:?}"
                );
            }
        }
    }

    #[test]
    fn no_phase_can_be_skipped() {
        // Resolving without a snapshot would mean resolving against live pool
        // state, which is the whole thing the snapshot/resolve split forbids.
        assert!(MarketState::Locked.apply(Event::Resolved_).is_err());
        assert!(MarketState::Open.apply(Event::SnapshotTaken).is_err());
        assert!(MarketState::Created.apply(Event::Locked_).is_err());
    }

    #[test]
    fn the_transition_table_is_exactly_as_large_as_it_looks() {
        // Eight legal moves over these events: four along the happy path, plus
        // one abandonment from each of the four unsettled states. Counting them
        // keeps an accidental widening of the table from passing review.
        let legal = ALL_STATES
            .iter()
            .flat_map(|state| ALL_EVENTS.iter().map(move |event| state.apply(*event)))
            .filter(Result::is_ok)
            .count();
        assert_eq!(legal, 8);
    }

    #[test]
    fn the_reason_for_abandoning_never_changes_whether_it_is_allowed() {
        // `ALL_EVENTS` carries one `Voided` variant, so the count above would
        // not notice a rule that admitted some reasons and not others.
        let reasons = [
            VoidReason::SnapshotMissed,
            VoidReason::EmptySide,
            VoidReason::FeedInvalid,
            VoidReason::PredicateAborted,
        ];
        for reason in reasons {
            for state in ALL_STATES {
                let expected = if state.is_final() {
                    Err(CoreError::ScheduleInvalid)
                } else {
                    Ok(MarketState::Void)
                };
                assert_eq!(
                    state.apply(Event::Voided(reason)),
                    expected,
                    "{state:?} {reason:?}"
                );
            }
        }
    }
}
