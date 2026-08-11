//! Parsing and averaging Raydium CLMM's `ObservationState`.

use crate::FeedError;

/// Ring capacity. `OBSERVATION_NUM` in Raydium's source.
pub const OBSERVATION_NUM: usize = 100;

/// Minimum spacing Raydium enforces between observations, in seconds.
///
/// This is a floor in consensus, not a suggestion: `update()` returns early
/// when less time has passed. It is what bounds how fast the ring can be
/// overwritten, and therefore what guarantees the history depth.
pub const OBSERVATION_UPDATE_DURATION: u32 = 15;

/// Guaranteed history depth, in seconds: 99 gaps, each at least 15 seconds.
///
/// Note the 99: a hundred observations delimit ninety-nine intervals. A quiet
/// pool reaches further back than this, never less.
pub const GUARANTEED_HISTORY: u32 = (OBSERVATION_NUM as u32 - 1) * OBSERVATION_UPDATE_DURATION;

/// Total account size, discriminator included.
pub const OBSERVATION_STATE_LEN: usize = 4483;

/// `sha256("account:ObservationState")[..8]`, verified by a unit test.
const DISCRIMINATOR: [u8; 8] = [0x7a, 0xae, 0xc5, 0x35, 0x81, 0x09, 0xa5, 0x84];

// Byte offsets into the account. The struct is `#[repr(C, packed)]`, so these
// are exact and there is no alignment padding to skip.
const OFF_INITIALIZED: usize = 8;
const OFF_OBSERVATION_INDEX: usize = 17;
const OFF_POOL_ID: usize = 19;
const OFF_OBSERVATIONS: usize = 51;
const OBSERVATION_LEN: usize = 44;

/// One end of the averaging window, recorded so the snapshot survives the ring
/// being overwritten.
///
/// Both ends of the bracketing segment are kept: the interpolated value depends
/// on the segment's slope, and a slope needs two points. Archiving only the near
/// one would let two different rings produce identical records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Boundary {
    /// Ring slot holding the observation at or before this boundary.
    pub index: u16,
    /// That observation's timestamp.
    pub observed_at: u32,
    /// That observation's cumulative.
    pub cumulative: i64,
    /// Ring slot holding the observation that closes the segment.
    ///
    /// Equal to `index` only at the newest observation, where no segment
    /// follows and no interpolation was needed.
    pub next_index: u16,
    pub next_observed_at: u32,
    pub next_cumulative: i64,
    /// The cumulative interpolated to the boundary instant itself.
    pub interpolated: i64,
}

/// The result of averaging, plus everything needed to reproduce it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TwapReading {
    /// Time-weighted average tick over the window, rounded toward negative
    /// infinity.
    pub average_tick: i32,
    pub start: Boundary,
    pub end: Boundary,
    /// Number of observations bounding segments that overlap the window.
    pub segments: u16,
    /// Observations recorded strictly inside the window.
    pub observations_inside: u16,
}

/// What a window must look like before its average is worth settling against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClmmLimits {
    /// Longest stretch of the window any single segment may account for.
    ///
    /// Not an attribution bound -- see the crate documentation, that error is
    /// capped at fifteen seconds by Raydium itself. This stops one segment
    /// from *being* the average, which would make the window a single reading
    /// wearing the word "average".
    pub max_segment: u32,
    /// Observations required strictly inside the window.
    ///
    /// This is the real quality test: it proves the pool was traded during the
    /// period being measured, rather than last touched some time before it and
    /// coasting. Measured against live mainnet rings, the deepest SOL/USDC pool
    /// records one roughly every fifty seconds, so a fifteen-minute window
    /// carries well over a dozen.
    pub min_observations: u16,
}

