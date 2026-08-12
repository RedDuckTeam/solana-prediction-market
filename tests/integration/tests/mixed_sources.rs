//! Settling a market priced from two different kinds of source.
//!
//! One market, two Raydium pools and one Pyth instrument, one median -- the
//! seam where a predicate must not learn which kind produced a number.
//!
//! Also the checks only a posted oracle account needs, since it alone is
//! supplied by the party who wants the market settled: window, confidence, and
//! how much of the window the chain was down for.

mod harness;

use anchor_lang::{AnchorDeserialize, InstructionData, ToAccountMetas};
use harness::*;
use market_math::Q64;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signature::Keypair, signature::Signer};

const NOW: i64 = 1_800_000_000;
const SETTLE_AT: i64 = NOW + 2_000;

/// 100.00, at Pyth's usual exponent of -8. The pools quote 99.998, so the
/// median of the three is a pool price and the oracle is the outlier -- which
/// is the point of taking a median at all.
const PYTH_PRICE: i64 = 10_000_000_000;
/// Ten basis points of disagreement: an ordinary, quiet market.
const PYTH_CONF: u64 = 10_000_000;

struct Market {
    address: Pubkey,
    yes_mint: Pubkey,
    no_mint: Pubkey,
    vault: Pubkey,
}

fn create_mixed_market(world: &mut World, strike_units: i64) -> Market {
    let id = [11u8; 32];
    let address = market_pda(&id);
    let market = Market {
        address,
        yes_mint: child_pda(b"yes", &address),
        no_mint: child_pda(b"no", &address),
        vault: child_pda(b"vault", &address),
    };

    let mut accounts = prediction_market::accounts::CreateMarket {
        config: config_pda(),
        collateral: collateral_pda(&world.collateral_mint),
        mint: world.collateral_mint,
        collateral_mint: world.collateral_mint,
        market: address,
        spec: child_pda(b"spec", &address),
        vault: market.vault,
        yes_mint: market.yes_mint,
        no_mint: market.no_mint,
        creator: world.authority.pubkey(),
        treasury: world.treasury,
        token_program: spl_token::ID,
        system_program: anchor_lang::solana_program::system_program::ID,
    }
    .to_account_metas(None);
    accounts.extend(world.mixed_feed_refs().iter().map(|reference| {
        solana_sdk::instruction::AccountMeta::new_readonly(reference.feed, false)
    }));

    let instruction = Instruction {
        program_id: prediction_market::ID,
        accounts,
        data: prediction_market::instruction::CreateMarket {
            args: prediction_market::instructions::CreateMarketArgs {
                market_id: id,
                settle_at: SETTLE_AT,
                strike: Q64::from_int(strike_units).raw(),
                ramp_bps: RAMP_BPS,
                feeds: world.mixed_feed_refs(),
                bytecode: median_of_three(),
                rules_uri: "https://example.test/rules/mixed".to_string(),
            },
        }
        .data(),
    };
    let authority = world.authority.insecure_clone();
    world.must_send(instruction, &[&authority]);
    market
}

fn deposit(world: &mut World, market: &Market, bettor: &Keypair, side_is_yes: bool, amount: u64) {
    let collateral = world.place_token_account(world.collateral_mint, bettor.pubkey(), amount);
    let side_mint = if side_is_yes {
        market.yes_mint
    } else {
        market.no_mint
    };
    let outcome = world.place_token_account(side_mint, bettor.pubkey(), 0);

    let instruction = Instruction {
        program_id: prediction_market::ID,
        accounts: prediction_market::accounts::Deposit {
            market: market.address,
            collateral: collateral_pda(&world.collateral_mint),
            collateral_mint: world.collateral_mint,
            vault: market.vault,
            side_mint,
            depositor_collateral: collateral,
            depositor_outcome: outcome,
            depositor: bettor.pubkey(),
            token_program: spl_token::ID,
        }
        .to_account_metas(None),
        data: prediction_market::instruction::Deposit {
            side_is_yes,
            amount,
        }
        .data(),
    };
    world.must_send(instruction, &[bettor]);
}

fn snapshot(world: &mut World, market: &Market, keeper: &Keypair) -> Result<(), String> {
    let mut accounts = prediction_market::accounts::TakeSnapshot {
        config: config_pda(),
        market: market.address,
        spec: child_pda(b"spec", &market.address),
        snapshot: child_pda(b"snapshot", &market.address),
        keeper: keeper.pubkey(),
        system_program: anchor_lang::solana_program::system_program::ID,
    }
    .to_account_metas(None);
    accounts.extend(world.mixed_account_metas());

    world.send(
        Instruction {
            program_id: prediction_market::ID,
            accounts,
            data: prediction_market::instruction::Snapshot {}.data(),
        },
        &[keeper],
    )
}

fn resolve(world: &mut World, market: &Market, resolver: &Keypair) -> Result<(), String> {
    world.send(
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::Resolve {
                market: market.address,
                spec: child_pda(b"spec", &market.address),
                snapshot: child_pda(b"snapshot", &market.address),
                resolver: resolver.pubkey(),
            }
            .to_account_metas(None),
            data: prediction_market::instruction::Resolve {}.data(),
        },
        &[resolver],
    )
}

fn read_market(world: &World, address: &Pubkey) -> prediction_market::state::Market {
    let account = world.svm.get_account(address).expect("market exists");
    prediction_market::state::Market::deserialize(&mut &account.data[8..]).expect("market decodes")
}

