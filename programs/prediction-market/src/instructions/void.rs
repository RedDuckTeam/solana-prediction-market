//! Abandoning a market that cannot settle honestly.

use anchor_lang::prelude::*;

use crate::constants::seeds;
use crate::errors::MarketError;
use crate::state::*;
use crate::utils::*;

#[derive(Accounts)]
pub struct VoidMarket<'info> {
    #[account(
        mut,
        seeds = [seeds::MARKET, market.market_id.as_ref()],
        bump = market.bump,
    )]
    pub market: Box<Account<'info, Market>>,

    pub caller: Signer<'info>,
}

pub fn void(ctx: Context<VoidMarket>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let market = &ctx.accounts.market;
    let schedule = market.schedule();

    // The cause is derived, never supplied. Every one of these is decided by
    // chain state at or before settlement, so none can be brought about after
    // the outcome is known -- which matters, because a losing position bought
    // cheaply after settlement would otherwise be worth par under a void.
    let cause = if matches!(market.status, MarketStatus::Created | MarketStatus::Open)
        && now >= schedule.lock_at()
        && (market.staked_yes == 0 || market.staked_no == 0)
    {
        VoidCause::EmptySide
    } else if market.status == MarketStatus::Open && schedule.snapshot_missed(now) {
        // Recorded when no snapshot arrived in time, whatever stopped it: an
        // idle keeper and a feed that could not be read look identical from
        // here, and the ring that could have told them apart has long been
        // overwritten by the time anyone asks.
        VoidCause::SnapshotMissed
    } else {
        return err!(MarketError::VoidConditionNotMet);
    };

    // The bond is not forfeited, whatever the cause. It exists to pay the
    // cranks, not to punish: the causes that reach here are either nobody's
    // fault (an empty side, an unreadable feed) or not the creator's (cranks
    // are permissionless, and governance disabling a feed can block them
    // outright). Whatever the cranks did not spend goes back with the rent at
    // `close_market`; the spam deterrent is the non-refundable creation fee.
    void_market(&mut ctx.accounts.market, cause, now)
}
