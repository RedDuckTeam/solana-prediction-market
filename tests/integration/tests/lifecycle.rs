//! The market lifecycle, end to end, against a simulated ledger.

mod harness;

use anchor_lang::{AnchorDeserialize, InstructionData, ToAccountMetas};
use harness::*;
use market_math::Q64;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signature::Keypair, signature::Signer};

const NOW: i64 = 1_800_000_000;
/// Far enough out that betting opens and closes with room to spare.
const SETTLE_AT: i64 = NOW + 2_000;

/// A strike well below the pool's price, so the ramp saturates at "all YES".
const STRIKE_BELOW: i64 = 90;
/// A strike well above it.
const STRIKE_ABOVE: i64 = 120;

struct Market {
    address: Pubkey,
    yes_mint: Pubkey,
    no_mint: Pubkey,
    vault: Pubkey,
}

fn create_market(world: &mut World, strike_units: i64, ramp_bps: u16) -> Market {
    let id = [7u8; 32];
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
    accounts.extend(
        world
            .rings
            .iter()
            .map(|ring| solana_sdk::instruction::AccountMeta::new_readonly(feed_pda(ring), false)),
    );

    let instruction = Instruction {
        program_id: prediction_market::ID,
        accounts,
        data: prediction_market::instruction::CreateMarket {
            args: prediction_market::instructions::CreateMarketArgs {
                market_id: id,
                settle_at: SETTLE_AT,
                strike: Q64::from_int(strike_units).raw(),
                ramp_bps,
                feeds: world.feed_refs(),
                bytecode: median_of_three(),
                rules_uri: "https://example.test/rules/1".to_string(),
            },
        }
        .data(),
    };
    let authority = world.authority.insecure_clone();
    world.must_send(instruction, &[&authority]);
    market
}

fn deposit(
    world: &mut World,
    market: &Market,
    bettor: &Keypair,
    side_is_yes: bool,
    amount: u64,
) -> (Pubkey, Pubkey) {
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
    (collateral, outcome)
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
    accounts.extend(world.feed_account_metas());

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

fn claim(
    world: &mut World,
    market: &Market,
    holder: &Keypair,
    side_is_yes: bool,
    outcome: Pubkey,
    collateral: Pubkey,
) -> Result<(), String> {
    let side_mint = if side_is_yes {
        market.yes_mint
    } else {
        market.no_mint
    };
    world.send(
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::Claim {
                market: market.address,
                vault: market.vault,
                side_mint,
                holder_outcome: outcome,
                holder_collateral: collateral,
                holder: holder.pubkey(),
                token_program: spl_token::ID,
            }
            .to_account_metas(None),
            data: prediction_market::instruction::Claim { side_is_yes }.data(),
        },
        &[holder],
    )
}

fn read_market(world: &World, address: &Pubkey) -> prediction_market::state::Market {
    let account = world.svm.get_account(address).expect("market exists");
    prediction_market::state::Market::deserialize(&mut &account.data[8..]).expect("market decodes")
}

fn void(world: &mut World, market: &Market, caller: &Keypair) -> Result<(), String> {
    world.send(
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::VoidMarket {
                market: market.address,
                caller: caller.pubkey(),
            }
            .to_account_metas(None),
            data: prediction_market::instruction::Void {}.data(),
        },
        &[caller],
    )
}

