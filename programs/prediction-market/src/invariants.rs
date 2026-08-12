//! Cross-crate invariants.
//!
//! Core's window-plus-grace budget is derived from the history a feeds ring
//! guarantees, but the two crates do not depend on each other. This program
//! depends on both, so it is the only place the relationship can be asserted.
//!
//! The assertions are constant by design -- that is what a static check is.
#![allow(clippy::assertions_on_constants)]

use market_core::MAX_HISTORY_BUDGET;
use market_feeds::{GUARANTEED_HISTORY, OBSERVATION_NUM, OBSERVATION_UPDATE_DURATION};

/// Slack the schedule leaves against the shallowest ring, in seconds.
const REQUIRED_MARGIN: u32 = 240;

#[test]
fn a_schedule_always_fits_inside_the_guaranteed_history() {
    assert!(
        MAX_HISTORY_BUDGET + REQUIRED_MARGIN <= GUARANTEED_HISTORY,
        "a schedule may span {MAX_HISTORY_BUDGET}s but the ring only guarantees \
         {GUARANTEED_HISTORY}s, leaving less than the {REQUIRED_MARGIN}s of margin \
         a snapshot needs to reach back past its own grace period"
    );
}

#[test]
fn the_guaranteed_history_is_what_the_ring_actually_promises() {
    // A hundred observations delimit ninety-nine intervals, and Raydium
    // refuses to write two closer together than its minimum. Getting the
    // fencepost wrong here would overstate the depth by fifteen seconds.
    assert_eq!(
        GUARANTEED_HISTORY,
        (OBSERVATION_NUM as u32 - 1) * OBSERVATION_UPDATE_DURATION
    );
    assert_eq!(GUARANTEED_HISTORY, 1_485);
}

/// The byte the TypeScript client filters markets on.
///
/// `fetchMarkets` narrows by status with a `memcmp` at a fixed offset, which is
/// knowledge of this layout living outside this crate. Reordering `Market`'s
/// first fields would leave that filter pointing at the wrong byte and silently
/// returning nothing, so the offset is pinned here where a reorder is made.
#[test]
fn status_sits_where_the_client_looks_for_it() {
    use anchor_lang::AnchorSerialize;

    // Only the first two fields matter, and Borsh writes them in order, so the
    // rest need not be built.
    let mut bytes = Vec::new();
    254u8.serialize(&mut bytes).expect("bump");
    crate::state::MarketStatus::Snapshotted
        .serialize(&mut bytes)
        .expect("status");

    // The account is written after an eight-byte discriminator, so the client's
    // offset is eight more than the offset within the struct.
    const DISCRIMINATOR: usize = 8;
    const CLIENT_STATUS_OFFSET: usize = 9;

    assert_eq!(bytes[0], 254, "bump is the first field");
    assert_eq!(
        bytes[CLIENT_STATUS_OFFSET - DISCRIMINATOR],
        3,
        "status must be the byte the client filters on, and Snapshotted must be 3"
    );
}
