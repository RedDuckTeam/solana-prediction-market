//! Freezing every declared price for the settlement window.

use anchor_lang::prelude::*;
use market_core::Schedule;
use market_feeds::{
    invert_price, normalized_price, pyth_twap, raydium_twap, ClmmLimits, PythLimits,
};
use market_math::Q64;

use crate::constants::seeds;
use crate::errors::MarketError;
use crate::events::*;
use crate::hashing::readings_hash;
use crate::state::*;
use crate::utils::*;

#[derive(Accounts)]
pub struct TakeSnapshot<'info> {
    #[account(seeds = [seeds::CONFIG], bump = config.bump)]
    pub config: Box<Account<'info, Config>>,

    #[account(
        mut,
        seeds = [seeds::MARKET, market.market_id.as_ref()],
        bump = market.bump,
    )]
    pub market: Box<Account<'info, Market>>,

    #[account(
        seeds = [seeds::SPEC, market.key().as_ref()],
        bump = spec.bump,
        has_one = market,
    )]
    pub spec: Box<Account<'info, MarketSpec>>,

    #[account(
        init,
        payer = keeper,
        space = 8 + Snapshot::INIT_SPACE,
        seeds = [seeds::SNAPSHOT, market.key().as_ref()],
        bump,
    )]
    pub snapshot: Box<Account<'info, Snapshot>>,

    #[account(mut)]
    pub keeper: Signer<'info>,
    pub system_program: Program<'info, System>,
    // `remaining_accounts` holds, per declared feed and in order, its `Feed`
    // account followed by that feed's observation ring.
}

pub fn snapshot(ctx: Context<TakeSnapshot>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let market = &ctx.accounts.market;
    let schedule = market.schedule();

    require!(market.status == MarketStatus::Open, MarketError::WrongState);
    require!(
        schedule.snapshot_open(now),
        MarketError::OutsideSnapshotWindow
    );
    // With one side empty there is no counterparty and nothing to settle; that
    // market has to be voided instead, and the check lives here so a keeper
    // cannot waste a snapshot on it.
    require!(
        market.staked_yes > 0 && market.staked_no > 0,
        MarketError::WrongState
    );

    let feeds = &ctx.accounts.spec.feeds;
    require!(
        ctx.remaining_accounts.len() == feeds.len() * 2,
        MarketError::FeedAccountsMismatch
    );

    let mut prices: Vec<i128> = Vec::with_capacity(feeds.len());
    let mut readings: Vec<ReadingRecord> = Vec::with_capacity(feeds.len());

    for (position, declared) in feeds.iter().enumerate() {
        let feed_account = &ctx.remaining_accounts[position * 2];
        let ring_account = &ctx.remaining_accounts[position * 2 + 1];

        require_keys_eq!(
            feed_account.key(),
            declared.feed,
            MarketError::FeedAccountsMismatch
        );
        let feed: Account<Feed> = Account::try_from(feed_account)?;
        require!(feed.is_active(now), MarketError::FeedNotActive);

        let price = match feed.kind {
            FeedKind::RaydiumClmm => {
                // The ring lives at a fixed address, so it is pinned outright.
                require!(
                    ring_account.key().to_bytes() == feed.source_id,
                    MarketError::FeedAccountsMismatch
                );
                require_keys_eq!(
                    *ring_account.owner,
                    ctx.accounts.config.raydium_clmm_program,
                    MarketError::FeedPoolMismatch
                );
                read_clmm_feed(
                    declared.feed,
                    &feed,
                    declared.invert,
                    &schedule,
                    ring_account,
                    &mut readings,
                )?
            }
            FeedKind::PythTwap => {
                // A Pyth account does not exist until someone posts one, so its
                // address cannot be pinned -- only its owner and its contents.
                require_keys_eq!(
                    *ring_account.owner,
                    ctx.accounts.config.pyth_receiver_program,
                    MarketError::FeedPoolMismatch
                );
                read_pyth_feed(
                    declared.feed,
                    &feed,
                    declared.invert,
                    &schedule,
                    &market.params,
                    ring_account,
                    &mut readings,
                )?
            }
        };
        prices.push(price.raw());
    }

    let feeds_hash = readings_hash(&readings);
    let market_key = market.key();

    let snapshot = &mut ctx.accounts.snapshot;
    snapshot.bump = ctx.bumps.snapshot;
    snapshot.market = market_key;
    snapshot.taken_at = now;
    snapshot.keeper = ctx.accounts.keeper.key();
    snapshot.feeds_hash = feeds_hash;
    snapshot.prices = prices.clone();
    snapshot.readings = readings;

    let reward = ctx.accounts.market.params.keeper_reward;
    pay_from_bond(
        &mut ctx.accounts.market,
        &ctx.accounts.keeper.to_account_info(),
        reward,
    )?;

    let market = &mut ctx.accounts.market;
    market.status = MarketStatus::Snapshotted;
    market.snapshot_keeper = ctx.accounts.keeper.key();

    emit!(SnapshotTaken {
        market: market_key,
        keeper: market.snapshot_keeper,
        taken_at: now,
        feeds_hash,
        prices,
    });
    Ok(())
}