#[test]
fn a_market_runs_from_creation_to_payout() {
    let mut world = World::new(NOW);
    let market = create_market(&mut world, STRIKE_BELOW, RAMP_BPS);

    // Betting opens after the cooldown that lets anyone read the spec first.
    let alice = Keypair::new();
    let bob = Keypair::new();
    world.svm.airdrop(&alice.pubkey(), 1_000_000_000).unwrap();
    world.svm.airdrop(&bob.pubkey(), 1_000_000_000).unwrap();

    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 10);
    let (alice_collateral, alice_outcome) = deposit(&mut world, &market, &alice, true, 1_000_000);
    let (bob_collateral, bob_outcome) = deposit(&mut world, &market, &bob, false, 4_000_000);

    let state = read_market(&world, &market.address);
    assert_eq!(state.staked_yes, 1_000_000);
    assert_eq!(state.staked_no, 4_000_000);
    assert_eq!(world.token_balance(&market.vault), 5_000_000);

    // The pool prices the token at ~100, comfortably above the strike, so the
    // ramp saturates and YES takes the pot.
    world.refresh_rings(SETTLE_AT, TICK_AT_100);
    world.warp_to(SETTLE_AT + 10);

    let keeper = Keypair::new();
    world.svm.airdrop(&keeper.pubkey(), 1_000_000_000).unwrap();
    snapshot(&mut world, &market, &keeper).expect("snapshot succeeds");
    resolve(&mut world, &market, &keeper).expect("resolve succeeds");

    let state = read_market(&world, &market.address);
    assert_eq!(state.share, Q64::ONE.raw(), "strike is far below the price");
    // Four million changed hands; two percent of it is the fee.
    assert_eq!(state.fee_total, 80_000);
    assert_eq!(state.pool_yes, 4_920_000);
    assert_eq!(state.pool_no, 0);

    claim(
        &mut world,
        &market,
        &alice,
        true,
        alice_outcome,
        alice_collateral,
    )
    .expect("winner claims");
    assert_eq!(world.token_balance(&alice_collateral), 4_920_000);

    // The losing side may claim, and is paid nothing; the call still closes the
    // token account so the rent comes back.
    claim(
        &mut world,
        &market,
        &bob,
        false,
        bob_outcome,
        bob_collateral,
    )
    .expect("loser claims");
    assert_eq!(world.token_balance(&bob_collateral), 0);
    // Claiming closed the outcome account outright, returning its rent.
    assert!(world
        .svm
        .get_account(&bob_outcome)
        .is_none_or(|account| account.data.is_empty()));
}

#[test]
fn a_strike_above_the_price_pays_the_other_side() {
    let mut world = World::new(NOW);
    let market = create_market(&mut world, STRIKE_ABOVE, RAMP_BPS);

    let alice = Keypair::new();
    let bob = Keypair::new();
    world.svm.airdrop(&alice.pubkey(), 1_000_000_000).unwrap();
    world.svm.airdrop(&bob.pubkey(), 1_000_000_000).unwrap();

    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 10);
    deposit(&mut world, &market, &alice, true, 1_000_000);
    let (bob_collateral, bob_outcome) = deposit(&mut world, &market, &bob, false, 1_000_000);

    world.refresh_rings(SETTLE_AT, TICK_AT_100);
    world.warp_to(SETTLE_AT + 10);

    let keeper = Keypair::new();
    world.svm.airdrop(&keeper.pubkey(), 1_000_000_000).unwrap();
    snapshot(&mut world, &market, &keeper).expect("snapshot succeeds");
    resolve(&mut world, &market, &keeper).expect("resolve succeeds");

    let state = read_market(&world, &market.address);
    assert_eq!(state.share, 0, "strike is far above the price");
    assert_eq!(state.fee_total, 20_000);
    assert_eq!(state.pool_no, 1_980_000);

    claim(
        &mut world,
        &market,
        &bob,
        false,
        bob_outcome,
        bob_collateral,
    )
    .expect("winner claims");
    assert_eq!(world.token_balance(&bob_collateral), 1_980_000);
}

#[test]
fn a_price_inside_the_band_splits_the_pot() {
    let mut world = World::new(NOW);
    // The pool sits at ~99.998; a strike of 100 with a 50 bp band puts the
    // price almost exactly at the midpoint, so both sides are paid.
    let market = create_market(&mut world, 100, RAMP_BPS);

    let alice = Keypair::new();
    let bob = Keypair::new();
    world.svm.airdrop(&alice.pubkey(), 1_000_000_000).unwrap();
    world.svm.airdrop(&bob.pubkey(), 1_000_000_000).unwrap();

    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 10);
    deposit(&mut world, &market, &alice, true, 1_000_000);
    deposit(&mut world, &market, &bob, false, 1_000_000);

    world.refresh_rings(SETTLE_AT, TICK_AT_100);
    world.warp_to(SETTLE_AT + 10);

    let keeper = Keypair::new();
    world.svm.airdrop(&keeper.pubkey(), 1_000_000_000).unwrap();
    snapshot(&mut world, &market, &keeper).expect("snapshot succeeds");
    resolve(&mut world, &market, &keeper).expect("resolve succeeds");

    let state = read_market(&world, &market.address);
    let share = Q64::from_raw(state.share);
    assert!(share > Q64::ZERO && share < Q64::ONE, "share was {share:?}");
    // Barely any stake changed hands, so the fee is a rounding artefact rather
    // than a windfall -- which is the point of charging on the transfer.
    assert!(state.fee_total < 1_000, "fee was {}", state.fee_total);
    assert_eq!(
        state.pool_yes + state.pool_no + state.fee_total,
        2_000_000,
        "the pot is conserved exactly"
    );
}