/// Time-weighted average tick over `[from, to]`, read from a Raydium CLMM
/// observation buffer.
///
/// `expected_pool` must be the pool the market's spec named; the check is what
/// stops a caller from passing some other pool's buffer.
pub fn raydium_twap(
    account_data: &[u8],
    expected_pool: &[u8; 32],
    from: i64,
    to: i64,
    limits: ClmmLimits,
) -> Result<TwapReading, FeedError> {
    if to <= from {
        return Err(FeedError::EmptyWindow);
    }

    let (walk, first) = RingWalk::new(account_data, expected_pool)?;

    // Bound the window before any segment arithmetic: a window far outside the
    // recorded range would reach the overlap subtraction with operands ~2^63
    // apart. Once both ends are known to lie within the ring, every difference
    // below is under 2^32, since timestamps are `u32`.
    let last = walk.read(account_data, walk.written - 1)?;
    if from < i64::from(first.timestamp) || to > i64::from(last.timestamp) {
        return Err(FeedError::WindowNotCovered);
    }

    let mut previous = first;
    let mut start: Option<Boundary> = None;
    let mut end: Option<Boundary> = None;
    let mut segments: u16 = 0;
    let mut observations_inside: u16 = 0;

    for position in 1..walk.written {
        let current = walk.read(account_data, position)?;
        if current.timestamp <= previous.timestamp {
            return Err(FeedError::NonMonotonic);
        }
        let tick = segment_tick(&previous, &current)?;

        let segment_start = i64::from(previous.timestamp);
        let segment_end = i64::from(current.timestamp);

        // Bounded on overlap rather than full length: overlap is exactly the
        // weight a momentary price carries in the average.
        let overlap = segment_end.min(to) - segment_start.max(from);
        if overlap > i64::from(limits.max_segment) {
            return Err(FeedError::SegmentTooLong);
        }
        if segment_end > from && segment_start < to {
            segments += 1;
        }
        if (from..to).contains(&segment_end) {
            observations_inside += 1;
        }

        if start.is_none() && (segment_start..segment_end).contains(&from) {
            start = Some(boundary_at(&previous, &current, tick, from));
        }
        if end.is_none() && (segment_start..segment_end).contains(&to) {
            end = Some(boundary_at(&previous, &current, tick, to));
        }
        if end.is_none() && segment_end == to {
            end = Some(boundary_at_newest(&current));
        }

        previous = current;
    }

    let (start, end) = match (start, end) {
        (Some(start), Some(end)) => (start, end),
        _ => return Err(FeedError::WindowNotCovered),
    };

    // A window nobody traded through is one price, not an average of many.
    if observations_inside < limits.min_observations {
        return Err(FeedError::WindowTooQuiet);
    }

    let elapsed = i128::from(to - from);
    // Raydium accumulates with `wrapping_add`, so the inverse must wrap too.
    let delta = i128::from(end.interpolated.wrapping_sub(start.interpolated));
    let average_tick = floor_div(delta, elapsed);

    Ok(TwapReading {
        average_tick: i32::try_from(average_tick).map_err(|_| FeedError::TickOutOfRange)?,
        start,
        end,
        segments,
        observations_inside,
    })
}

/// Walks the ring once, validating as it goes.
///
/// Nothing is materialised. An earlier version collected the hundred
/// timestamps, cumulatives, slot numbers and segment ticks into arrays; that is
/// 2192 bytes of stack, which is fine on a host and fatal inside a Solana
/// program, whose frames are 4 KiB. Streaming keeps the footprint constant and
/// costs nothing, since every check is local to one segment anyway.
struct RingWalk {
    data_start: usize,
    oldest_slot: usize,
    /// Observations actually written, which is the capacity only once the ring
    /// has wrapped. A pool opened this morning has a handful.
    written: usize,
}

/// The timestamp in a slot, read without deciding whether the slot is real.
fn timestamp_at(data: &[u8], slot: usize) -> u32 {
    let base = OFF_OBSERVATIONS + slot * OBSERVATION_LEN;
    u32::from_le_bytes([data[base], data[base + 1], data[base + 2], data[base + 3]])
}

/// One observation, as stored.
#[derive(Clone, Copy)]
struct Entry {
    slot: u16,
    timestamp: u32,
    cumulative: i64,
}

impl RingWalk {
    fn new(data: &[u8], expected_pool: &[u8; 32]) -> Result<(Self, Entry), FeedError> {
        if data.len() < OBSERVATION_STATE_LEN {
            return Err(FeedError::AccountTooSmall);
        }
        if data[..8] != DISCRIMINATOR {
            return Err(FeedError::WrongAccountType);
        }
        if data[OFF_INITIALIZED] == 0 {
            return Err(FeedError::NotInitialized);
        }
        if &data[OFF_POOL_ID..OFF_POOL_ID + 32] != expected_pool.as_slice() {
            return Err(FeedError::PoolMismatch);
        }

        let newest =
            u16::from_le_bytes([data[OFF_OBSERVATION_INDEX], data[OFF_OBSERVATION_INDEX + 1]]);
        if newest as usize >= OBSERVATION_NUM {
            return Err(FeedError::IndexOutOfRange);
        }

        // Whether the ring has wrapped is readable, not guessed: the slot just
        // past the newest is either the oldest entry or has never been written.
        // A new pool is a real pool, and refusing one until a hundred swaps have
        // gone through it would refuse every pool on its first day.
        let newest = newest as usize;
        let after_newest = (newest + 1) % OBSERVATION_NUM;
        let wrapped = timestamp_at(data, after_newest) != 0;

        let walk = RingWalk {
            data_start: OFF_OBSERVATIONS,
            oldest_slot: if wrapped { after_newest } else { 0 },
            written: if wrapped { OBSERVATION_NUM } else { newest + 1 },
        };

        // Two readings are the fewest an average can be taken between.
        if walk.written < 2 {
            return Err(FeedError::BufferNotFull);
        }
        let first = walk.read(data, 0)?;
        Ok((walk, first))
    }

