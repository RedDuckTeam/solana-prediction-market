//! Queueing a parameter change behind the timelock.

use anchor_lang::prelude::*;

use crate::errors::MarketError;
use crate::events::*;
use crate::instructions::contexts::Governance;
use crate::state::*;
use crate::utils::*;

pub fn propose_params(ctx: Context<Governance>, params: MarketParams) -> Result<()> {
    validate_params(&params)?;
    let config = &mut ctx.accounts.config;
    let effective_at = Clock::get()?
        .unix_timestamp
        .checked_add(i64::from(config.timelock))
        .ok_or(MarketError::Overflow)?;

    config.pending_params = params;
    config.pending_effective_at = effective_at;
    config.has_pending = true;

    emit!(ParamsProposed { effective_at });
    Ok(())
}
