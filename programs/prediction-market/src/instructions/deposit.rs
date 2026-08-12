//! Staking on a side, and minting the position that says so.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, MintTo, Token, TokenAccount};
use market_core::Side;

use crate::constants::seeds;
use crate::errors::MarketError;
use crate::events::*;
use crate::state::*;

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(
        mut,
        seeds = [seeds::MARKET, market.market_id.as_ref()],
        bump = market.bump,
        has_one = vault,
        has_one = collateral_mint,
    )]
    pub market: Box<Account<'info, Market>>,

    #[account(
        seeds = [seeds::COLLATERAL, collateral_mint.key().as_ref()],
        bump = collateral.bump,
        constraint = collateral.enabled @ MarketError::CollateralNotRegistered,
    )]
    pub collateral: Box<Account<'info, Collateral>>,
    pub collateral_mint: Box<Account<'info, Mint>>,

    #[account(mut)]
    pub vault: Box<Account<'info, TokenAccount>>,

    /// The side being staked. Checked against the market in the handler so a
    /// single context serves both sides.
    #[account(mut)]
    pub side_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        constraint = depositor_collateral.mint == collateral_mint.key(),
        constraint = depositor_collateral.owner == depositor.key(),
    )]
    pub depositor_collateral: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = depositor_outcome.mint == side_mint.key(),
        constraint = depositor_outcome.owner == depositor.key(),
    )]
    pub depositor_outcome: Box<Account<'info, TokenAccount>>,

    pub depositor: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

pub fn deposit(ctx: Context<Deposit>, side_is_yes: bool, amount: u64) -> Result<()> {
    require!(amount > 0, MarketError::ZeroAmount);
    require!(
        amount >= ctx.accounts.collateral.min_stake,
        MarketError::BelowMinimumStake
    );

    let now = Clock::get()?.unix_timestamp;
    let market = &ctx.accounts.market;
    require!(
        matches!(market.status, MarketStatus::Created | MarketStatus::Open),
        MarketError::WrongState
    );
    require!(
        market.schedule().deposits_open(now, market.open_at),
        MarketError::DepositsClosed
    );

    let side = if side_is_yes { Side::Yes } else { Side::No };
    let expected_mint = if side_is_yes {
        market.yes_mint
    } else {
        market.no_mint
    };
    require_keys_eq!(
        ctx.accounts.side_mint.key(),
        expected_mint,
        MarketError::WrongState
    );

    // The cap is per side. Capping the total instead would let one account fill
    // it from one side, blocking every later bet and forcing a void that hands
    // the blocker a full refund.
    let updated = market
        .stakes()
        .deposit(side, amount, market.cap_per_side)
        .map_err(MarketError::from)?;

    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            token::Transfer {
                from: ctx.accounts.depositor_collateral.to_account_info(),
                to: ctx.accounts.vault.to_account_info(),
                authority: ctx.accounts.depositor.to_account_info(),
            },
        ),
        amount,
    )?;

    let market_key = market.key();
    let signer_seeds: &[&[&[u8]]] = &[&[seeds::MARKET, market.market_id.as_ref(), &[market.bump]]];
    token::mint_to(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            MintTo {
                mint: ctx.accounts.side_mint.to_account_info(),
                to: ctx.accounts.depositor_outcome.to_account_info(),
                authority: ctx.accounts.market.to_account_info(),
            },
            signer_seeds,
        ),
        amount,
    )?;

    let market = &mut ctx.accounts.market;
    market.staked_yes = updated.yes;
    market.staked_no = updated.no;
    market.status = MarketStatus::Open;

    emit!(Deposited {
        market: market_key,
        depositor: ctx.accounts.depositor.key(),
        side_is_yes,
        amount,
        staked_yes: updated.yes,
        staked_no: updated.no,
    });
    Ok(())
}
