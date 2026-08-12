//! Creating a market: its question, its sources, and its bond.

use anchor_lang::prelude::*;
use anchor_lang::system_program::{transfer, Transfer};
use anchor_spl::token::{Mint, Token, TokenAccount};
use market_core::{validate_ramp, BPS_DENOMINATOR};
use market_math::Q64;
use market_vm::{verify, MAX_INPUTS};

use crate::constants::{seeds, MIN_MARKET_FEEDS};
use crate::errors::MarketError;
use crate::events::*;
use crate::hashing::spec_hash;
use crate::state::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct CreateMarketArgs {
    /// Caller-chosen, so the market's address is known before signing.
    pub market_id: [u8; 32],
    pub settle_at: i64,
    /// Value the predicate's score is compared against, Q64.64.
    pub strike: i128,
    /// Half-width of the settlement band, in basis points of the strike.
    pub ramp_bps: u16,
    pub feeds: Vec<FeedRef>,
    pub bytecode: Vec<u8>,
    pub rules_uri: String,
}

#[derive(Accounts)]
#[instruction(args: CreateMarketArgs)]
pub struct CreateMarket<'info> {
    // Every deserialised account is boxed. Anchor stores them inline in this
    // struct, and a context this wide does not fit in a 4 KiB BPF stack frame.
    #[account(mut, seeds = [seeds::CONFIG], bump = config.bump)]
    pub config: Box<Account<'info, Config>>,

    #[account(
        seeds = [seeds::COLLATERAL, collateral_mint.key().as_ref()],
        bump = collateral.bump,
        has_one = mint @ MarketError::CollateralNotRegistered,
        constraint = collateral.enabled @ MarketError::CollateralNotRegistered,
    )]
    pub collateral: Box<Account<'info, Collateral>>,
    /// Same account as `collateral.mint`; named separately so `has_one` can
    /// bind them.
    pub mint: Box<Account<'info, Mint>>,
    pub collateral_mint: Box<Account<'info, Mint>>,

    #[account(
        init,
        payer = creator,
        space = 8 + Market::INIT_SPACE,
        seeds = [seeds::MARKET, args.market_id.as_ref()],
        bump,
    )]
    pub market: Box<Account<'info, Market>>,

    #[account(
        init,
        payer = creator,
        space = 8 + MarketSpec::INIT_SPACE,
        seeds = [seeds::SPEC, market.key().as_ref()],
        bump,
    )]
    pub spec: Box<Account<'info, MarketSpec>>,

    #[account(
        init,
        payer = creator,
        seeds = [seeds::VAULT, market.key().as_ref()],
        bump,
        token::mint = collateral_mint,
        token::authority = market,
    )]
    pub vault: Box<Account<'info, TokenAccount>>,

    #[account(
        init,
        payer = creator,
        seeds = [seeds::YES_MINT, market.key().as_ref()],
        bump,
        mint::decimals = collateral_mint.decimals,
        mint::authority = market,
    )]
    pub yes_mint: Box<Account<'info, Mint>>,

    #[account(
        init,
        payer = creator,
        seeds = [seeds::NO_MINT, market.key().as_ref()],
        bump,
        mint::decimals = collateral_mint.decimals,
        mint::authority = market,
    )]
    pub no_mint: Box<Account<'info, Mint>>,

    #[account(mut)]
    pub creator: Signer<'info>,

    /// CHECK: matched against `config.treasury`; only receives lamports.
    #[account(mut, address = config.treasury @ MarketError::NotAuthorized)]
    pub treasury: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    // `remaining_accounts` holds one `Feed` per entry of `args.feeds`, in the
    // same order. They are deserialised and checked in the handler.
}