#[test]
fn betting_closes_before_the_measured_window_opens() {
    let mut world = World::new(NOW);
    let market = create_market(&mut world, STRIKE_BELOW, RAMP_BPS);

    let latecomer = Keypair::new();
    world
        .svm
        .airdrop(&latecomer.pubkey(), 1_000_000_000)
        .unwrap();

    // One second before the deadline is still fine.
    let lock_at = SETTLE_AT - i64::from(TWAP_WINDOW) - i64::from(SKEW);
    world.warp_to(lock_at - 1);
    deposit(&mut world, &market, &latecomer, true, 1_000_000);

    // At the deadline it is not, and the whole skew gap stays shut so a slow
    // clock cannot let a bet land inside the window that settles it.
    world.warp_to(lock_at);
    let collateral =
        world.place_token_account(world.collateral_mint, latecomer.pubkey(), 1_000_000);
    let outcome = world.place_token_account(market.yes_mint, latecomer.pubkey(), 0);
    let instruction = Instruction {
        program_id: prediction_market::ID,
        accounts: prediction_market::accounts::Deposit {
            market: market.address,
            collateral: collateral_pda(&world.collateral_mint),
            collateral_mint: world.collateral_mint,
            vault: market.vault,
            side_mint: market.yes_mint,
            depositor_collateral: collateral,
            depositor_outcome: outcome,
            depositor: latecomer.pubkey(),
            token_program: spl_token::ID,
        }
        .to_account_metas(None),
        data: prediction_market::instruction::Deposit {
            side_is_yes: true,
            amount: 1_000_000,
        }
        .data(),
    };
    assert!(
        world.send(instruction, &[&latecomer]).is_err(),
        "deposits must be closed once the skew gap begins"
    );
}

#[test]
fn a_market_with_one_empty_side_voids_and_refunds() {
    let mut world = World::new(NOW);
    let market = create_market(&mut world, STRIKE_BELOW, RAMP_BPS);

    let alice = Keypair::new();
    world.svm.airdrop(&alice.pubkey(), 1_000_000_000).unwrap();
    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 10);
    let (alice_collateral, alice_outcome) = deposit(&mut world, &market, &alice, true, 1_000_000);

    // Nobody took the other side, so there is no counterparty and no bet.
    world.refresh_rings(SETTLE_AT, TICK_AT_100);
    world.warp_to(SETTLE_AT + 10);

    let keeper = Keypair::new();
    world.svm.airdrop(&keeper.pubkey(), 1_000_000_000).unwrap();
    assert!(
        snapshot(&mut world, &market, &keeper).is_err(),
        "a one-sided market cannot be settled"
    );
    void(&mut world, &market, &keeper).expect("void succeeds");

    let state = read_market(&world, &market.address);
    assert_eq!(state.fee_total, 0, "a void charges nothing");
    assert_eq!(state.pool_yes, 1_000_000);

    claim(
        &mut world,
        &market,
        &alice,
        true,
        alice_outcome,
        alice_collateral,
    )
    .expect("refund claim");
    assert_eq!(
        world.token_balance(&alice_collateral),
        1_000_000,
        "refunded at par"
    );
}

#[test]
fn a_market_nobody_cranks_voids_after_the_grace_period() {
    let mut world = World::new(NOW);
    let market = create_market(&mut world, STRIKE_BELOW, RAMP_BPS);

    let alice = Keypair::new();
    let bob = Keypair::new();
    world.svm.airdrop(&alice.pubkey(), 1_000_000_000).unwrap();
    world.svm.airdrop(&bob.pubkey(), 1_000_000_000).unwrap();
    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 10);
    let (alice_collateral, alice_outcome) = deposit(&mut world, &market, &alice, true, 1_000_000);
    let (bob_collateral, bob_outcome) = deposit(&mut world, &market, &bob, false, 1_000_000);

    let caller = Keypair::new();
    world.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

    // Inside the grace period the market is not yet abandoned.
    world.warp_to(SETTLE_AT + i64::from(GRACE));
    assert!(void(&mut world, &market, &caller).is_err());

    world.warp_to(SETTLE_AT + i64::from(GRACE) + 1);
    void(&mut world, &market, &caller).expect("void after grace");

    // Both sides get their stake back, whole.
    claim(
        &mut world,
        &market,
        &alice,
        true,
        alice_outcome,
        alice_collateral,
    )
    .unwrap();
    claim(
        &mut world,
        &market,
        &bob,
        false,
        bob_outcome,
        bob_collateral,
    )
    .unwrap();
    assert_eq!(world.token_balance(&alice_collateral), 1_000_000);
    assert_eq!(world.token_balance(&bob_collateral), 1_000_000);
    assert_eq!(world.token_balance(&market.vault), 0);
}

