//! Burning a position for what it is owed.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Burn, CloseAccount, Mint, Token, TokenAccount};
use market_core::{payout_for, Settlement, Side};

use crate::constants::seeds;
use crate::errors::MarketError;
use crate::events::*;
use crate::state::*;

#[derive(Accounts)]
pub struct Claim<'info> {
    #[account(
        mut,
        seeds = [seeds::MARKET, market.market_id.as_ref()],
        bump = market.bump,
        has_one = vault,
    )]
    pub market: Box<Account<'info, Market>>,

    #[account(mut)]
    pub vault: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub side_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        constraint = holder_outcome.mint == side_mint.key(),
        constraint = holder_outcome.owner == holder.key(),
    )]
    pub holder_outcome: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = holder_collateral.mint == market.collateral_mint,
        constraint = holder_collateral.owner == holder.key(),
    )]
    pub holder_collateral: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub holder: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

pub fn claim(ctx: Context<Claim>, side_is_yes: bool) -> Result<()> {
    let market = &ctx.accounts.market;
    require!(
        matches!(market.status, MarketStatus::Resolved | MarketStatus::Void),
        MarketError::WrongState
    );
    require!(
        Clock::get()?.unix_timestamp <= market.claim_end(),
        MarketError::ClaimWindowClosed
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

    // The whole position is redeemed at once, which lets the outcome account be
    // closed in the same instruction. Rent on two token accounts is a real cost
    // against a small bet, and leaving it stranded would quietly tax exactly
    // the users the protocol is least able to afford to lose.
    let burned = ctx.accounts.holder_outcome.amount;
    require!(burned > 0, MarketError::NothingToClaim);

    let settlement = Settlement {
        pool_yes: market.pool_yes,
        pool_no: market.pool_no,
        fee: market.fee_total,
    };
    let paid = payout_for(burned, side, market.stakes(), settlement).map_err(MarketError::from)?;

    let market_key = market.key();
    let signer_seeds: &[&[&[u8]]] = &[&[seeds::MARKET, market.market_id.as_ref(), &[market.bump]]];

    token::burn(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            Burn {
                mint: ctx.accounts.side_mint.to_account_info(),
                from: ctx.accounts.holder_outcome.to_account_info(),
                authority: ctx.accounts.holder.to_account_info(),
            },
        ),
        burned,
    )?;

    if paid > 0 {
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.key(),
                token::Transfer {
                    from: ctx.accounts.vault.to_account_info(),
                    to: ctx.accounts.holder_collateral.to_account_info(),
                    authority: ctx.accounts.market.to_account_info(),
                },
                signer_seeds,
            ),
            paid,
        )?;
    }

    token::close_account(CpiContext::new(
        ctx.accounts.token_program.key(),
        CloseAccount {
            account: ctx.accounts.holder_outcome.to_account_info(),
            destination: ctx.accounts.holder.to_account_info(),
            authority: ctx.accounts.holder.to_account_info(),
        },
    ))?;

    emit!(Claimed {
        market: market_key,
        holder: ctx.accounts.holder.key(),
        side_is_yes,
        burned,
        paid,
    });
    Ok(())
}