pub fn create_market(ctx: Context<CreateMarket>, args: CreateMarketArgs) -> Result<()> {
    let config = &ctx.accounts.config;
    require!(!config.paused, MarketError::Paused);
    require!(
        args.rules_uri.len() <= MAX_RULES_URI_LEN,
        MarketError::RulesUriTooLong
    );
    // The VM accepts one input; the protocol does not. How many independent
    // sources a price must clear is an economic question, answered here.
    require!(
        (usize::from(MIN_MARKET_FEEDS)..=MAX_INPUTS).contains(&args.feeds.len()),
        MarketError::PredicateInvalid
    );
    require!(args.strike > 0, MarketError::ParameterOutOfRange);
    require!(
        args.ramp_bps >= config.params.min_ramp_bps,
        MarketError::RampTooNarrow
    );

    let now = Clock::get()?.unix_timestamp;
    let params = config.params;
    let schedule = params.schedule(args.settle_at);
    schedule.validate().map_err(MarketError::from)?;

    // Betting must open, and stay open, before the deadline; a market whose
    // window has already begun could be bet on by someone watching it.
    let open_at = now
        .checked_add(i64::from(params.creation_cooldown))
        .ok_or(MarketError::Overflow)?;
    require!(
        schedule.lock_at() > open_at,
        MarketError::SettlementTimeInvalid
    );

    verify(&args.bytecode, args.feeds.len()).map_err(MarketError::from)?;

    // Checked now, not at resolution: parameters that could only abort would
    // otherwise hand the creator a guaranteed void.
    validate_ramp(Q64::from_raw(args.strike), args.ramp_bps).map_err(MarketError::from)?;

    let cap_per_side = resolve_feeds_and_cap(&ctx, &args, now, params.feed_cap_bps)?;

    // Non-refundable, and separate from the bond. The bond deters nothing on its
    // own: cranks are permissionless, so a creator can run them and recover it.
    transfer(
        CpiContext::new(
            ctx.accounts.system_program.key(),
            Transfer {
                from: ctx.accounts.creator.to_account_info(),
                to: ctx.accounts.treasury.to_account_info(),
            },
        ),
        params.creation_fee,
    )?;

    let bond = params
        .keeper_reward
        .checked_mul(2)
        .ok_or(MarketError::Overflow)?;
    transfer(
        CpiContext::new(
            ctx.accounts.system_program.key(),
            Transfer {
                from: ctx.accounts.creator.to_account_info(),
                to: ctx.accounts.market.to_account_info(),
            },
        ),
        bond,
    )?;

    let market_key = ctx.accounts.market.key();
    let spec_hash = spec_hash(
        &args.market_id,
        args.settle_at,
        args.strike,
        args.ramp_bps,
        &args.feeds,
        &args.bytecode,
        &args.rules_uri,
    );

    let spec = &mut ctx.accounts.spec;
    spec.bump = ctx.bumps.spec;
    spec.market = market_key;
    spec.feeds = args.feeds.clone();
    spec.bytecode = args.bytecode.clone();
    spec.rules_uri = args.rules_uri.clone();

    let market = &mut ctx.accounts.market;
    market.bump = ctx.bumps.market;
    market.status = MarketStatus::Created;
    market.market_id = args.market_id;
    market.spec_hash = spec_hash;
    market.creator = ctx.accounts.creator.key();
    market.collateral_mint = ctx.accounts.collateral_mint.key();
    market.yes_mint = ctx.accounts.yes_mint.key();
    market.no_mint = ctx.accounts.no_mint.key();
    market.vault = ctx.accounts.vault.key();
    market.created_at = now;
    market.open_at = open_at;
    market.settle_at = args.settle_at;
    market.params = params;
    market.cap_per_side = cap_per_side;
    market.strike = args.strike;
    market.ramp_bps = args.ramp_bps;
    market.staked_yes = 0;
    market.staked_no = 0;
    market.status_reason = VoidCause::None;
    market.share = 0;
    market.pool_yes = 0;
    market.pool_no = 0;
    market.fee_total = 0;
    market.fee_owed_treasury = 0;
    market.fee_owed_creator = 0;
    market.fee_owed_keeper = 0;
    market.snapshot_keeper = Pubkey::default();
    market.resolved_at = 0;
    market.bond_lamports = bond;

    ctx.accounts.config.markets_created = ctx.accounts.config.markets_created.saturating_add(1);

    emit!(MarketCreated {
        market: market_key,
        creator: market.creator,
        collateral_mint: market.collateral_mint,
        settle_at: args.settle_at,
        lock_at: schedule.lock_at(),
        cap_per_side,
        spec_hash,
    });
    Ok(())
}

/// Checks every declared feed and derives the per-side cap from the thinnest.
///
/// Distinctness matters more than it looks: without it a market can name the
/// same pool three times and call the result a median of three sources.
fn resolve_feeds_and_cap(
    ctx: &Context<CreateMarket>,
    args: &CreateMarketArgs,
    now: i64,
    feed_cap_bps: u16,
) -> Result<u64> {
    require!(
        ctx.remaining_accounts.len() == args.feeds.len(),
        MarketError::FeedAccountsMismatch
    );

    let mut thinnest = u64::MAX;
    for (position, declared) in args.feeds.iter().enumerate() {
        for earlier in &args.feeds[..position] {
            require!(earlier.feed != declared.feed, MarketError::DuplicateFeed);
        }

        let account = &ctx.remaining_accounts[position];
        require_keys_eq!(
            account.key(),
            declared.feed,
            MarketError::FeedAccountsMismatch
        );
        let feed: Account<Feed> = Account::try_from(account)?;
        require!(feed.is_active(now), MarketError::FeedNotActive);
        thinnest = thinnest.min(feed.depth_quote);
    }

    let cap = (u128::from(thinnest) * u128::from(feed_cap_bps) / u128::from(BPS_DENOMINATOR))
        .try_into()
        .map_err(|_| MarketError::Overflow)?;
    require!(cap > 0, MarketError::ParameterOutOfRange);
    Ok(cap)
}
