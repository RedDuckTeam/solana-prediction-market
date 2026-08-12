//! Applying a parameter change whose timelock has run out.

use anchor_lang::prelude::*;

use crate::errors::MarketError;
use crate::instructions::contexts::Governance;

pub fn adopt_params(ctx: Context<Governance>) -> Result<()> {
    let config = &mut ctx.accounts.config;
    require!(config.has_pending, MarketError::NothingPending);
    require!(
        Clock::get()?.unix_timestamp >= config.pending_effective_at,
        MarketError::TimelockPending
    );

    config.params = config.pending_params;
    config.has_pending = false;
    config.pending_effective_at = 0;
    Ok(())
}