/// Stakes both sides and advances to the settlement instant.
fn open_and_lock(world: &mut World, market: &Market) -> Keypair {
    let alice = Keypair::new();
    let bob = Keypair::new();
    world.svm.airdrop(&alice.pubkey(), 1_000_000_000).unwrap();
    world.svm.airdrop(&bob.pubkey(), 1_000_000_000).unwrap();

    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 10);
    deposit(world, market, &alice, true, 1_000_000);
    deposit(world, market, &bob, false, 1_000_000);

    world.refresh_rings(SETTLE_AT, TICK_AT_100);
    world.warp_to(SETTLE_AT + 10);

    let keeper = Keypair::new();
    world.svm.airdrop(&keeper.pubkey(), 1_000_000_000).unwrap();
    keeper
}

#[test]
fn a_pool_and_an_oracle_settle_the_same_market() {
    let mut world = World::new(NOW);
    let market = create_mixed_market(&mut world, 90);
    let keeper = open_and_lock(&mut world, &market);
    world.place_pyth_twap(SETTLE_AT, PYTH_PRICE, PYTH_CONF, 0);

    snapshot(&mut world, &market, &keeper).expect("snapshot succeeds");
    resolve(&mut world, &market, &keeper).expect("resolve succeeds");

    let state = read_market(&world, &market.address);
    assert_eq!(
        state.share,
        Q64::ONE.raw(),
        "the strike is far below the price"
    );
    assert_eq!(state.pool_yes + state.pool_no + state.fee_total, 2_000_000);
}

#[test]
fn the_median_prefers_the_pools_when_the_oracle_is_the_outlier() {
    let mut world = World::new(NOW);
    // Strike between the pools' 99.998 and a wildly wrong oracle price.
    let market = create_mixed_market(&mut world, 500);
    let keeper = open_and_lock(&mut world, &market);
    // The oracle claims a thousand; the two pools disagree and outvote it.
    world.place_pyth_twap(SETTLE_AT, 100_000_000_000, PYTH_CONF, 0);

    snapshot(&mut world, &market, &keeper).expect("snapshot succeeds");
    resolve(&mut world, &market, &keeper).expect("resolve succeeds");

    let state = read_market(&world, &market.address);
    assert_eq!(
        state.share, 0,
        "two honest sources must outvote one that is far off"
    );
}

#[test]
fn a_window_the_market_did_not_ask_for_is_refused() {
    let mut world = World::new(NOW);
    let market = create_mixed_market(&mut world, 90);
    let keeper = open_and_lock(&mut world, &market);

    // Signed Pyth history stretches back a long way, so a keeper free to pick
    // the window would pick a profitable one. This one is an hour early.
    world.place_pyth_twap(SETTLE_AT - 3_600, PYTH_PRICE, PYTH_CONF, 0);
    assert!(
        snapshot(&mut world, &market, &keeper).is_err(),
        "the averaged window must be the one the market declared"
    );

    // With the right window it settles.
    world.place_pyth_twap(SETTLE_AT, PYTH_PRICE, PYTH_CONF, 0);
    snapshot(&mut world, &market, &keeper).expect("snapshot succeeds");
}

#[test]
fn a_market_in_disarray_is_not_settled_against() {
    let mut world = World::new(NOW);
    let market = create_mixed_market(&mut world, 90);
    let keeper = open_and_lock(&mut world, &market);

    // Two hundred basis points of publisher disagreement, against a hundred
    // point limit: something is happening, and the midpoint is not a price
    // anyone should have money resolved against.
    world.place_pyth_twap(SETTLE_AT, PYTH_PRICE, 200_000_000, 0);
    assert!(
        snapshot(&mut world, &market, &keeper).is_err(),
        "a wide confidence interval must stop the settlement"
    );

    world.place_pyth_twap(SETTLE_AT, PYTH_PRICE, PYTH_CONF, 0);
    snapshot(&mut world, &market, &keeper).expect("snapshot succeeds");
}

#[test]
fn an_average_over_a_halted_chain_is_refused() {
    let mut world = World::new(NOW);
    let market = create_mixed_market(&mut world, 90);
    let keeper = open_and_lock(&mut world, &market);

    // Ten percent of the window fell in slots that were never produced, against
    // a five percent limit. An average over a chain that was down is not an
    // average over a market.
    world.place_pyth_twap(SETTLE_AT, PYTH_PRICE, PYTH_CONF, 100_000);
    assert!(
        snapshot(&mut world, &market, &keeper).is_err(),
        "too many missed slots must stop the settlement"
    );

    world.place_pyth_twap(SETTLE_AT, PYTH_PRICE, PYTH_CONF, 50_000);
    snapshot(&mut world, &market, &keeper).expect("snapshot succeeds");
}

#[test]
fn an_account_from_the_wrong_program_is_not_a_price() {
    let mut world = World::new(NOW);
    let market = create_mixed_market(&mut world, 90);
    let keeper = open_and_lock(&mut world, &market);
    world.place_pyth_twap(SETTLE_AT, PYTH_PRICE, PYTH_CONF, 0);

    // Re-home the account under an impostor program. Its bytes are still
    // perfectly well formed, which is exactly why the owner is checked.
    let account = world.svm.get_account(&world.pyth_account).unwrap();
    world
        .svm
        .set_account(
            world.pyth_account,
            solana_sdk::account::Account {
                owner: Pubkey::new_unique(),
                ..account
            },
        )
        .unwrap();

    assert!(
        snapshot(&mut world, &market, &keeper).is_err(),
        "only the configured receiver program may vouch for a price"
    );
}
