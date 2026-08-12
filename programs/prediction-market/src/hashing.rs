//! Hashing, and the host's implementation of the predicate machine's hasher.

use anchor_lang::prelude::*;
use market_vm::HostHasher;
use solana_sha256_hasher::hashv;

use crate::state::{BoundaryRecord, FeedRef, ReadingRecord, SourceRecord};

/// Binds a market to its spec for good.
///
/// The market identifier is part of the preimage, so two markets sharing a
/// predicate still commit to distinct hashes and neither can be settled against
/// the other's spec. Lengths are hashed alongside the variable-length fields so
/// that no two different specs can be made to produce the same byte stream.
pub fn spec_hash(
    market_id: &[u8; 32],
    settle_at: i64,
    strike: i128,
    ramp_bps: u16,
    feeds: &[FeedRef],
    bytecode: &[u8],
    rules_uri: &str,
) -> [u8; 32] {
    let feed_bytes: Vec<u8> = feeds
        .iter()
        .flat_map(|entry| {
            let mut encoded = entry.feed.to_bytes().to_vec();
            encoded.push(u8::from(entry.invert));
            encoded
        })
        .collect();

    hashv(&[
        market_id,
        &settle_at.to_le_bytes(),
        &strike.to_le_bytes(),
        &ramp_bps.to_le_bytes(),
        &(feeds.len() as u8).to_le_bytes(),
        &feed_bytes,
        &(bytecode.len() as u16).to_le_bytes(),
        bytecode,
        &(rules_uri.len() as u16).to_le_bytes(),
        rules_uri.as_bytes(),
    ])
    .to_bytes()
}

/// Commits to every number a snapshot was derived from.
///
/// The ring a reading came from is overwritten within about twenty-five minutes
/// on a busy pool, so without the indices and the raw cumulatives a settlement
/// could never be re-derived. This is what makes "verifiable" a property rather
/// than a claim.
pub fn readings_hash(readings: &[ReadingRecord]) -> [u8; 32] {
    fn encode_boundary(boundary: &BoundaryRecord, into: &mut Vec<u8>) {
        into.extend_from_slice(&boundary.index.to_le_bytes());
        into.extend_from_slice(&boundary.observed_at.to_le_bytes());
        into.extend_from_slice(&boundary.cumulative.to_le_bytes());
        into.extend_from_slice(&boundary.next_index.to_le_bytes());
        into.extend_from_slice(&boundary.next_observed_at.to_le_bytes());
        into.extend_from_slice(&boundary.next_cumulative.to_le_bytes());
        into.extend_from_slice(&boundary.interpolated.to_le_bytes());
    }

    let encoded: Vec<u8> = readings
        .iter()
        .flat_map(|reading| {
            let mut bytes = reading.feed.to_bytes().to_vec();
            match &reading.source {
                SourceRecord::RaydiumClmm {
                    average_tick,
                    window_start,
                    window_end,
                } => {
                    // The tag is hashed too, so a Raydium record and a Pyth one
                    // can never collide however their fields happen to line up.
                    bytes.push(0);
                    bytes.extend_from_slice(&average_tick.to_le_bytes());
                    encode_boundary(window_start, &mut bytes);
                    encode_boundary(window_end, &mut bytes);
                }
                SourceRecord::PythTwap {
                    raw_price,
                    raw_conf,
                    exponent,
                    confidence_bps,
                    down_slots_ratio,
                    start_time,
                    end_time,
                } => {
                    bytes.push(1);
                    bytes.extend_from_slice(&raw_price.to_le_bytes());
                    bytes.extend_from_slice(&raw_conf.to_le_bytes());
                    bytes.extend_from_slice(&exponent.to_le_bytes());
                    bytes.extend_from_slice(&confidence_bps.to_le_bytes());
                    bytes.extend_from_slice(&down_slots_ratio.to_le_bytes());
                    bytes.extend_from_slice(&start_time.to_le_bytes());
                    bytes.extend_from_slice(&end_time.to_le_bytes());
                }
            }
            bytes
        })
        .collect();
    hashv(&[&(readings.len() as u8).to_le_bytes(), &encoded]).to_bytes()
}

/// Wires the predicate machine's hashing to Solana's syscalls.
///
/// The machine takes its hasher as a trait so that the on-chain build, the
/// native tests, and the WebAssembly build the market builder runs all execute
/// the same interpreter. Only this implementation differs.
pub struct SyscallHasher;

impl HostHasher for SyscallHasher {
    fn sha256(&self, data: &[u8]) -> [u8; 32] {
        hashv(&[data]).to_bytes()
    }

    fn keccak256(&self, data: &[u8]) -> [u8; 32] {
        solana_keccak_hasher::hashv(&[data]).to_bytes()
    }
}
