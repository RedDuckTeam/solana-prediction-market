//! Account contexts shared by more than one instruction.
//!
//! Kept together so that a change to who may call something lands in one
//! place rather than in four that have to be found first.

use anchor_lang::prelude::*;

use crate::constants::seeds;
use crate::errors::MarketError;
use crate::state::*;

/// Anything only the current authority may do.
#[derive(Accounts)]
pub struct Governance<'info> {
    #[account(
        mut,
        seeds = [seeds::CONFIG],
        bump = config.bump,
        has_one = authority @ MarketError::NotAuthorized,
    )]
    pub config: Box<Account<'info, Config>>,
    pub authority: Signer<'info>,
}
