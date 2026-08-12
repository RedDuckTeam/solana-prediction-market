//! Pushing a fee share to the party it belongs to.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount};

use crate::constants::seeds;
use crate::errors::MarketError;
use crate::events::*;
use crate::state::*;

/// Which of the three fee cuts is being collected.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeeRecipient {
    Treasury,
    Creator,
    /// The keeper that took the snapshot.
    Keeper,
}

#[derive(Accounts)]
pub struct CollectFee<'info> {
    #[account(seeds = [seeds::CONFIG], bump = config.bump)]
    pub config: Box<Account<'info, Config>>,

    #[account(
        mut,
        seeds = [seeds::MARKET, market.market_id.as_ref()],
        bump = market.bump,
        has_one = vault,
    )]
    pub market: Box<Account<'info, Market>>,

    #[account(mut)]
    pub vault: Box<Account<'info, TokenAccount>>,

    /// Must be owned by whoever the chosen cut belongs to.
    #[account(mut, constraint = destination.mint == market.collateral_mint)]
    pub destination: Box<Account<'info, TokenAccount>>,

    pub caller: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

pub fn collect_fee(ctx: Context<CollectFee>, recipient: FeeRecipient) -> Result<()> {
    let market = &ctx.accounts.market;
    require!(
        market.status == MarketStatus::Resolved,
        MarketError::WrongState
    );

    // Permissionless to call, but the destination must belong to the party the
    // cut is owed to -- so anyone may push a payout, nobody may redirect one.
    let (owed, owner) = match recipient {
        FeeRecipient::Treasury => (market.fee_owed_treasury, ctx.accounts.config.treasury),
        FeeRecipient::Creator => (market.fee_owed_creator, market.creator),
        FeeRecipient::Keeper => (market.fee_owed_keeper, market.snapshot_keeper),
    };
    require!(owed > 0, MarketError::NothingToClaim);
    require_keys_eq!(
        ctx.accounts.destination.owner,
        owner,
        MarketError::NotAuthorized
    );

    let signer_seeds: &[&[&[u8]]] = &[&[seeds::MARKET, market.market_id.as_ref(), &[market.bump]]];
    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            token::Transfer {
                from: ctx.accounts.vault.to_account_info(),
                to: ctx.accounts.destination.to_account_info(),
                authority: ctx.accounts.market.to_account_info(),
            },
            signer_seeds,
        ),
        owed,
    )?;

    let market_key = market.key();
    let market = &mut ctx.accounts.market;
    match recipient {
        FeeRecipient::Treasury => market.fee_owed_treasury = 0,
        FeeRecipient::Creator => market.fee_owed_creator = 0,
        FeeRecipient::Keeper => market.fee_owed_keeper = 0,
    }

    emit!(FeeCollected {
        market: market_key,
        recipient: owner,
        amount: owed,
    });
    Ok(())
}
