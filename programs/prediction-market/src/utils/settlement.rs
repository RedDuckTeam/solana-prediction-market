//! Steps shared by the instructions that end a market's life.

use anchor_lang::prelude::*;

use crate::errors::MarketError;
use crate::events::*;
use crate::state::*;

/// Refunds everyone at par: each side's pool is exactly what it staked, and no
/// fee is charged.
pub fn void_market(market: &mut Account<Market>, cause: VoidCause, now: i64) -> Result<()> {
    market.status = MarketStatus::Void;
    market.status_reason = cause;
    market.share = 0;
    market.pool_yes = market.staked_yes;
    market.pool_no = market.staked_no;
    market.fee_total = 0;
    market.fee_owed_treasury = 0;
    market.fee_owed_creator = 0;
    market.fee_owed_keeper = 0;
    market.resolved_at = now;

    emit!(MarketVoided {
        market: market.key(),
        cause,
        at: now,
    });
    Ok(())
}

/// Moves lamports out of the market's bond.
///
/// The crank is paid whatever the outcome and never out of the pot. A reward
/// that came from the winnings would leave thin markets with nobody willing to
/// settle them, and a market nobody settles voids by default -- the worst
/// available failure mode.
pub fn pay_from_bond(
    market: &mut Account<Market>,
    recipient: &AccountInfo,
    amount: u64,
) -> Result<()> {
    let available = market.bond_lamports.min(amount);
    if available == 0 {
        return Ok(());
    }
    market.bond_lamports = market
        .bond_lamports
        .checked_sub(available)
        .ok_or(MarketError::Overflow)?;

    let market_info = market.to_account_info();
    **market_info.try_borrow_mut_lamports()? = market_info
        .lamports()
        .checked_sub(available)
        .ok_or(MarketError::Overflow)?;
    **recipient.try_borrow_mut_lamports()? = recipient
        .lamports()
        .checked_add(available)
        .ok_or(MarketError::Overflow)?;
    Ok(())
}

pub fn mul_bps(amount: u64, bps: u64) -> Result<u64> {
    (u128::from(amount) * u128::from(bps) / u128::from(market_core::BPS_DENOMINATOR))
        .try_into()
        .map_err(|_| MarketError::Overflow.into())
}
