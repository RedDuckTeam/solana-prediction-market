//! Byte-level tests against a faithful reimplementation of Raydium's writer.
//!
//! The fixtures are built by replaying `ObservationState::update()` exactly as
//! Raydium implements it, then serialising the struct at the offsets the parser
//! reads. That makes these real tests of the wire format, not tests of a Rust
//! struct we control both ends of.

use proptest::prelude::*;
use sha2::Digest;
use std::vec;
use std::vec::Vec;

use crate::raydium::{
    raydium_twap, ClmmLimits, OBSERVATION_NUM, OBSERVATION_STATE_LEN, OBSERVATION_UPDATE_DURATION,
};
use crate::FeedError;

const POOL: [u8; 32] = [7u8; 32];
const DISCRIMINATOR: [u8; 8] = [0x7a, 0xae, 0xc5, 0x35, 0x81, 0x09, 0xa5, 0x84];
const OFF_OBSERVATIONS: usize = 51;
const OBSERVATION_LEN: usize = 44;
const FIRST_TIMESTAMP: u32 = 1_800_000_000;

/// A synthetic `ObservationState`, written the way Raydium writes one.
struct Fixture {
    timestamps: Vec<u32>,
    cumulatives: Vec<i64>,
    /// Ring slot that holds the newest entry.
    newest_slot: usize,
    pool: [u8; 32],
    initialized: bool,
}

impl Fixture {
    /// Builds a full ring from `segments` of `(duration, tick)`.
    ///
    /// Mirrors `update()`: the cumulative advances by `tick * delta_time`, and
    /// the tick recorded for a segment is the one that stood during it.
    fn from_segments(segments: &[(u32, i32)]) -> Self {
        let mut timestamps = vec![FIRST_TIMESTAMP];
        let mut cumulatives = vec![0i64];
        for (duration, tick) in segments {
            let previous_timestamp = *timestamps.last().expect("seeded above");
            let previous_cumulative = *cumulatives.last().expect("seeded above");
            timestamps.push(previous_timestamp + duration);
            cumulatives
                .push(previous_cumulative.wrapping_add(i64::from(*tick) * i64::from(*duration)));
        }
        Fixture {
            newest_slot: timestamps.len() - 1,
            timestamps,
            cumulatives,
            pool: POOL,
            initialized: true,
        }
    }

    /// A full ring holding one constant tick throughout.
    fn constant(tick: i32) -> Self {
        Self::from_segments(&vec![
            (OBSERVATION_UPDATE_DURATION, tick);
            OBSERVATION_NUM - 1
        ])
    }

    /// Rotates which slot holds the newest entry, exercising ring wrap-around.
    fn rotated(mut self, newest_slot: usize) -> Self {
        self.newest_slot = newest_slot % OBSERVATION_NUM;
        self
    }

    fn encode(&self) -> Vec<u8> {
        let mut data = vec![0u8; OBSERVATION_STATE_LEN];
        data[..8].copy_from_slice(&DISCRIMINATOR);
        data[8] = u8::from(self.initialized);
        data[9..17].copy_from_slice(&7u64.to_le_bytes()); // recent_epoch, unread
        data[17..19].copy_from_slice(&(self.newest_slot as u16).to_le_bytes());
        data[19..51].copy_from_slice(&self.pool);

        // Chronological entry `i` lands `count - 1 - i` slots before the newest.
        let count = self.timestamps.len();
        for position in 0..count {
            let distance_from_newest = count - 1 - position;
            let slot =
                (self.newest_slot + OBSERVATION_NUM - distance_from_newest) % OBSERVATION_NUM;
            let base = OFF_OBSERVATIONS + slot * OBSERVATION_LEN;
            data[base..base + 4].copy_from_slice(&self.timestamps[position].to_le_bytes());
            data[base + 4..base + 12].copy_from_slice(&self.cumulatives[position].to_le_bytes());
        }
        data
    }

    fn window(&self) -> (i64, i64) {
        (
            i64::from(self.timestamps[0]),
            i64::from(*self.timestamps.last().expect("non-empty")),
        )
    }
}

fn limits(max_segment: u32) -> ClmmLimits {
    ClmmLimits {
        max_segment,
        // Most tests are not about liveness, so they ask for none; the ones
        // that are say so explicitly.
        min_observations: 0,
    }
}

