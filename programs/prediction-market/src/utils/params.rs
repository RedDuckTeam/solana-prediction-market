//! Validating a governance parameter set.

use anchor_lang::prelude::*;
use market_core::Schedule;

use crate::errors::MarketError;
use crate::state::*;

/// Rejects parameter sets that no market should ever be created under.
///
/// The schedule bounds are not style: they are what the observation ring can
/// physically serve, and what keeps a bettor from placing a wager inside the
/// window that will settle it.
pub fn validate_params(params: &MarketParams) -> Result<()> {
    require!(
        params.fee_bps <= 500 && params.feed_cap_bps <= 2_000 && params.feed_cap_bps > 0,
        MarketError::ParameterOutOfRange
    );
    require!(
        (10..=1_000).contains(&params.min_ramp_bps),
        MarketError::ParameterOutOfRange
    );
    // A tolerance of zero would make every Pyth feed permanently unreadable;
    // a generous one would hand the poster the choice of window back.
    require!(
        (1..=30).contains(&params.pyth_window_tolerance),
        MarketError::ParameterOutOfRange
    );
    require!(
        (1..=1_000).contains(&params.max_confidence_bps),
        MarketError::ParameterOutOfRange
    );
    require!(
        params.max_down_slots_ratio <= 200_000,
        MarketError::ParameterOutOfRange
    );
    require!(params.creation_fee > 0, MarketError::ParameterOutOfRange);
    require!(
        params.claim_window >= 90 * 86_400,
        MarketError::ParameterOutOfRange
    );

    // The snapshot keeper fronts the rent for the `Snapshot` account, which is
    // never closed -- it is the audit trail a resolution is re-derived from.
    // A reward below that rent makes cranking a net loss, and a market nobody
    // cranks voids: the worst failure mode this protocol has. So the floor is
    // not a style choice; it is what keeps the crank worth running.
    let snapshot_rent = Rent::get()?.minimum_balance(8 + Snapshot::INIT_SPACE);
    require!(
        params.keeper_reward >= snapshot_rent,
        MarketError::ParameterOutOfRange
    );

    // Any settlement instant will do here; `validate` only inspects durations.
    let probe = Schedule {
        settle_at: 0,
        twap_window: params.twap_window,
        grace: params.grace,
        skew: params.skew,
        max_segment: params.max_segment,
        min_observations: params.min_observations,
    };
    probe.validate().map_err(MarketError::from)?;
    Ok(())
}
