//! Half of a two-step authority handover.

use anchor_lang::prelude::*;

use crate::instructions::contexts::Governance;

pub fn nominate_authority(ctx: Context<Governance>, next: Pubkey) -> Result<()> {
    ctx.accounts.config.pending_authority = next;
    Ok(())
}
