//! Running the predicate over frozen prices and splitting the pot.

use anchor_lang::prelude::*;
use market_core::{apply_ramp, settle as split_pot};
use market_math::Q64;
use market_vm::{verify, EvalContext};

use crate::constants::{seeds, FEE_SHARE_CREATOR_BPS, FEE_SHARE_TREASURY_BPS};
use crate::errors::MarketError;
use crate::events::*;
use crate::hashing::{spec_hash, SyscallHasher};
use crate::state::*;
use crate::utils::*;

#[derive(Accounts)]
pub struct Resolve<'info> {
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
        seeds = [seeds::SNAPSHOT, market.key().as_ref()],
        bump = snapshot.bump,
        has_one = market,
    )]
    pub snapshot: Box<Account<'info, Snapshot>>,

    #[account(mut)]
    pub resolver: Signer<'info>,
}

pub fn resolve(ctx: Context<Resolve>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let market = &ctx.accounts.market;
    // Deliberately no deadline: the snapshot is on chain for good and resolving
    // is a pure function of it, so a late crank returns what an early one would.
    // Expiring it would let a losing side sit out the clock for a refund.
    require!(
        market.status == MarketStatus::Snapshotted,
        MarketError::WrongState
    );

    let spec = &ctx.accounts.spec;
    require!(
        spec_hash(
            &market.market_id,
            market.settle_at,
            market.strike,
            market.ramp_bps,
            &spec.feeds,
            &spec.bytecode,
            &spec.rules_uri,
        ) == market.spec_hash,
        MarketError::SpecHashMismatch
    );

    let inputs: Vec<Q64> = ctx
        .accounts
        .snapshot
        .prices
        .iter()
        .map(|raw| Q64::from_raw(*raw))
        .collect();

    // Paid before the predicate runs, so the crank earns its reward whatever
    // the outcome. A resolution that voids is still work somebody had to do,
    // and if only the happy path paid, an aborting predicate would be settled
    // by nobody but whoever the void happens to favour.
    let reward = ctx.accounts.market.params.keeper_reward;
    pay_from_bond(
        &mut ctx.accounts.market,
        &ctx.accounts.resolver.to_account_info(),
        reward,
    )?;
    let market = &ctx.accounts.market;

    // Re-verified here rather than trusted from creation. It is the only way to
    // get something executable, it costs one linear pass, and it means the
    // proof that the interpreter relies on is established against the very
    // bytes about to be run.
    let program = match verify(&spec.bytecode, spec.feeds.len()) {
        Ok(program) => program,
        Err(_) => return void_market(&mut ctx.accounts.market, VoidCause::PredicateAborted, now),
    };

    // A predicate that aborts -- overflow, division by zero -- voids the market
    // and refunds everyone. It must never resolve to a side: otherwise writing
    // a predicate that aborts on the branch you dislike becomes a strategy.
    let score = match program.execute(&EvalContext {
        inputs: &inputs,
        settle_at: market.settle_at,
        hasher: &SyscallHasher,
    }) {
        Ok(score) => score,
        Err(_) => return void_market(&mut ctx.accounts.market, VoidCause::PredicateAborted, now),
    };

    let share = match apply_ramp(score, Q64::from_raw(market.strike), market.ramp_bps) {
        Ok(share) => share,
        Err(_) => return void_market(&mut ctx.accounts.market, VoidCause::PredicateAborted, now),
    };

    let settlement =
        split_pot(share, market.stakes(), market.params.fee_bps).map_err(MarketError::from)?;

    let treasury_cut = mul_bps(settlement.fee, FEE_SHARE_TREASURY_BPS)?;
    let creator_cut = mul_bps(settlement.fee, FEE_SHARE_CREATOR_BPS)?;
    // Whatever the two floors left behind goes to the keeper, so the three cuts
    // always add back to the fee exactly.
    let keeper_cut = settlement
        .fee
        .checked_sub(treasury_cut)
        .and_then(|rest| rest.checked_sub(creator_cut))
        .ok_or(MarketError::Overflow)?;

    let market_key = market.key();
    let market = &mut ctx.accounts.market;
    market.status = MarketStatus::Resolved;
    market.share = share.raw();
    market.pool_yes = settlement.pool_yes;
    market.pool_no = settlement.pool_no;
    market.fee_total = settlement.fee;
    market.fee_owed_treasury = treasury_cut;
    market.fee_owed_creator = creator_cut;
    market.fee_owed_keeper = keeper_cut;
    market.resolved_at = now;

    emit!(MarketResolved {
        market: market_key,
        resolver: ctx.accounts.resolver.key(),
        share: share.raw(),
        pool_yes: settlement.pool_yes,
        pool_no: settlement.pool_no,
        fee_total: settlement.fee,
    });
    Ok(())
}
