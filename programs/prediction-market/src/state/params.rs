//! Governance parameters, and the copy each market keeps of them.

use anchor_lang::prelude::*;
use market_core::Schedule;

/// Governance parameters a market copies at creation.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, InitSpace)]
pub struct MarketParams {
    /// Fee on the transferred amount, in basis points.
    pub fee_bps: u16,
    /// Per-side stake cap as a fraction of the thinnest feed's attested depth.
    pub feed_cap_bps: u16,
    /// Narrowest settlement band a market may declare, in basis points.
    pub min_ramp_bps: u16,
    /// Averaging window, seconds.
    pub twap_window: u32,
    /// How long after settlement a snapshot may still be taken, seconds.
    pub grace: u32,
    /// Gap between the betting deadline and the window, seconds.
    pub skew: u32,
    /// Longest tolerated observation gap inside the window, seconds.
    pub max_segment: u32,
    /// Observations that must fall inside the window for a pool feed to count.
    pub min_observations: u16,
    /// Delay between creating a market and opening it for bets, seconds.
    pub creation_cooldown: u32,
    /// How long holders have to claim before dust is swept, seconds.
    pub claim_window: u32,
    /// Seconds either end of a Pyth window may differ from the requested one.
    pub pyth_window_tolerance: u32,
    /// Widest publisher disagreement a market will settle against, in basis
    /// points of the price.
    pub max_confidence_bps: u32,
    /// Largest share of the window that may have fallen in missed slots, out
    /// of 1 000 000.
    pub max_down_slots_ratio: u32,
    /// Paid to each crank, in lamports, out of the creator's bond.
    pub keeper_reward: u64,
    /// Non-refundable, in lamports. The bond alone is not an anti-spam measure:
    /// a creator can crank their own market and get all of it back.
    pub creation_fee: u64,
}

impl MarketParams {
    /// Builds the timetable a market created now would follow.
    pub fn schedule(&self, settle_at: i64) -> Schedule {
        Schedule {
            settle_at,
            twap_window: self.twap_window,
            grace: self.grace,
            skew: self.skew,
            max_segment: self.max_segment,
            min_observations: self.min_observations,
        }
    }
}