fn read(data: &[u8], from: i64, to: i64, max_segment: u32) -> Result<i32, FeedError> {
    raydium_twap(data, &POOL, from, to, limits(max_segment)).map(|reading| reading.average_tick)
}

#[test]
fn discriminator_matches_anchors_derivation() {
    let expected: [u8; 8] = sha2::Sha256::digest(b"account:ObservationState")[..8]
        .try_into()
        .expect("eight bytes");
    assert_eq!(DISCRIMINATOR, expected);
}

#[test]
fn constant_tick_averages_to_itself() {
    for tick in [0i32, 1, -1, 4_096, -16_096, 200_000, -200_000] {
        let fixture = Fixture::constant(tick);
        let (from, to) = fixture.window();
        assert_eq!(
            read(&fixture.encode(), from, to, 30),
            Ok(tick),
            "tick {tick}"
        );
    }
}

#[test]
fn average_is_weighted_by_time_not_by_observation_count() {
    // Ninety seconds at tick 100, then nine hundred at tick 0. A naive mean of
    // the two ticks would say 50; the time-weighted answer is 9.
    let mut segments = vec![(15u32, 100i32); 6];
    segments.extend(vec![(15u32, 0i32); OBSERVATION_NUM - 7]);
    let fixture = Fixture::from_segments(&segments);
    let (from, to) = fixture.window();

    let total = to - from;
    let expected = (100 * 90) / total as i32;
    assert_eq!(read(&fixture.encode(), from, to, 30), Ok(expected));
}

#[test]
fn interpolation_inside_a_segment_is_exact() {
    let fixture = Fixture::constant(-16_096);
    let (from, to) = fixture.window();
    // Both boundaries land strictly inside segments, seven seconds off the
    // recorded observations. The answer must not shift at all.
    assert_eq!(read(&fixture.encode(), from + 7, to - 7, 30), Ok(-16_096));
}

#[test]
fn average_tick_floors_for_negative_values() {
    // 15 s at tick -100, the rest at -101: the exact mean is not an integer,
    // and truncating toward zero would round it the wrong way.
    let mut segments = vec![(15u32, -100i32)];
    segments.extend(vec![(15u32, -101i32); OBSERVATION_NUM - 2]);
    let fixture = Fixture::from_segments(&segments);
    let (from, to) = fixture.window();

    let total = (to - from) as i128;
    let exact = (-100i128 * 15) + (-101i128 * (total - 15));
    let floored = (exact.div_euclid(total)) as i32;
    assert!(exact % total != 0, "test needs an inexact average");
    assert_eq!(read(&fixture.encode(), from, to, 30), Ok(floored));
    assert_eq!(
        floored,
        (exact / total) as i32 - 1,
        "floor differs from trunc"
    );
}

#[test]
fn ring_position_does_not_change_the_answer() {
    let fixture = Fixture::constant(-16_096);
    let (from, to) = fixture.window();
    let baseline = read(&fixture.encode(), from, to, 30);
    for newest_slot in [0usize, 1, 37, 98, 99] {
        let rotated = Fixture::constant(-16_096).rotated(newest_slot);
        assert_eq!(
            read(&rotated.encode(), from, to, 30),
            baseline,
            "slot {newest_slot}"
        );
    }
}

#[test]
fn window_must_be_bracketed_on_both_sides() {
    let fixture = Fixture::constant(50);
    let (from, to) = fixture.window();
    let data = fixture.encode();

    // Asking past the newest observation would require extrapolation.
    assert_eq!(
        read(&data, from, to + 1, 30),
        Err(FeedError::WindowNotCovered)
    );
    // Asking before the oldest one would require history that is gone.
    assert_eq!(
        read(&data, from - 1, to, 30),
        Err(FeedError::WindowNotCovered)
    );
    // Exactly on the boundaries is fine.
    assert!(read(&data, from, to, 30).is_ok());
}

