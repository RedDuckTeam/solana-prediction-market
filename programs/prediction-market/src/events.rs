//! Events.
//!
//! Every state change emits one. Indexers and the front end read these instead
//! of diffing account snapshots, and they are also the audit trail: a
//! resolution can be reconstructed years later from `MarketResolved` plus the
//! `Snapshot` account it names, without replaying the chain.

use anchor_lang::prelude::*;

use crate::state::VoidCause;

#[event]
pub struct MarketCreated {
    pub market: Pubkey,
    pub creator: Pubkey,
    pub collateral_mint: Pubkey,
    pub settle_at: i64,
    pub lock_at: i64,
    pub cap_per_side: u64,
    pub spec_hash: [u8; 32],
}

#[event]
pub struct Deposited {
    pub market: Pubkey,
    pub depositor: Pubkey,
    pub side_is_yes: bool,
    pub amount: u64,
    pub staked_yes: u64,
    pub staked_no: u64,
}

#[event]
pub struct SnapshotTaken {
    pub market: Pubkey,
    pub keeper: Pubkey,
    pub taken_at: i64,
    pub feeds_hash: [u8; 32],
    /// Q64.64 prices, in the order the spec declares its feeds.
    pub prices: Vec<i128>,
}

#[event]
pub struct MarketResolved {
    pub market: Pubkey,
    pub resolver: Pubkey,
    /// Fraction of the pot owed to YES, Q64.64.
    pub share: i128,
    pub pool_yes: u64,
    pub pool_no: u64,
    pub fee_total: u64,
}

#[event]
pub struct MarketVoided {
    pub market: Pubkey,
    pub cause: VoidCause,
    pub at: i64,
}

#[event]
pub struct Claimed {
    pub market: Pubkey,
    pub holder: Pubkey,
    pub side_is_yes: bool,
    pub burned: u64,
    pub paid: u64,
}

#[event]
pub struct FeeCollected {
    pub market: Pubkey,
    pub recipient: Pubkey,
    pub amount: u64,
}

#[event]
pub struct DustSwept {
    pub market: Pubkey,
    pub amount: u64,
}

#[event]
pub struct MarketClosed {
    pub market: Pubkey,
    pub rent_returned_to: Pubkey,
}

#[event]
pub struct FeedRegistered {
    pub feed: Pubkey,
    pub pool: Pubkey,
    pub effective_at: i64,
    pub depth_quote: u64,
}

#[event]
pub struct ParamsProposed {
    pub effective_at: i64,
}
