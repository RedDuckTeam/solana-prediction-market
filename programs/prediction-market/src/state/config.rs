//! Protocol-wide settings and the governance that changes them.

use anchor_lang::prelude::*;

use crate::state::MarketParams;

/// Protocol-wide settings.
#[account]
#[derive(InitSpace)]
pub struct Config {
    pub bump: u8,
    /// Governance. Changes to it are two-step, so a typo cannot orphan the
    /// protocol.
    pub authority: Pubkey,
    pub pending_authority: Pubkey,
    pub treasury: Pubkey,
    /// Kill switch for new markets.
    ///
    /// Exists because the price source is a program someone else can upgrade:
    /// Raydium's CLMM sits behind a multisig, and if its account layout or its
    /// accumulation rule ever changes, this stops new markets from being
    /// created against it while existing ones settle out.
    pub paused: bool,
    pub params: MarketParams,
    /// Parameters waiting out the timelock, and when they become effective.
    pub pending_params: MarketParams,
    pub pending_effective_at: i64,
    pub has_pending: bool,
    /// How long parameter and feed changes must wait, seconds.
    pub timelock: u32,
    /// The Raydium CLMM program whose pools this deployment reads.
    ///
    /// Configured rather than hard-coded: it is an address that differs between
    /// clusters and could be redeployed, and burying it in a constant would
    /// make a wrong value a redeploy instead of a governance action.
    pub raydium_clmm_program: Pubkey,
    /// The Pyth receiver program whose price accounts this deployment reads.
    pub pyth_receiver_program: Pubkey,
    /// Markets created so far, for reporting only.
    pub markets_created: u64,
}
