//! Closing a settled, emptied market and returning its rent.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, CloseAccount, Token, TokenAccount};

use crate::constants::seeds;
use crate::errors::MarketError;
use crate::events::*;
use crate::state::*;

#[derive(Accounts)]
pub struct CloseMarket<'info> {
    #[account(
        mut,
        seeds = [seeds::MARKET, market.market_id.as_ref()],
        bump = market.bump,
        has_one = vault,
        has_one = creator,
        close = creator,
    )]
    pub market: Box<Account<'info, Market>>,

    #[account(
        mut,
        seeds = [seeds::SPEC, market.key().as_ref()],
        bump = spec.bump,
        has_one = market,
        close = creator,
    )]
    pub spec: Box<Account<'info, MarketSpec>>,

    #[account(mut, constraint = vault.amount == 0 @ MarketError::VaultNotEmpty)]
    pub vault: Box<Account<'info, TokenAccount>>,

    /// CHECK: receives the reclaimed rent; bound by `has_one = creator`.
    #[account(mut)]
    pub creator: UncheckedAccount<'info>,

    pub caller: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

pub fn close_market(ctx: Context<CloseMarket>) -> Result<()> {
    let market = &ctx.accounts.market;
    require!(market.status.is_settled(), MarketError::WrongState);

    // Not conditioned on the outcome mints' supplies: holders of a worthless
    // side never burn it, so waiting for zero would strand the rent. The mints
    // themselves cannot be closed at all — the legacy Token program has no
    // instruction for it, so ~0.003 SOL per market is the price of using the
    // standard every wallet already supports.
    let signer_seeds: &[&[&[u8]]] = &[&[seeds::MARKET, market.market_id.as_ref(), &[market.bump]]];
    token::close_account(CpiContext::new_with_signer(
        ctx.accounts.token_program.key(),
        CloseAccount {
            account: ctx.accounts.vault.to_account_info(),
            destination: ctx.accounts.creator.to_account_info(),
            authority: ctx.accounts.market.to_account_info(),
        },
        signer_seeds,
    ))?;

    emit!(MarketClosed {
        market: market.key(),
        rent_returned_to: ctx.accounts.creator.key(),
    });
    Ok(())
}
