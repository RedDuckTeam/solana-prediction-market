//! Sweeping what rounding and inattention left behind.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount};

use crate::constants::seeds;
use crate::errors::MarketError;
use crate::events::*;
use crate::state::*;

#[derive(Accounts)]
pub struct SweepDust<'info> {
    #[account(seeds = [seeds::CONFIG], bump = config.bump)]
    pub config: Box<Account<'info, Config>>,

    #[account(
        seeds = [seeds::MARKET, market.market_id.as_ref()],
        bump = market.bump,
        has_one = vault,
    )]
    pub market: Box<Account<'info, Market>>,

    #[account(mut)]
    pub vault: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = treasury_token.mint == market.collateral_mint,
        constraint = treasury_token.owner == config.treasury @ MarketError::NotAuthorized,
    )]
    pub treasury_token: Box<Account<'info, TokenAccount>>,

    pub caller: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

pub fn sweep_dust(ctx: Context<SweepDust>) -> Result<()> {
    let market = &ctx.accounts.market;
    require!(market.status.is_settled(), MarketError::WrongState);
    require!(
        Clock::get()?.unix_timestamp > market.claim_end(),
        MarketError::ClaimWindowOpen
    );

    // Whatever the flooring left behind, plus anything never claimed, plus any
    // stray transfer someone made into the vault. The balance is read, never
    // compared against an expected figure: a one-lamport donation must not be
    // able to brick settlement.
    let remaining = ctx.accounts.vault.amount;
    require!(remaining > 0, MarketError::NothingToClaim);

    let signer_seeds: &[&[&[u8]]] = &[&[seeds::MARKET, market.market_id.as_ref(), &[market.bump]]];
    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            token::Transfer {
                from: ctx.accounts.vault.to_account_info(),
                to: ctx.accounts.treasury_token.to_account_info(),
                authority: ctx.accounts.market.to_account_info(),
            },
            signer_seeds,
        ),
        remaining,
    )?;

    emit!(DustSwept {
        market: market.key(),
        amount: remaining,
    });
    Ok(())
}