    /// Reads the entry `position` steps after the oldest.
    fn read(&self, data: &[u8], position: usize) -> Result<Entry, FeedError> {
        if position >= self.written {
            return Err(FeedError::IndexOutOfRange);
        }
        let slot = (self.oldest_slot + position) % OBSERVATION_NUM;
        let base = self.data_start + slot * OBSERVATION_LEN;

        let timestamp = timestamp_at(data, slot);
        // Nothing marks a never-written slot, and read as data it yields a price
        // of 1.0. Within `written` this cannot happen; a zero here means the
        // account is not laid out the way this parser believes.
        if timestamp == 0 {
            return Err(FeedError::BufferNotFull);
        }
        let mut cumulative = [0u8; 8];
        cumulative.copy_from_slice(&data[base + 4..base + 12]);
        Ok(Entry {
            slot: slot as u16,
            timestamp,
            cumulative: i64::from_le_bytes(cumulative),
        })
    }
}

/// Recovers the constant tick that prevailed over one segment.
///
/// Exact by construction: Raydium stores `cumulative + tick * delta_time`, which
/// cannot saturate for `|tick| <= 443636` over a `u32` duration, so the delta
/// always divides evenly. Either check failing is the canary for Raydium's
/// layout having changed under us.
fn segment_tick(previous: &Entry, current: &Entry) -> Result<i64, FeedError> {
    let duration = i64::from(current.timestamp - previous.timestamp);
    let delta = current.cumulative.wrapping_sub(previous.cumulative);
    if delta % duration != 0 {
        return Err(FeedError::InconsistentCumulative);
    }
    let tick = delta / duration;
    if i32::try_from(tick).is_err() {
        return Err(FeedError::InconsistentCumulative);
    }
    Ok(tick)
}

/// The cumulative at `instant`, exactly.
///
/// Within a segment the cumulative is linear with a known integer slope, so
/// this is interpolation and not approximation. Instants outside the recorded
/// range are never reached: the caller only calls this for an instant it has
/// already bracketed.
fn boundary_at(previous: &Entry, next: &Entry, tick: i64, instant: i64) -> Boundary {
    let elapsed = instant - i64::from(previous.timestamp);
    Boundary {
        index: previous.slot,
        observed_at: previous.timestamp,
        cumulative: previous.cumulative,
        next_index: next.slot,
        next_observed_at: next.timestamp,
        next_cumulative: next.cumulative,
        interpolated: previous.cumulative.wrapping_add(tick.wrapping_mul(elapsed)),
    }
}

/// The window ends exactly on the newest observation, so nothing was
/// interpolated and there is no following segment to name.
fn boundary_at_newest(entry: &Entry) -> Boundary {
    Boundary {
        index: entry.slot,
        observed_at: entry.timestamp,
        cumulative: entry.cumulative,
        next_index: entry.slot,
        next_observed_at: entry.timestamp,
        next_cumulative: entry.cumulative,
        interpolated: entry.cumulative,
    }
}

/// Integer division rounding toward negative infinity.
///
/// Rust truncates toward zero. The average tick of a normal pair is negative
/// -- SOL/USDC with 9 and 6 decimals sits near tick -16094 -- so the two
/// disagree on the common case, by one tick, which is one basis point, which
/// is the difference between YES and NO.
fn floor_div(numerator: i128, denominator: i128) -> i128 {
    let quotient = numerator / denominator;
    if numerator % denominator != 0 && (numerator < 0) != (denominator < 0) {
        quotient - 1
    } else {
        quotient
    }
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn floor_div_rounds_down_for_negatives() {
        assert_eq!(floor_div(7, 3), 2);
        assert_eq!(floor_div(-7, 3), -3);
        assert_eq!(floor_div(7, -3), -3);
        assert_eq!(floor_div(-7, -3), 2);
        assert_eq!(floor_div(-6, 3), -2);
    }

    #[test]
    fn guaranteed_history_leaves_room_for_the_protocol_budget() {
        // The protocol caps `W + G` at 1200 s; this is the margin it relies on.
        assert_eq!(GUARANTEED_HISTORY, 1485);
        const { assert!(GUARANTEED_HISTORY > 1200) };
    }
}
