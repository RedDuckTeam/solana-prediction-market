//! Re-attesting a feed's depth, or disabling it.

use anchor_lang::prelude::*;

use crate::constants::seeds;
use crate::errors::MarketError;
use crate::state::*;

#[derive(Accounts)]
pub struct UpdateFeed<'info> {
    #[account(
        seeds = [seeds::CONFIG],
        bump = config.bump,
        has_one = authority @ MarketError::NotAuthorized,
    )]
    pub config: Box<Account<'info, Config>>,
    pub authority: Signer<'info>,
    #[account(
        mut,
        seeds = [seeds::FEED, feed.source_id.as_ref()],
        bump = feed.bump,
    )]
    pub feed: Box<Account<'info, Feed>>,
}

pub fn update_feed(ctx: Context<UpdateFeed>, depth_quote: u64, enabled: bool) -> Result<()> {
    require!(depth_quote > 0, MarketError::ParameterOutOfRange);
    // Markets already created keep the cap they were created with, so
    // re-attesting depth can only affect markets made from here on.
    let feed = &mut ctx.accounts.feed;
    feed.depth_quote = depth_quote;
    feed.enabled = enabled;
    Ok(())
}