#[test]
fn over_long_segments_are_rejected() {
    // One 600-second gap in the middle: a price held for an instant before the
    // closing swap would be credited with all ten minutes.
    let mut segments = vec![(15u32, 10i32); 40];
    segments.push((600, 10));
    segments.extend(vec![(15u32, 10i32); OBSERVATION_NUM - 42]);
    let fixture = Fixture::from_segments(&segments);
    let (from, to) = fixture.window();

    assert_eq!(
        read(&fixture.encode(), from, to, 30),
        Err(FeedError::SegmentTooLong)
    );
    // A window that stops before the gap is still readable.
    let before_gap = i64::from(fixture.timestamps[40]);
    assert!(read(&fixture.encode(), from, before_gap, 30).is_ok());
}

#[test]
fn only_the_overlap_with_the_window_counts_toward_the_segment_limit() {
    // A long segment that the window barely touches contributes only its
    // overlap, which is what actually weights the average.
    let mut segments = vec![(600u32, 10i32)];
    segments.extend(vec![(15u32, 10i32); OBSERVATION_NUM - 2]);
    let fixture = Fixture::from_segments(&segments);
    let data = fixture.encode();
    let (_, to) = fixture.window();

    let long_segment_end = i64::from(fixture.timestamps[1]);
    assert!(read(&data, long_segment_end - 20, to, 30).is_ok());
    assert_eq!(
        read(&data, long_segment_end - 40, to, 30),
        Err(FeedError::SegmentTooLong)
    );
}

/// A ring that has not wrapped yet, holding `count` observations.
fn partly_filled(tick: i32, count: usize) -> Fixture {
    let mut fixture = Fixture::constant(tick);
    fixture.timestamps.truncate(count);
    fixture.cumulatives.truncate(count);
    fixture.newest_slot = count - 1;
    fixture
}

/// A pool opened this morning is a real pool.
///
/// The reader used to refuse anything short of a hundred observations, which
/// meant a market could not settle against a pool until a hundred swaps had
/// gone through it -- every pool on its first day, and every pool anyone has to
/// trade by hand. Whether the ring has wrapped is readable rather than guessed:
/// the slot past the newest is either the oldest entry or has never been
/// written.
#[test]
fn a_ring_that_has_not_wrapped_yet_is_read_from_what_it_holds() {
    let fixture = partly_filled(10, 5);
    let (from, to) = fixture.window();
    assert_eq!(read(&fixture.encode(), from, to, 30), Ok(10));
}

#[test]
fn two_observations_are_the_fewest_an_average_can_be_taken_between() {
    let (from, to) = partly_filled(10, 2).window();
    assert_eq!(read(&partly_filled(10, 2).encode(), from, to, 30), Ok(10));

    let one = partly_filled(10, 1);
    let window = one.window();
    assert_eq!(
        read(&one.encode(), window.0, window.1 + 1, 30),
        Err(FeedError::BufferNotFull),
    );
}

/// The unwritten tail must never be read as data.
///
/// This is what the old blanket refusal was really protecting. A zero slot has
/// nothing marking it as empty, and taken as an observation it reads as a
/// timestamp of 0 at a price of 1.0 -- a number far from any real one, in a
/// window that would otherwise look answerable. Asking for a window that runs
/// past the newest entry has to be refused for want of coverage, not answered
/// out of the zeros.
#[test]
fn a_window_running_past_the_last_observation_is_refused_not_read_from_zeros() {
    let fixture = partly_filled(10, 5);
    let (from, to) = fixture.window();
    let data = fixture.encode();

    assert_eq!(
        read(&data, from, to + 1, 30),
        Err(FeedError::WindowNotCovered),
        "the tail of the ring is not data",
    );
    assert_eq!(
        read(&data, from - 1, to, 30),
        Err(FeedError::WindowNotCovered),
        "and neither is anything before the first entry",
    );
}

#[test]
fn wrong_account_is_refused_before_anything_is_read() {
    let fixture = Fixture::constant(10);
    let (from, to) = fixture.window();

    let mut foreign_pool = Fixture::constant(10);
    foreign_pool.pool = [9u8; 32];
    assert_eq!(
        read(&foreign_pool.encode(), from, to, 30),
        Err(FeedError::PoolMismatch)
    );

    let mut wrong_type = fixture.encode();
    wrong_type[0] ^= 0xff;
    assert_eq!(
        read(&wrong_type, from, to, 30),
        Err(FeedError::WrongAccountType)
    );

    let mut uninitialised = Fixture::constant(10);
    uninitialised.initialized = false;
    assert_eq!(
        read(&uninitialised.encode(), from, to, 30),
        Err(FeedError::NotInitialized)
    );

    assert_eq!(
        read(&fixture.encode()[..OBSERVATION_STATE_LEN - 1], from, to, 30),
        Err(FeedError::AccountTooSmall)
    );
}

