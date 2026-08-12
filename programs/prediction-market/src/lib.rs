//! Prediction markets that settle from on-chain price history.
//!
//! A market asks whether a token's time-weighted price clears a strike at a
//! stated instant, and settles from Raydium observation rings or signed Pyth
//! updates with no privileged reporter in the path.
//!
//! This crate is glue: accounts, authority, and plain numbers in and out of the
//! pure crates that hold every rule. No arithmetic that decides money is here.

#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;

pub mod constants;
pub mod errors;
pub mod events;
pub mod hashing;
pub mod instructions;
pub mod state;
pub mod utils;

#[cfg(test)]
mod invariants;

use instructions::*;

declare_id!("CDJdcKyxiBHDKbyLMTi9988Bw2Vu1i9FnqRcQp7dJreD");

#[program]
pub mod prediction_market {
    use super::*;

    // -- Governance ----------------------------------------------------

    /// One-time setup. The deployer becomes the first authority and is
    /// expected to hand it to a multisig immediately.
    pub fn initialize_config(
        ctx: Context<InitializeConfig>,
        args: InitializeConfigArgs,
    ) -> Result<()> {
        instructions::initialize_config(ctx, args)
    }

    /// Queues a parameter change. It takes effect only after the timelock, and
    /// only for markets created after that.
    pub fn propose_params(ctx: Context<Governance>, params: state::MarketParams) -> Result<()> {
        instructions::propose_params(ctx, params)
    }

    /// Applies a queued parameter change whose timelock has run out.
    pub fn adopt_params(ctx: Context<Governance>) -> Result<()> {
        instructions::adopt_params(ctx)
    }

    /// Stops new markets from being created. Existing markets settle normally.
    pub fn set_paused(ctx: Context<Governance>, paused: bool) -> Result<()> {
        instructions::set_paused(ctx, paused)
    }

    /// Nominates a new authority; it must accept before anything changes.
    pub fn nominate_authority(ctx: Context<Governance>, next: Pubkey) -> Result<()> {
        instructions::nominate_authority(ctx, next)
    }

    /// Accepts a nomination.
    pub fn accept_authority(ctx: Context<AcceptAuthority>) -> Result<()> {
        instructions::accept_authority(ctx)
    }

    /// Approves a collateral mint after checking it carries no extension that
    /// would make the vault insolvent.
    pub fn register_collateral(ctx: Context<RegisterCollateral>, min_stake: u64) -> Result<()> {
        instructions::register_collateral(ctx, min_stake)
    }

    /// Admits a price source to the registry, effective one timelock later.
    pub fn register_feed(ctx: Context<RegisterFeed>, args: RegisterFeedArgs) -> Result<()> {
        instructions::register_feed(ctx, args)
    }

    /// Updates a feed's attested depth, or disables it.
    pub fn update_feed(ctx: Context<UpdateFeed>, depth_quote: u64, enabled: bool) -> Result<()> {
        instructions::update_feed(ctx, depth_quote, enabled)
    }

    /// Admits a Pyth instrument to the registry, effective one timelock later.
    pub fn register_pyth_feed(
        ctx: Context<RegisterPythFeed>,
        args: RegisterPythFeedArgs,
    ) -> Result<()> {
        instructions::register_pyth_feed(ctx, args)
    }

    /// Adjusts a collateral mint's floor, or withdraws it from new markets.
    pub fn update_collateral(
        ctx: Context<UpdateCollateral>,
        min_stake: u64,
        enabled: bool,
    ) -> Result<()> {
        instructions::update_collateral(ctx, min_stake, enabled)
    }

    // -- Market lifecycle ----------------------------------------------

    /// Creates a market: mints, vault, spec, and the bond that pays its cranks.
    pub fn create_market(ctx: Context<CreateMarket>, args: CreateMarketArgs) -> Result<()> {
        instructions::create_market(ctx, args)
    }

    /// Stakes collateral on a side and mints that side's outcome tokens 1:1.
    pub fn deposit(ctx: Context<Deposit>, side_is_yes: bool, amount: u64) -> Result<()> {
        instructions::deposit(ctx, side_is_yes, amount)
    }

    /// Freezes every declared feed's price for the settlement window.
    ///
    /// Permissionless and paid from the bond. Separate from `resolve` so the
    /// answer depends only on what was recorded, never on when the crank ran.
    pub fn snapshot(ctx: Context<TakeSnapshot>) -> Result<()> {
        instructions::snapshot(ctx)
    }

    /// Runs the predicate over the frozen prices and splits the pot.
    pub fn resolve(ctx: Context<Resolve>) -> Result<()> {
        instructions::resolve(ctx)
    }

    /// Abandons a market that cannot settle honestly, refunding at par.
    pub fn void(ctx: Context<VoidMarket>) -> Result<()> {
        instructions::void(ctx)
    }

    // -- Settlement ----------------------------------------------------

    /// Burns a whole outcome position for the collateral it is owed and closes
    /// the token account.
    pub fn claim(ctx: Context<Claim>, side_is_yes: bool) -> Result<()> {
        instructions::claim(ctx, side_is_yes)
    }

    /// Pushes one of the three fee cuts to the party it belongs to.
    pub fn collect_fee(ctx: Context<CollectFee>, recipient: FeeRecipient) -> Result<()> {
        instructions::collect_fee(ctx, recipient)
    }

    /// Sweeps rounding dust and unclaimed collateral once claiming has closed.
    pub fn sweep_dust(ctx: Context<SweepDust>) -> Result<()> {
        instructions::sweep_dust(ctx)
    }

    /// Closes an emptied market and returns what rent can be returned.
    pub fn close_market(ctx: Context<CloseMarket>) -> Result<()> {
        instructions::close_market(ctx)
    }
}