#[test]
fn a_snapshot_outside_its_window_is_refused() {
    let mut world = World::new(NOW);
    let market = create_market(&mut world, STRIKE_BELOW, RAMP_BPS);

    let alice = Keypair::new();
    let bob = Keypair::new();
    world.svm.airdrop(&alice.pubkey(), 1_000_000_000).unwrap();
    world.svm.airdrop(&bob.pubkey(), 1_000_000_000).unwrap();
    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 10);
    deposit(&mut world, &market, &alice, true, 1_000_000);
    deposit(&mut world, &market, &bob, false, 1_000_000);

    world.refresh_rings(SETTLE_AT, TICK_AT_100);
    let keeper = Keypair::new();
    world.svm.airdrop(&keeper.pubkey(), 1_000_000_000).unwrap();

    world.warp_to(SETTLE_AT - 1);
    assert!(
        snapshot(&mut world, &market, &keeper).is_err(),
        "settlement has not happened yet"
    );

    world.warp_to(SETTLE_AT + i64::from(GRACE) + 1);
    assert!(
        snapshot(&mut world, &market, &keeper).is_err(),
        "the grace period has expired"
    );
}

#[test]
fn a_stale_ring_cannot_settle_a_market() {
    let mut world = World::new(NOW);
    let market = create_market(&mut world, STRIKE_BELOW, RAMP_BPS);

    let alice = Keypair::new();
    let bob = Keypair::new();
    world.svm.airdrop(&alice.pubkey(), 1_000_000_000).unwrap();
    world.svm.airdrop(&bob.pubkey(), 1_000_000_000).unwrap();
    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 10);
    deposit(&mut world, &market, &alice, true, 1_000_000);
    deposit(&mut world, &market, &bob, false, 1_000_000);

    // The rings were last written long before settlement, so nothing brackets
    // the closing instant. Extrapolating would be the easy answer and is
    // exactly what must not happen.
    world.warp_to(SETTLE_AT + 10);
    let keeper = Keypair::new();
    world.svm.airdrop(&keeper.pubkey(), 1_000_000_000).unwrap();
    assert!(
        snapshot(&mut world, &market, &keeper).is_err(),
        "an unbracketed window must be refused, not extrapolated"
    );
}

#[test]
fn a_quiet_pool_is_refused_even_though_its_ring_is_full() {
    let mut world = World::new(NOW);
    let market = create_market(&mut world, STRIKE_BELOW, RAMP_BPS);

    let alice = Keypair::new();
    let bob = Keypair::new();
    world.svm.airdrop(&alice.pubkey(), 1_000_000_000).unwrap();
    world.svm.airdrop(&bob.pubkey(), 1_000_000_000).unwrap();
    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 10);
    deposit(&mut world, &market, &alice, true, 1_000_000);
    deposit(&mut world, &market, &bob, false, 1_000_000);

    // Rewrite one ring with a single very long gap covering the window. Raydium
    // credits a segment's whole duration to the tick standing at its end, so a
    // price held for an instant would otherwise carry the entire average.
    let newest_at = SETTLE_AT + 30;
    world.refresh_rings(SETTLE_AT, TICK_AT_100);
    let (ring, pool) = (world.rings[0], world.pools[0]);
    world.place_ring_with_spacing(ring, pool, newest_at, 600, TICK_AT_100);

    world.warp_to(SETTLE_AT + 10);
    let keeper = Keypair::new();
    world.svm.airdrop(&keeper.pubkey(), 1_000_000_000).unwrap();
    assert!(
        snapshot(&mut world, &market, &keeper).is_err(),
        "a window covered by one long segment must be refused"
    );
}
