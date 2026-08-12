//! A market: the immutable question, and the mutable state answering it.

use crate::state::{FeedRef, MarketParams};
use anchor_lang::prelude::*;
use market_core::Schedule;
use market_vm::{MAX_CODE_LEN, MAX_INPUTS};

/// Longest human-readable rules link a market may carry.
pub const MAX_RULES_URI_LEN: usize = 200;

/// The immutable half of a market: what it measures and how it decides.
#[account]
#[derive(InitSpace)]
pub struct MarketSpec {
    pub bump: u8,
    pub market: Pubkey,
    #[max_len(MAX_INPUTS)]
    pub feeds: Vec<FeedRef>,
    #[max_len(MAX_CODE_LEN)]
    pub bytecode: Vec<u8>,
    #[max_len(MAX_RULES_URI_LEN)]
    pub rules_uri: String,
}

/// Where a market is in its life. Mirrors `market_core::MarketState`.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub enum MarketStatus {
    Created,
    Open,
    Locked,
    Snapshotted,
    Resolved,
    Void,
}

impl MarketStatus {
    /// Whether the market has reached a final state and money may move out.
    pub fn is_settled(self) -> bool {
        matches!(self, MarketStatus::Resolved | MarketStatus::Void)
    }
}

/// Why a market was abandoned. Mirrors `market_core::VoidReason`.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub enum VoidCause {
    None,
    SnapshotMissed,
    EmptySide,
    PredicateAborted,
}

/// The mutable half of a market.
#[account]
#[derive(InitSpace)]
pub struct Market {
    pub bump: u8,
    pub status: MarketStatus,
    /// Caller-chosen identifier, so the address is known before the
    /// transaction is sent and two markets may share a spec.
    pub market_id: [u8; 32],
    /// Commits to the spec for good. Resolution checks it, so swapping the
    /// spec account for another cannot change how a market settles.
    pub spec_hash: [u8; 32],
    pub creator: Pubkey,
    pub collateral_mint: Pubkey,
    pub yes_mint: Pubkey,
    pub no_mint: Pubkey,
    pub vault: Pubkey,

    pub created_at: i64,
    pub open_at: i64,
    pub settle_at: i64,
    /// Copied from `Config` at creation and never revisited.
    pub params: MarketParams,
    /// Per-side stake ceiling, in collateral base units.
    pub cap_per_side: u64,

    /// The value the predicate's score is compared against, Q64.64.
    pub strike: i128,
    /// Half-width of the settlement band, in basis points of the strike.
    ///
    /// Applied by the program, never written into the predicate: a band buried
    /// in bytecode could not be checked, so a creator could publish a step
    /// function. Here every market has one, at least as wide as governance asks.
    pub ramp_bps: u16,

    /// Authoritative stake counters.
    ///
    /// Not the outcome mints' supplies: any holder can burn their own SPL
    /// tokens through the Token program, which would shrink a supply while the
    /// collateral stayed in the vault. Payout arithmetic anchored to supply
    /// would then overpay whoever claimed last.
    pub staked_yes: u64,
    pub staked_no: u64,

    pub status_reason: VoidCause,
    /// Fraction of the pot owed to YES, Q64.64, fixed at resolution.
    pub share: i128,
    pub pool_yes: u64,
    pub pool_no: u64,
    pub fee_total: u64,
    pub fee_owed_treasury: u64,
    pub fee_owed_creator: u64,
    pub fee_owed_keeper: u64,
    pub snapshot_keeper: Pubkey,
    pub resolved_at: i64,
    /// Refundable lamports held for the cranks.
    pub bond_lamports: u64,
}

impl Market {
    pub fn schedule(&self) -> Schedule {
        self.params.schedule(self.settle_at)
    }

    /// When claiming closes and dust may be swept.
    pub fn claim_end(&self) -> i64 {
        self.resolved_at + i64::from(self.params.claim_window)
    }

    pub fn stakes(&self) -> market_core::Stakes {
        market_core::Stakes {
            yes: self.staked_yes,
            no: self.staked_no,
        }
    }
}