#[test]
fn corrupted_cumulative_trips_the_layout_canary() {
    // Raydium's cumulative always advances by an exact multiple of the
    // duration. Anything else means the account layout or the accumulation
    // rule changed under us -- which is the failure mode to expect if the
    // Raydium multisig upgrades the program.
    let fixture = Fixture::constant(10);
    let (from, to) = fixture.window();
    let mut data = fixture.encode();
    let base = OFF_OBSERVATIONS + 50 * OBSERVATION_LEN;
    data[base + 4] = data[base + 4].wrapping_add(1);

    // Exactly this error: the mutated byte belongs to `tick_cumulative`, so a
    // timestamp complaint would mean the parser was reading the wrong field.
    assert_eq!(
        read(&data, from, to, 30),
        Err(FeedError::InconsistentCumulative)
    );
}

#[test]
fn non_monotonic_timestamps_are_refused() {
    let mut fixture = Fixture::constant(10);
    fixture.timestamps[50] = fixture.timestamps[49];
    let (from, to) = fixture.window();
    assert_eq!(
        read(&fixture.encode(), from, to, 30),
        Err(FeedError::NonMonotonic)
    );
}

#[test]
fn empty_or_inverted_windows_are_refused() {
    let fixture = Fixture::constant(10);
    let (from, to) = fixture.window();
    let data = fixture.encode();
    assert_eq!(read(&data, to, to, 30), Err(FeedError::EmptyWindow));
    assert_eq!(read(&data, to, from, 30), Err(FeedError::EmptyWindow));
}

#[test]
fn boundaries_record_enough_to_reproduce_the_reading() {
    let fixture = Fixture::constant(-16_096);
    let (from, to) = fixture.window();
    let reading = raydium_twap(&fixture.encode(), &POOL, from + 7, to - 7, limits(30)).unwrap();

    // Everything a verifier needs after the ring has been overwritten.
    assert!(reading.start.observed_at <= (from + 7) as u32);
    assert!(reading.end.observed_at <= (to - 7) as u32);
    assert!((reading.start.index as usize) < OBSERVATION_NUM);
    assert!((reading.end.index as usize) < OBSERVATION_NUM);
    assert_eq!(reading.segments as usize, OBSERVATION_NUM - 1);

    let elapsed = (to - 7) - (from + 7);
    let delta = reading.end.interpolated - reading.start.interpolated;
    assert_eq!(delta / elapsed, i64::from(reading.average_tick));
}

#[test]
fn a_window_nobody_traded_through_is_refused() {
    // The ring is full, fresh and internally consistent -- and the window still
    // contains no trading at all, so its "average" would be one stale reading.
    let mut segments = vec![(15u32, 10i32); 50];
    segments.push((900, 10));
    segments.extend(vec![(15u32, 10i32); OBSERVATION_NUM - 52]);
    let fixture = Fixture::from_segments(&segments);
    let data = fixture.encode();

    // A window sitting inside the long quiet stretch.
    let quiet_start = i64::from(fixture.timestamps[50]) + 100;
    let strict = ClmmLimits {
        max_segment: 900,
        min_observations: 3,
    };
    assert_eq!(
        raydium_twap(&data, &POOL, quiet_start, quiet_start + 300, strict),
        Err(FeedError::WindowTooQuiet)
    );

    // The same window over the busy part of the ring is fine.
    let busy_start = i64::from(fixture.timestamps[0]);
    let reading = raydium_twap(&data, &POOL, busy_start, busy_start + 300, strict)
        .expect("a traded window reads");
    assert!(reading.observations_inside >= 3);
}

