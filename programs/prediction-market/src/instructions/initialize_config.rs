//! Bringing a deployment into existence.

use anchor_lang::prelude::*;

use crate::constants::{
    seeds,
    time::{MAX_TIMELOCK, MIN_TIMELOCK},
};
use crate::errors::MarketError;
use crate::state::*;
use crate::utils::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct InitializeConfigArgs {
    pub treasury: Pubkey,
    pub raydium_clmm_program: Pubkey,
    pub pyth_receiver_program: Pubkey,
    pub timelock: u32,
    pub params: MarketParams,
}

#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    #[account(
        init,
        payer = payer,
        space = 8 + Config::INIT_SPACE,
        seeds = [seeds::CONFIG],
        bump,
    )]
    pub config: Box<Account<'info, Config>>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

pub fn initialize_config(ctx: Context<InitializeConfig>, args: InitializeConfigArgs) -> Result<()> {
    require!(
        (MIN_TIMELOCK..=MAX_TIMELOCK).contains(&args.timelock),
        MarketError::ParameterOutOfRange
    );
    validate_params(&args.params)?;

    let config = &mut ctx.accounts.config;
    config.bump = ctx.bumps.config;
    config.authority = ctx.accounts.payer.key();
    config.pending_authority = Pubkey::default();
    config.treasury = args.treasury;
    config.raydium_clmm_program = args.raydium_clmm_program;
    config.pyth_receiver_program = args.pyth_receiver_program;
    config.paused = false;
    config.params = args.params;
    config.pending_params = MarketParams::default();
    config.pending_effective_at = 0;
    config.has_pending = false;
    config.timelock = args.timelock;
    config.markets_created = 0;
    Ok(())
}
