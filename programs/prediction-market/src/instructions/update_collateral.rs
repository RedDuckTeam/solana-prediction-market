//! Adjusting or withdrawing a collateral mint.

use anchor_lang::prelude::*;

use crate::constants::seeds;
use crate::errors::MarketError;
use crate::state::*;

#[derive(Accounts)]
pub struct UpdateCollateral<'info> {
    #[account(
        seeds = [seeds::CONFIG],
        bump = config.bump,
        has_one = authority @ MarketError::NotAuthorized,
    )]
    pub config: Box<Account<'info, Config>>,
    pub authority: Signer<'info>,
    #[account(
        mut,
        seeds = [seeds::COLLATERAL, collateral.mint.as_ref()],
        bump = collateral.bump,
    )]
    pub collateral: Box<Account<'info, Collateral>>,
}

/// Turns a collateral mint off, or adjusts its floor.
///
/// Disabling stops new markets from being created against it. Markets already
/// holding it settle and pay out normally -- withdrawing collateral approval
/// must never strand funds that are already deposited.
pub fn update_collateral(
    ctx: Context<UpdateCollateral>,
    min_stake: u64,
    enabled: bool,
) -> Result<()> {
    require!(min_stake > 0, MarketError::ParameterOutOfRange);
    let collateral = &mut ctx.accounts.collateral;
    collateral.min_stake = min_stake;
    collateral.enabled = enabled;
    Ok(())
}