#[test]
fn a_recorded_boundary_pins_down_the_answer_it_produced() {
    // Two rings identical except for the tick of their final segment. The
    // window ends inside that segment, so the answers differ -- and the
    // archived record has to differ with them, or a settlement could never be
    // audited after the ring was overwritten.
    let build = |final_tick: i32| {
        let mut segments = vec![(15u32, 10i32); OBSERVATION_NUM - 2];
        segments.push((15, final_tick));
        Fixture::from_segments(&segments)
    };
    let calm = build(10);
    let spike = build(4_000);
    let (from, last) = calm.window();
    let to = last - 7;

    let calm_reading = raydium_twap(&calm.encode(), &POOL, from, to, limits(30)).unwrap();
    let spike_reading = raydium_twap(&spike.encode(), &POOL, from, to, limits(30)).unwrap();

    assert_ne!(
        calm_reading.average_tick, spike_reading.average_tick,
        "the fixtures must actually disagree for this test to mean anything"
    );

    // The near observation alone is identical in both -- which is exactly why
    // recording only it was not enough.
    assert_eq!(calm_reading.end.index, spike_reading.end.index);
    assert_eq!(calm_reading.end.cumulative, spike_reading.end.cumulative);

    // The segment's far end is what separates them.
    assert_ne!(
        calm_reading.end.next_cumulative,
        spike_reading.end.next_cumulative
    );

    // And from the recorded pair alone, the answer is reproducible.
    for reading in [calm_reading, spike_reading] {
        let end = reading.end;
        let slope = (end.next_cumulative - end.cumulative)
            / i64::from(end.next_observed_at - end.observed_at);
        let recomputed = end.cumulative + slope * (to - i64::from(end.observed_at));
        assert_eq!(recomputed, end.interpolated);
    }
}

#[test]
fn windows_near_the_ends_of_time_are_refused_not_panicked_on() {
    // Regression: bounding the window used to be a side effect of interpolating
    // the boundaries first. When the segment scan moved ahead of it, a window
    // down near i64::MIN reached the overlap subtraction with operands 2^63
    // apart and overflowed it -- a panic in a crate that promises never to.
    let fixture = Fixture::constant(-16_096);
    let data = fixture.encode();
    let cases = [
        (i64::MIN + 1, i64::MIN + 301),
        (i64::MIN, i64::MIN + 1),
        (i64::MAX - 300, i64::MAX),
        (i64::MIN + 1, i64::MAX),
        (0, 1),
    ];
    for (from, to) in cases {
        assert_eq!(
            read(&data, from, to, 30),
            Err(FeedError::WindowNotCovered),
            "window ({from}, {to})"
        );
    }
}

proptest! {
    /// Any window at all, over a valid buffer, resolves or fails cleanly.
    ///
    /// The generator is deliberately biased toward the ends of `i64`: a
    /// uniform draw lands in the interesting band with probability about
    /// 1e-10, which is why the original fuzz test missed the overflow above.
    #[test]
    fn any_window_over_a_valid_buffer_is_total(
        from in prop_oneof![
            Just(i64::MIN), Just(i64::MIN + 1), Just(i64::MAX), Just(0i64),
            (i64::MIN..i64::MIN + 4_000_000_000),
            (1_799_000_000i64..1_801_000_000),
            any::<i64>(),
        ],
        length in prop_oneof![Just(0i64), Just(1i64), (1i64..100_000), any::<i64>()],
    ) {
        let fixture = Fixture::constant(-16_096);
        let to = from.saturating_add(length);
        let _ = read(&fixture.encode(), from, to, 30);
    }

    /// Arbitrary bytes must never panic the parser -- it reads account data an
    /// attacker can shape.
    #[test]
    fn parsing_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..5000)) {
        let _ = raydium_twap(&bytes, &POOL, 1_800_000_000, 1_800_000_300, limits(30));
    }

    /// Arbitrary windows over a valid buffer either read cleanly or fail
    /// cleanly, and a constant-tick pool always averages to that tick.
    #[test]
    fn constant_pools_read_their_tick_over_any_covered_window(
        tick in -200_000i32..200_000,
        start_offset in 0i64..600,
        length in 1i64..600,
    ) {
        let fixture = Fixture::constant(tick);
        let (from, _) = fixture.window();
        let outcome = read(
            &fixture.encode(),
            from + start_offset,
            from + start_offset + length,
            30,
        );
        match outcome {
            Ok(average) => prop_assert_eq!(average, tick),
            Err(FeedError::WindowNotCovered) => {}
            Err(other) => prop_assert!(false, "unexpected failure: {:?}", other),
        }
    }
}

