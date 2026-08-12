//! Admitting a mint the protocol will hold.

use anchor_lang::prelude::*;
use anchor_spl::token::Mint;

use crate::constants::seeds;
use crate::errors::MarketError;
use crate::state::*;

#[derive(Accounts)]
pub struct RegisterCollateral<'info> {
    #[account(
        seeds = [seeds::CONFIG],
        bump = config.bump,
        has_one = authority @ MarketError::NotAuthorized,
    )]
    pub config: Box<Account<'info, Config>>,
    pub authority: Signer<'info>,
    /// Classic SPL mints only.
    ///
    /// `Account<Mint>` refuses anything the legacy Token program does not own,
    /// which rules out Token-2022 entirely. That is deliberate for v1: a
    /// transfer fee would make the vault structurally insolvent, since a
    /// deposit of `x` arrives as `x - fee` while `x` outcome tokens are minted.
    /// Admitting Token-2022 means screening every extension first.
    pub mint: Box<Account<'info, Mint>>,
    #[account(
        init,
        payer = payer,
        space = 8 + Collateral::INIT_SPACE,
        seeds = [seeds::COLLATERAL, mint.key().as_ref()],
        bump,
    )]
    pub collateral: Box<Account<'info, Collateral>>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

pub fn register_collateral(ctx: Context<RegisterCollateral>, min_stake: u64) -> Result<()> {
    require!(min_stake > 0, MarketError::ParameterOutOfRange);
    let collateral = &mut ctx.accounts.collateral;
    collateral.bump = ctx.bumps.collateral;
    collateral.mint = ctx.accounts.mint.key();
    collateral.decimals = ctx.accounts.mint.decimals;
    collateral.enabled = true;
    collateral.min_stake = min_stake;
    Ok(())
}
