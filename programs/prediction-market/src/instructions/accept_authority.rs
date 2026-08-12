//! The other half, which the nominee must sign.

use anchor_lang::prelude::*;

use crate::constants::seeds;
use crate::errors::MarketError;
use crate::state::*;

#[derive(Accounts)]
pub struct AcceptAuthority<'info> {
    #[account(
        mut,
        seeds = [seeds::CONFIG],
        bump = config.bump,
        constraint = config.pending_authority == next_authority.key() @ MarketError::NotAuthorized,
    )]
    pub config: Box<Account<'info, Config>>,
    pub next_authority: Signer<'info>,
}

pub fn accept_authority(ctx: Context<AcceptAuthority>) -> Result<()> {
    // Two-step by design: a mistyped address that never signs simply expires
    // instead of orphaning the protocol.
    let config = &mut ctx.accounts.config;
    config.authority = config.pending_authority;
    config.pending_authority = Pubkey::default();
    Ok(())
}
