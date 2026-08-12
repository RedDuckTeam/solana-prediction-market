//! The price sources governance has admitted, and what kind each is.

use anchor_lang::prelude::*;

/// Both kinds exist because they answer to different assumptions: Raydium is
/// re-derivable by anyone from consensus state but needs a deep, busy pool;
/// Pyth covers far more instruments at the cost of trusting its publishers and
/// the guardians carrying their data.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub enum FeedKind {
    RaydiumClmm,
    PythTwap,
}

/// A price source governance has admitted.
///
/// Markets choose from the registry rather than naming pools directly: pool
/// creation is permissionless and has no liquidity floor, so a creator free to
/// pick sources can stand up two shallow pools and own the median of three.
#[account]
#[derive(InitSpace)]
pub struct Feed {
    pub bump: u8,
    pub kind: FeedKind,
    /// Seeds this account. The ring's address for Raydium, the instrument id
    /// for Pyth -- both 32 bytes, so nothing can be registered twice under two
    /// names.
    pub source_id: [u8; 32],
    /// Raydium only: the pool the ring belongs to.
    pub pool: Pubkey,
    /// Raydium only: the pair's mints, recorded for indexers.
    pub token0_mint: Pubkey,
    pub token1_mint: Pubkey,
    /// Raydium only: how the tick is normalised into a price. Pyth carries its
    /// own exponent, so these are zero there.
    pub token0_decimals: u8,
    pub token1_decimals: u8,
    /// Quote value that moves this pool's price by one percent, attested by
    /// governance rather than measured. `PoolState.liquidity` is in-range `L`,
    /// not depth: a $100 single-tick JIT position inflates it as much as $200k
    /// spread normally. A trusted honest number beats an objective-looking one.
    pub depth_quote: u64,
    /// When this feed becomes usable. Set one timelock into the future.
    pub effective_at: i64,
    pub enabled: bool,
    /// Human-readable pair label, for indexers.
    pub label: [u8; 32],
}

impl Feed {
    pub fn is_active(&self, now: i64) -> bool {
        self.enabled && now >= self.effective_at
    }

    /// A Raydium ring has a fixed address and is pinned outright. A Pyth account
    /// does not exist until someone posts it, so only its contents can be
    /// checked -- hence the window, confidence and missed-slot limits.
    pub fn expects_fixed_source_address(&self) -> bool {
        matches!(self.kind, FeedKind::RaydiumClmm)
    }
}

/// One price input of a market, naming a registry feed and how to read it.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, InitSpace)]
pub struct FeedRef {
    pub feed: Pubkey,
    /// Read the pair the other way round: the price of `token1` in `token0`.
    ///
    /// This is what lets one pool serve as either leg of a composed price, so
    /// three genuinely independent estimates can be built out of pairs that
    /// exist, instead of pretending three deep pools exist for every token.
    pub invert: bool,
}
