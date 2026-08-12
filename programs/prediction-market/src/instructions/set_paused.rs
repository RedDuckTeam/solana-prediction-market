//! The switch that stops new markets being created.

use anchor_lang::prelude::*;

use crate::instructions::contexts::Governance;

pub fn set_paused(ctx: Context<Governance>, paused: bool) -> Result<()> {
    // Not timelocked, on purpose. This is the lever to pull when the program
    // whose accounts we parse gets upgraded under us, and a delay would defeat
    // the point. It can only stop new markets, never touch existing funds.
    ctx.accounts.config.paused = paused;
    Ok(())
}
