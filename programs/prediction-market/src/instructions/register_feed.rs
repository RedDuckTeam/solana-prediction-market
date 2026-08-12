//! Admitting a pool as a price source, by reading it.

use anchor_lang::prelude::*;
use market_feeds::{raydium_twap, read_pool_state};

use crate::constants::seeds;
use crate::errors::MarketError;
use crate::events::*;
use crate::state::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct RegisterFeedArgs {
    pub depth_quote: u64,
    pub label: [u8; 32],
    /// A window the ring must be able to serve right now.
    ///
    /// Registration proves the feed works by actually reading it, rather than
    /// asserting that it will. A pool that cannot answer a probe today will not
    /// answer a settlement tomorrow.
    pub probe_from: i64,
    pub probe_to: i64,
    pub probe_max_segment: u32,
    pub probe_min_observations: u16,
}

#[derive(Accounts)]
pub struct RegisterFeed<'info> {
    #[account(
        seeds = [seeds::CONFIG],
        bump = config.bump,
        has_one = authority @ MarketError::NotAuthorized,
    )]
    pub config: Box<Account<'info, Config>>,
    pub authority: Signer<'info>,

    /// CHECK: validated against `config.raydium_clmm_program` and parsed by
    /// `market_feeds`, which checks the discriminator. The pair's mints and
    /// decimals are read out of it rather than taken as arguments: a
    /// transposed pair or a wrong decimal count is a price normalised upside
    /// down, discovered only when a market settles wrong.
    #[account(owner = config.raydium_clmm_program @ MarketError::FeedPoolMismatch)]
    pub pool: UncheckedAccount<'info>,

    /// CHECK: parsed by `market_feeds`, which verifies the discriminator and
    /// that `pool_id` names the pool above; the pool must name it back.
    #[account(owner = config.raydium_clmm_program @ MarketError::FeedPoolMismatch)]
    pub observation_state: UncheckedAccount<'info>,

    #[account(
        init,
        payer = payer,
        space = 8 + Feed::INIT_SPACE,
        seeds = [seeds::FEED, observation_state.key().as_ref()],
        bump,
    )]
    pub feed: Box<Account<'info, Feed>>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

pub fn register_feed(ctx: Context<RegisterFeed>, args: RegisterFeedArgs) -> Result<()> {
    require!(args.depth_quote > 0, MarketError::ParameterOutOfRange);

    let pool = ctx.accounts.pool.key();
    let pool_data = ctx.accounts.pool.try_borrow_data()?;
    let pool_info = read_pool_state(&pool_data).map_err(MarketError::from)?;
    drop(pool_data);

    // The binding runs both ways: the ring names its pool (checked inside the
    // probe below) and the pool names its ring. Either alone would let a feed
    // pair one pool's identity with another pool's prices.
    require!(
        pool_info.observation_key == ctx.accounts.observation_state.key().to_bytes(),
        MarketError::FeedPoolMismatch
    );

    // Read it for real. This exercises the discriminator, the pool binding, the
    // fully-populated ring, timestamp monotonicity, and the per-segment
    // consistency canary in one call.
    let data = ctx.accounts.observation_state.try_borrow_data()?;
    raydium_twap(
        &data,
        &pool.to_bytes(),
        args.probe_from,
        args.probe_to,
        market_feeds::ClmmLimits {
            max_segment: args.probe_max_segment,
            min_observations: args.probe_min_observations,
        },
    )
    .map_err(MarketError::from)?;
    drop(data);

    let effective_at = Clock::get()?
        .unix_timestamp
        .checked_add(i64::from(ctx.accounts.config.timelock))
        .ok_or(MarketError::Overflow)?;

    let feed = &mut ctx.accounts.feed;
    feed.bump = ctx.bumps.feed;
    feed.kind = FeedKind::RaydiumClmm;
    feed.source_id = ctx.accounts.observation_state.key().to_bytes();
    feed.pool = pool;
    feed.token0_mint = Pubkey::new_from_array(pool_info.mint0);
    feed.token1_mint = Pubkey::new_from_array(pool_info.mint1);
    feed.token0_decimals = pool_info.mint_decimals0;
    feed.token1_decimals = pool_info.mint_decimals1;
    feed.depth_quote = args.depth_quote;
    feed.effective_at = effective_at;
    feed.enabled = true;
    feed.label = args.label;

    emit!(FeedRegistered {
        feed: feed.key(),
        pool,
        effective_at,
        depth_quote: args.depth_quote,
    });
    Ok(())
}