/// Reads a Raydium feed over the market's window and records how it was read.
fn read_clmm_feed(
    feed_key: Pubkey,
    feed: &Feed,
    invert: bool,
    schedule: &Schedule,
    ring_account: &AccountInfo,
    readings: &mut Vec<ReadingRecord>,
) -> Result<Q64> {
    let data = ring_account.try_borrow_data()?;
    let reading = raydium_twap(
        &data,
        &feed.pool.to_bytes(),
        schedule.window_start(),
        schedule.settle_at,
        ClmmLimits {
            max_segment: schedule.max_segment,
            min_observations: schedule.min_observations,
        },
    )
    .map_err(MarketError::from)?;
    drop(data);

    let price = normalized_price(
        reading.average_tick,
        feed.token0_decimals,
        feed.token1_decimals,
        invert,
    )
    .map_err(MarketError::from)?;

    let record = |boundary: market_feeds::Boundary| BoundaryRecord {
        index: boundary.index,
        observed_at: boundary.observed_at,
        cumulative: boundary.cumulative,
        next_index: boundary.next_index,
        next_observed_at: boundary.next_observed_at,
        next_cumulative: boundary.next_cumulative,
        interpolated: boundary.interpolated,
    };
    readings.push(ReadingRecord {
        feed: feed_key,
        source: SourceRecord::RaydiumClmm {
            average_tick: reading.average_tick,
            window_start: record(reading.start),
            window_end: record(reading.end),
        },
    });
    Ok(price)
}

/// Reads a Pyth feed, questioning everything the poster could have chosen.
fn read_pyth_feed(
    feed_key: Pubkey,
    feed: &Feed,
    invert: bool,
    schedule: &Schedule,
    params: &MarketParams,
    account: &AccountInfo,
    readings: &mut Vec<ReadingRecord>,
) -> Result<Q64> {
    let data = account.try_borrow_data()?;
    let reading = pyth_twap(
        &data,
        &feed.source_id,
        schedule.window_start(),
        schedule.settle_at,
        PythLimits {
            window_tolerance: params.pyth_window_tolerance,
            max_confidence_bps: params.max_confidence_bps,
            max_down_slots_ratio: params.max_down_slots_ratio,
        },
    )
    .map_err(MarketError::from)?;
    drop(data);

    // Band-checked, exactly as the Raydium path is inside `normalized_price`:
    // a price near the band's floor inverts to one past its ceiling.
    let price = if invert {
        invert_price(reading.price).map_err(MarketError::from)?
    } else {
        reading.price
    };

    readings.push(ReadingRecord {
        feed: feed_key,
        source: SourceRecord::PythTwap {
            raw_price: reading.raw_price,
            raw_conf: reading.raw_conf,
            exponent: reading.exponent,
            confidence_bps: reading.confidence_bps,
            down_slots_ratio: reading.down_slots_ratio,
            start_time: reading.start_time,
            end_time: reading.end_time,
        },
    });
    Ok(price)
}
