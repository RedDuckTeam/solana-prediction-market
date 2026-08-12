//! Mints the protocol will hold on a market's behalf.

use anchor_lang::prelude::*;

/// A collateral mint governance has approved.
///
/// Approval is explicit rather than implicit because a Token-2022 mint carrying
/// a transfer fee would make the vault structurally insolvent: a deposit of `x`
/// arrives as `x - fee` while `x` outcome tokens get minted. Transfer hooks and
/// permanent delegates are refused for the same reason.
#[account]
#[derive(InitSpace)]
pub struct Collateral {
    pub bump: u8,
    pub mint: Pubkey,
    pub decimals: u8,
    pub enabled: bool,
    /// Smallest stake accepted, so that rent on the token accounts stays a
    /// sane fraction of the bet.
    pub min_stake: u64,
}
