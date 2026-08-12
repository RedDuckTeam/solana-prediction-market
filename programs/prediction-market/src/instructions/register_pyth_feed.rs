//! Admitting a Pyth instrument as a price source.

use anchor_lang::prelude::*;

use crate::constants::seeds;
use crate::errors::MarketError;
use crate::events::*;
use crate::state::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct RegisterPythFeedArgs {
    /// The Pyth instrument identifier this feed quotes.
    pub feed_id: [u8; 32],
    pub depth_quote: u64,
    pub label: [u8; 32],
}

/// Admits a Pyth instrument to the registry.
///
/// Unlike a Raydium feed there is nothing to probe: a Pyth price account is
/// created by whoever posts one, so none exists at registration time. Every
/// check that would have gone here happens at settlement instead -- the window,
/// the publisher disagreement, the missed-slot ratio -- against limits this
/// market froze when it was created.
#[derive(Accounts)]
#[instruction(args: RegisterPythFeedArgs)]
pub struct RegisterPythFeed<'info> {
    #[account(
        seeds = [seeds::CONFIG],
        bump = config.bump,
        has_one = authority @ MarketError::NotAuthorized,
    )]
    pub config: Box<Account<'info, Config>>,
    pub authority: Signer<'info>,
    #[account(
        init,
        payer = payer,
        space = 8 + Feed::INIT_SPACE,
        seeds = [seeds::FEED, args.feed_id.as_ref()],
        bump,
    )]
    pub feed: Box<Account<'info, Feed>>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

pub fn register_pyth_feed(
    ctx: Context<RegisterPythFeed>,
    args: RegisterPythFeedArgs,
) -> Result<()> {
    require!(args.depth_quote > 0, MarketError::ParameterOutOfRange);
    require!(args.feed_id != [0u8; 32], MarketError::ParameterOutOfRange);

    let effective_at = Clock::get()?
        .unix_timestamp
        .checked_add(i64::from(ctx.accounts.config.timelock))
        .ok_or(MarketError::Overflow)?;

    let feed = &mut ctx.accounts.feed;
    feed.bump = ctx.bumps.feed;
    feed.kind = FeedKind::PythTwap;
    feed.source_id = args.feed_id;
    feed.pool = Pubkey::default();
    feed.token0_mint = Pubkey::default();
    feed.token1_mint = Pubkey::default();
    feed.token0_decimals = 0;
    feed.token1_decimals = 0;
    feed.depth_quote = args.depth_quote;
    feed.effective_at = effective_at;
    feed.enabled = true;
    feed.label = args.label;

    emit!(FeedRegistered {
        feed: feed.key(),
        pool: Pubkey::default(),
        effective_at,
        depth_quote: args.depth_quote,
    });
    Ok(())
}