/// What each of the two knobs actually decides.
///
/// `max_segment` below the window demands a reading inside it; at or above the
/// window it demands none, and a quiet window averages the one price that stood
/// throughout. Neither is safer than the other -- they differ in how many trades
/// a pool needs before it can be settled from -- so both are pinned here rather
/// than argued in a comment.
mod the_segment_bound_is_what_proves_the_pool_traded {
    use super::*;

    const WINDOW: u32 = 900;
    const TICK: i32 = 100;

    /// A full ring whose last segments have the shape a test needs.
    ///
    /// The reader refuses a partly-filled buffer, so the interesting gaps are
    /// padded out to ninety-nine segments with short ones.
    fn ring_ending_with(tail: &[u32]) -> (Vec<u8>, Vec<u32>) {
        let filler = OBSERVATION_NUM - 1 - tail.len();
        let mut segments: Vec<(u32, i32)> = vec![(OBSERVATION_UPDATE_DURATION, TICK); filler];
        segments.extend(tail.iter().map(|duration| (*duration, TICK)));
        let fixture = Fixture::from_segments(&segments);
        (fixture.encode(), fixture.timestamps.clone())
    }

    /// A window spanned by a single segment: readings bracketing it and nothing
    /// between. A bound below the window refuses it.
    #[test]
    fn one_segment_covering_the_whole_window_is_refused() {
        // ... a long quiet stretch, then one final write.
        let (data, stamps) = ring_ending_with(&[WINDOW + 120, 30]);
        let quiet_start = i64::from(stamps[stamps.len() - 3]);

        let from = quiet_start + 30;
        let to = from + i64::from(WINDOW);
        assert_eq!(
            read(&data, from, to, WINDOW / 2),
            Err(FeedError::SegmentTooLong),
            "a window nobody traded through must not settle, whatever the floor is",
        );
    }

    /// The same window with one reading inside it passes, and reports exactly
    /// that one -- which is all the average needs, since a cumulative TWAP is
    /// two readings and the distance between them.
    #[test]
    fn a_single_observation_inside_is_enough_to_settle() {
        // With a window of `W` and a segment bound of `W/2`, the lone interior
        // reading has to sit exactly at the midpoint: any further either way
        // and one of the two segments overlaps more than the bound allows.
        let half = WINDOW / 2;
        let lead = 30;
        let (data, stamps) = ring_ending_with(&[lead + half, half + lead, lead]);
        let before = i64::from(stamps[stamps.len() - 4]);

        let from = before + i64::from(lead);
        let to = from + i64::from(WINDOW);
        let reading = raydium_twap(
            &data,
            &POOL,
            from,
            to,
            ClmmLimits {
                max_segment: half,
                min_observations: 1,
            },
        )
        .expect("one reading inside the window is a legal average");

        assert_eq!(reading.observations_inside, 1);
        assert_eq!(reading.average_tick, TICK);
    }

    /// The same untraded window, read under a bound that does not ask for
    /// interior activity: it settles, at the price that stood for all of it.
    ///
    /// This is what lets one trade revive a window instead of two. The single
    /// write has to land at or after the window closes -- an average needs a
    /// reading at each end, and no trade made before the end can supply the one
    /// at the end -- but nothing has to happen inside.
    #[test]
    fn one_trade_after_the_window_settles_it_when_no_interior_reading_is_required() {
        let (data, stamps) = ring_ending_with(&[WINDOW + 120, 30]);
        let quiet_start = i64::from(stamps[stamps.len() - 3]);

        let from = quiet_start + 30;
        let to = from + i64::from(WINDOW);
        let reading = raydium_twap(
            &data,
            &POOL,
            from,
            to,
            ClmmLimits {
                max_segment: WINDOW,
                min_observations: 0,
            },
        )
        .expect("a quiet window is one price, which is a legal average of it");

        assert_eq!(reading.observations_inside, 0);
        assert_eq!(reading.average_tick, TICK);
    }
}
