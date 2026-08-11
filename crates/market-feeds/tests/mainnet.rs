//! The parser, against real Raydium accounts pulled off mainnet.
//!
//! Synthetic fixtures only prove the reader agrees with itself. Bytes Raydium
//! produced are the only thing that can show the layout, the accumulation rule
//! and the exactness canary are what we believe.
//!
//! Fixtures live in `tests/fixtures` and are committed, so the suite runs
//! offline long after those rings were overwritten. See the README for what
//! they disproved.

use std::fs;
use std::path::PathBuf;

use market_feeds::{
    normalized_price, raydium_twap, read_pool_state, ClmmLimits, FeedError, OBSERVATION_NUM,
};

/// Offsets into `ObservationState`, mirrored here so the test reads the bytes
/// independently of the code it is testing.
const OFF_OBSERVATION_INDEX: usize = 17;
const OFF_OBSERVATIONS: usize = 51;
const OBSERVATION_LEN: usize = 44;

struct Fixture {
    name: String,
    ring: Vec<u8>,
    pool: [u8; 32],
    decimals_0: u8,
    decimals_1: u8,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn load() -> Vec<Fixture> {
    let dir = fixtures_dir();
    let manifest = fs::read_to_string(dir.join("mainnet.json"))
        .expect("mainnet fixtures are missing from tests/fixtures");

    // A three-field manifest does not justify a JSON dependency in a crate that
    // otherwise has one.
    manifest
        .split("\"pool\": \"")
        .skip(1)
        .map(|chunk| {
            let name = chunk.split('"').next().expect("pool address").to_string();
            let field = |key: &str| -> u8 {
                chunk
                    .split(&format!("\"{key}\": "))
                    .nth(1)
                    .and_then(|rest| rest.split(&[',', '\n'][..]).next())
                    .and_then(|value| value.trim().parse().ok())
                    .unwrap_or_else(|| panic!("{key} missing for {name}"))
            };
            let mut pool = [0u8; 32];
            pool.copy_from_slice(
                &fs::read(dir.join(format!("{name}.poolkey"))).expect("pool key fixture"),
            );
            Fixture {
                ring: fs::read(dir.join(format!("{name}.observation"))).expect("ring fixture"),
                pool,
                decimals_0: field("decimals_0"),
                decimals_1: field("decimals_1"),
                name,
            }
        })
        .collect()
}

/// Reads the ring's timestamps directly, without going through the parser.
fn timestamps(ring: &[u8]) -> Vec<u32> {
    let newest =
        u16::from_le_bytes([ring[OFF_OBSERVATION_INDEX], ring[OFF_OBSERVATION_INDEX + 1]]) as usize;
    (0..OBSERVATION_NUM)
        .map(|position| {
            let slot = (newest + 1 + position) % OBSERVATION_NUM;
            let base = OFF_OBSERVATIONS + slot * OBSERVATION_LEN;
            u32::from_le_bytes([ring[base], ring[base + 1], ring[base + 2], ring[base + 3]])
        })
        .collect()
}

#[test]
fn the_parser_reads_accounts_raydium_actually_produced() {
    let fixtures = load();
    assert!(!fixtures.is_empty(), "no fixtures captured");

    for fixture in &fixtures {
        let stamps = timestamps(&fixture.ring);
        let (oldest, newest) = (i64::from(stamps[0]), i64::from(stamps[OBSERVATION_NUM - 1]));

        // A fifteen-minute window ending a little before the newest entry, so
        // both boundaries are bracketed by real observations.
        let to = newest - 60;
        let from = to - 900;
        assert!(
            from > oldest,
            "{}: ring is too shallow to test",
            fixture.name
        );

        let reading = raydium_twap(
            &fixture.ring,
            &fixture.pool,
            from,
            to,
            ClmmLimits {
                max_segment: 450,
                min_observations: 3,
            },
        )
        .unwrap_or_else(|error| panic!("{}: live account rejected: {error:?}", fixture.name));

        let price = normalized_price(
            reading.average_tick,
            fixture.decimals_0,
            fixture.decimals_1,
            false,
        )
        .unwrap_or_else(|error| panic!("{}: price rejected: {error:?}", fixture.name));

        // SOL against USDC. A band this wide is not a market call -- it is a
        // check that the decimals, the sign of the tick and the exponent all
        // point the same way. Getting any of them wrong misses by orders of
        // magnitude, not by percent.
        let as_float = price.raw() as f64 / 2f64.powi(64);
        assert!(
            (1.0..10_000.0).contains(&as_float),
            "{}: SOL priced at {as_float} USDC, which means a conversion is wrong",
            fixture.name
        );

        println!(
            "{}: tick {} -> {as_float:.4} USDC, {} observations inside the window",
            fixture.name, reading.average_tick, reading.observations_inside
        );
    }
}

#[test]
fn the_pool_parser_reads_accounts_raydium_actually_produced() {
    // Registration reads the pair's mints and decimals out of `PoolState`
    // rather than taking governance's word, so the offsets it reads from have
    // to hold on accounts Raydium wrote -- not merely on ones this repository
    // writes for itself.
    for fixture in load() {
        let data = fs::read(fixtures_dir().join(format!("{}.pool", fixture.name)))
            .expect("pool state fixture");
        let info = read_pool_state(&data)
            .unwrap_or_else(|error| panic!("{}: live pool rejected: {error:?}", fixture.name));

        assert_eq!(info.mint_decimals0, fixture.decimals_0, "{}", fixture.name);
        assert_eq!(info.mint_decimals1, fixture.decimals_1, "{}", fixture.name);
        assert_ne!(
            info.mint0, info.mint1,
            "{}: a pool prices a pair",
            fixture.name
        );
        assert_ne!(
            info.observation_key, [0u8; 32],
            "{}: every pool names its ring",
            fixture.name
        );
    }
}

#[test]
fn the_exactness_canary_does_not_fire_on_real_data() {
    // Every segment's cumulative delta must divide its duration exactly. The
    // whole layout-drift alarm rests on that being true of accounts Raydium
    // writes, not merely of ones this repository writes.
    for fixture in load() {
        let stamps = timestamps(&fixture.ring);
        let from = i64::from(stamps[1]);
        let to = i64::from(stamps[OBSERVATION_NUM - 1]);

        let reading = raydium_twap(
            &fixture.ring,
            &fixture.pool,
            from,
            to,
            ClmmLimits {
                max_segment: u32::MAX,
                min_observations: 0,
            },
        );
        assert!(
            !matches!(reading, Err(FeedError::InconsistentCumulative)),
            "{}: the canary fired on live data, so the assumption behind it is wrong",
            fixture.name
        );
        reading.unwrap_or_else(|error| panic!("{}: {error:?}", fixture.name));
    }
}

#[test]
fn a_wrong_pool_is_refused_even_with_a_real_ring() {
    for fixture in load() {
        let stamps = timestamps(&fixture.ring);
        let mut impostor = fixture.pool;
        impostor[0] ^= 0xff;
        assert_eq!(
            raydium_twap(
                &fixture.ring,
                &impostor,
                i64::from(stamps[1]),
                i64::from(stamps[OBSERVATION_NUM - 1]),
                ClmmLimits {
                    max_segment: u32::MAX,
                    min_observations: 0
                },
            ),
            Err(FeedError::PoolMismatch),
            "{}: a real ring must still belong to the pool that was declared",
            fixture.name
        );
    }
}

/// Reports the observation cadence real pools actually keep.
///
/// This is not an assertion about Raydium so much as a record of the numbers
/// the protocol's limits have to live with. The design originally assumed
/// observations arrive near the fifteen-second floor; they do not, and the
/// schedule defaults were rebuilt around what this prints.
#[test]
fn live_rings_show_the_cadence_the_limits_must_tolerate() {
    for fixture in load() {
        let stamps = timestamps(&fixture.ring);
        let mut gaps: Vec<u32> = stamps.windows(2).map(|pair| pair[1] - pair[0]).collect();
        gaps.sort_unstable();

        let span = stamps[OBSERVATION_NUM - 1] - stamps[0];
        let median = gaps[gaps.len() / 2];
        let worst = *gaps.last().expect("99 gaps");

        println!(
            "{}: span {span}s, gaps min {} median {median} max {worst}",
            fixture.name, gaps[0]
        );

        // The floor Raydium enforces, confirmed against live data.
        assert!(gaps[0] >= 15, "{}: a gap below the 15s floor", fixture.name);

        // The guaranteed depth is a floor, and a wide one: real pools reach
        // back far further than the 1485 seconds the protocol budgets against.
        assert!(
            span > 1_485,
            "{}: live ring spans only {span}s, at the guaranteed minimum",
            fixture.name
        );

        // And the reason `max_segment` was rebuilt. It is the worst gap that
        // decides, not the typical one: a single over-long segment rejects the
        // whole window, and both live pools carry gaps many times the
        // thirty seconds the original rule allowed.
        assert!(
            worst > 60,
            "{}: worst gap is only {worst}s -- if live pools now record this \
             evenly, the original max_segment rule deserves revisiting",
            fixture.name
        );
        let _ = median;
    }
}
