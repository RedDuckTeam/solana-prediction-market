//! Prices frozen at settlement, and the evidence behind them.

use anchor_lang::prelude::*;
use market_vm::MAX_INPUTS;

/// Prices frozen at settlement, plus everything needed to re-derive them.
///
/// The raw observation indices and cumulatives are kept because the ring they
/// came from is overwritten within about twenty-five minutes on an active pool.
/// Without them a resolution could never be audited after the fact, which would
/// make "verifiable settlement" a claim rather than a property.
#[account]
#[derive(InitSpace)]
pub struct Snapshot {
    pub bump: u8,
    pub market: Pubkey,
    pub taken_at: i64,
    pub keeper: Pubkey,
    /// Hash over every field below, so a snapshot can be quoted compactly.
    pub feeds_hash: [u8; 32],
    #[max_len(MAX_INPUTS)]
    pub prices: Vec<i128>,
    #[max_len(MAX_INPUTS)]
    pub readings: Vec<ReadingRecord>,
}

/// One feed's reading, in enough detail to reproduce it from archived state.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, InitSpace)]
pub struct ReadingRecord {
    pub feed: Pubkey,
    pub source: SourceRecord,
}

/// Everything needed to re-derive one reading, whatever produced it.
///
/// Archived differently because they are verifiable differently: a Raydium
/// reading is reproduced from Solana's own history, so the raw pair is what
/// matters, while a Pyth account was transient and cannot be reproduced at all —
/// so what is kept is the assertion believed and the tests it passed.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, InitSpace)]
pub enum SourceRecord {
    RaydiumClmm {
        average_tick: i32,
        window_start: BoundaryRecord,
        window_end: BoundaryRecord,
    },
    PythTwap {
        raw_price: i64,
        raw_conf: u64,
        exponent: i32,
        confidence_bps: u32,
        down_slots_ratio: u32,
        start_time: i64,
        end_time: i64,
    },
}

/// Both observations bounding the segment a window boundary falls in.
///
/// The pair, not just the nearer one: the interpolated cumulative depends on
/// the segment's slope, and a slope needs two points. Recording one would let
/// two different rings archive identical bytes and settle differently.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, InitSpace)]
pub struct BoundaryRecord {
    pub index: u16,
    pub observed_at: u32,
    pub cumulative: i64,
    pub next_index: u16,
    pub next_observed_at: u32,
    pub next_cumulative: i64,
    pub interpolated: i64,
}
