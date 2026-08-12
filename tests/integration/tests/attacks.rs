//! Deliberate attempts to break the protocol.
//!
//! The other suites check that honest use works and that honest mistakes are
//! refused. This one assumes a caller who reads the source, builds instructions
//! by hand, and substitutes any account the runtime will let them.
//!
//! Every test here must fail the attack. A test that starts passing for the
//! wrong reason is worse than none, so each asserts the specific refusal rather
//! than merely that something went wrong.

mod harness;

use anchor_lang::{AnchorDeserialize, InstructionData, ToAccountMetas};
use harness::*;
use market_math::Q64;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Keypair,
    signature::Signer,
};

const NOW: i64 = 1_800_000_000;
const SETTLE_AT: i64 = NOW + 2_000;
const STRIKE_BELOW: i64 = 90;
/// The smallest stake the harness's registered collateral accepts.
const MIN_STAKE: u64 = 1_000;

struct Market {
    address: Pubkey,
    yes_mint: Pubkey,
    no_mint: Pubkey,
    vault: Pubkey,
}

impl Market {
    fn spec(&self) -> Pubkey {
        child_pda(b"spec", &self.address)
    }
    fn snapshot(&self) -> Pubkey {
        child_pda(b"snapshot", &self.address)
    }
}

/// A fresh keypair with lamports, since a crank pays rent for the snapshot it
/// creates and a bettor pays fees.
fn funded(world: &mut World) -> Keypair {
    let key = Keypair::new();
    world.svm.airdrop(&key.pubkey(), 1_000_000_000).unwrap();
    key
}

fn create_market(world: &mut World, id: [u8; 32], strike_units: i64) -> Market {
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
        spec: market.spec(),
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
            .map(|ring| AccountMeta::new_readonly(feed_pda(ring), false)),
    );

    let instruction = Instruction {
        program_id: prediction_market::ID,
        accounts,
        data: prediction_market::instruction::CreateMarket {
            args: prediction_market::instructions::CreateMarketArgs {
                market_id: id,
                settle_at: SETTLE_AT,
                strike: Q64::from_int(strike_units).raw(),
                ramp_bps: RAMP_BPS,
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
        spec: market.spec(),
        snapshot: market.snapshot(),
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

/// `spec` and `snapshot` are parameters so an attacker can point them elsewhere.
fn resolve_with(
    world: &mut World,
    market: &Market,
    spec: Pubkey,
    snapshot: Pubkey,
    resolver: &Keypair,
) -> Result<(), String> {
    world.send(
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::Resolve {
                market: market.address,
                spec,
                snapshot,
                resolver: resolver.pubkey(),
            }
            .to_account_metas(None),
            data: prediction_market::instruction::Resolve {}.data(),
        },
        &[resolver],
    )
}

fn resolve(world: &mut World, market: &Market, resolver: &Keypair) -> Result<(), String> {
    resolve_with(world, market, market.spec(), market.snapshot(), resolver)
}

/// `vault` and `side_mint` are parameters for the same reason.
#[allow(clippy::too_many_arguments)]
fn claim_with(
    world: &mut World,
    market: &Market,
    vault: Pubkey,
    side_mint: Pubkey,
    side_is_yes: bool,
    holder: &Keypair,
    outcome: Pubkey,
    collateral: Pubkey,
) -> Result<(), String> {
    world.send(
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::Claim {
                market: market.address,
                vault,
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

/// A resolved market with a winner, a loser, and both positions still held.
struct Settled {
    market: Market,
    winner: Keypair,
    winner_outcome: Pubkey,
    winner_collateral: Pubkey,
    loser: Keypair,
    loser_outcome: Pubkey,
    loser_collateral: Pubkey,
}

fn settled_market(world: &mut World, id: [u8; 32]) -> Settled {
    let market = create_market(world, id, STRIKE_BELOW);
    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 1);

    let winner = funded(world);
    let loser = funded(world);
    let (winner_collateral, winner_outcome) = deposit(world, &market, &winner, true, 1_000_000);
    let (loser_collateral, loser_outcome) = deposit(world, &market, &loser, false, 3_000_000);

    world.refresh_rings(SETTLE_AT, TICK_AT_100);
    world.warp_to(SETTLE_AT + 1);

    let keeper = funded(world);
    snapshot(world, &market, &keeper).expect("snapshot succeeds");
    resolve(world, &market, &keeper).expect("resolve succeeds");

    Settled {
        market,
        winner,
        winner_outcome,
        winner_collateral,
        loser,
        loser_outcome,
        loser_collateral,
    }
}

// -- Substituting accounts -------------------------------------------------

#[test]
fn a_vault_belonging_to_another_market_cannot_be_drained() {
    let mut world = World::new(NOW);
    // Created first: a market's betting window must open before it locks, so
    // it cannot be created after the clock has moved on.
    let attacker_market = create_market(&mut world, [2u8; 32], STRIKE_BELOW);
    let victim = settled_market(&mut world, [1u8; 32]);

    let before = world.token_balance(&attacker_market.vault);
    assert_eq!(before, 0, "a fresh market's vault is empty");

    // Claim against the victim market while naming a foreign vault. The market
    // account carries `has_one = vault`, so the substitution is what fails --
    // not the balance, and not the holder's own position.
    let failure = claim_with(
        &mut world,
        &victim.market,
        attacker_market.vault,
        victim.market.yes_mint,
        true,
        &victim.winner,
        victim.winner_outcome,
        victim.winner_collateral,
    )
    .expect_err("a foreign vault must be refused");
    assert!(
        failure.contains("ConstraintHasOne") || failure.contains("2001"),
        "expected a has_one violation, got: {failure}"
    );
}

#[test]
fn a_token_account_the_attacker_controls_cannot_stand_in_for_the_vault() {
    let mut world = World::new(NOW);
    let settled = settled_market(&mut world, [1u8; 32]);

    let attacker = funded(&mut world);
    let impostor = world.place_token_account(world.collateral_mint, attacker.pubkey(), 5_000_000);

    let failure = claim_with(
        &mut world,
        &settled.market,
        impostor,
        settled.market.yes_mint,
        true,
        &settled.winner,
        settled.winner_outcome,
        settled.winner_collateral,
    )
    .expect_err("an attacker-owned vault must be refused");
    assert!(
        failure.contains("ConstraintHasOne") || failure.contains("2001"),
        "expected a has_one violation, got: {failure}"
    );
}

#[test]
fn a_snapshot_taken_for_another_market_cannot_resolve_this_one() {
    let mut world = World::new(NOW);

    // Two markets over the same feeds, differing only in strike. If a snapshot
    // were interchangeable, the cheaper one could be resolved from the other's.
    let first = create_market(&mut world, [1u8; 32], STRIKE_BELOW);
    let second = create_market(&mut world, [2u8; 32], 120);
    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 1);

    for market in [&first, &second] {
        let a = funded(&mut world);
        let b = funded(&mut world);
        deposit(&mut world, market, &a, true, 1_000_000);
        deposit(&mut world, market, &b, false, 1_000_000);
    }

    world.refresh_rings(SETTLE_AT, TICK_AT_100);
    world.warp_to(SETTLE_AT + 1);

    let keeper = funded(&mut world);
    snapshot(&mut world, &first, &keeper).expect("the first market snapshots");

    let failure = resolve_with(
        &mut world,
        &second,
        second.spec(),
        first.snapshot(),
        &keeper,
    )
    .expect_err("a foreign snapshot must be refused");
    assert!(
        failure.contains("ConstraintSeeds")
            || failure.contains("ConstraintHasOne")
            || failure.contains("2006")
            || failure.contains("2001"),
        "expected a seeds or has_one violation, got: {failure}"
    );
}

#[test]
fn a_spec_belonging_to_another_market_cannot_resolve_this_one() {
    let mut world = World::new(NOW);
    let first = create_market(&mut world, [1u8; 32], STRIKE_BELOW);
    let second = create_market(&mut world, [2u8; 32], 120);
    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 1);

    for market in [&first, &second] {
        let a = funded(&mut world);
        let b = funded(&mut world);
        deposit(&mut world, market, &a, true, 1_000_000);
        deposit(&mut world, market, &b, false, 1_000_000);
    }

    world.refresh_rings(SETTLE_AT, TICK_AT_100);
    world.warp_to(SETTLE_AT + 1);

    let keeper = funded(&mut world);
    snapshot(&mut world, &second, &keeper).expect("the second market snapshots");

    let failure = resolve_with(
        &mut world,
        &second,
        first.spec(),
        second.snapshot(),
        &keeper,
    )
    .expect_err("a foreign spec must be refused");
    assert!(
        failure.contains("ConstraintSeeds")
            || failure.contains("ConstraintHasOne")
            || failure.contains("2006")
            || failure.contains("2001"),
        "expected a seeds or has_one violation, got: {failure}"
    );
}

#[test]
fn a_ring_that_belongs_to_no_registered_feed_is_refused() {
    let mut world = World::new(NOW);
    let market = create_market(&mut world, [1u8; 32], STRIKE_BELOW);
    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 1);

    let a = funded(&mut world);
    let b = funded(&mut world);
    deposit(&mut world, &market, &a, true, 1_000_000);
    deposit(&mut world, &market, &b, false, 1_000_000);

    // A ring the attacker laid out themselves, showing whatever price suits.
    let rogue_pool = Pubkey::new_unique();
    let rogue_ring = Pubkey::new_unique();
    world.place_pool(rogue_pool, rogue_ring);
    world.place_ring(rogue_ring, rogue_pool, TICK_AT_100 + 20_000, SETTLE_AT);

    world.refresh_rings(SETTLE_AT, TICK_AT_100);
    world.warp_to(SETTLE_AT + 1);

    // Substitute it for the first declared feed's ring, keeping the registered
    // `Feed` account alongside it so only the ring is swapped.
    let mut accounts = prediction_market::accounts::TakeSnapshot {
        config: config_pda(),
        market: market.address,
        spec: market.spec(),
        snapshot: market.snapshot(),
        keeper: a.pubkey(),
        system_program: anchor_lang::solana_program::system_program::ID,
    }
    .to_account_metas(None);
    let mut metas = world.feed_account_metas();
    metas[1] = AccountMeta::new_readonly(rogue_ring, false);
    accounts.extend(metas);

    let failure = world
        .send(
            Instruction {
                program_id: prediction_market::ID,
                accounts,
                data: prediction_market::instruction::Snapshot {}.data(),
            },
            &[&a],
        )
        .expect_err("an unregistered ring must be refused");
    assert!(
        failure.contains("FeedAccountsMismatch"),
        "expected FeedAccountsMismatch, got: {failure}"
    );
}

// -- Spending the same position twice --------------------------------------

#[test]
fn a_winning_position_cannot_be_claimed_twice() {
    let mut world = World::new(NOW);
    let settled = settled_market(&mut world, [1u8; 32]);

    claim_with(
        &mut world,
        &settled.market,
        settled.market.vault,
        settled.market.yes_mint,
        true,
        &settled.winner,
        settled.winner_outcome,
        settled.winner_collateral,
    )
    .expect("the first claim succeeds");

    let paid = world.token_balance(&settled.winner_collateral);
    assert!(paid > 1_000_000, "the winner was paid more than principal");

    // The position was burned and its account closed, so the second attempt has
    // nothing to present.
    let second = claim_with(
        &mut world,
        &settled.market,
        settled.market.vault,
        settled.market.yes_mint,
        true,
        &settled.winner,
        settled.winner_outcome,
        settled.winner_collateral,
    );
    assert!(second.is_err(), "a position must not pay twice");
    assert_eq!(
        world.token_balance(&settled.winner_collateral),
        paid,
        "the balance must not have moved"
    );
}

#[test]
fn a_losing_position_cannot_be_claimed_as_the_winning_side() {
    let mut world = World::new(NOW);
    let settled = settled_market(&mut world, [1u8; 32]);

    // The holder owns NO tokens and asks to be paid as YES. The mint they
    // present must match the side they name, whichever way they lie.
    let failure = claim_with(
        &mut world,
        &settled.market,
        settled.market.vault,
        settled.market.no_mint,
        true,
        &settled.loser,
        settled.loser_outcome,
        settled.loser_collateral,
    )
    .expect_err("the side and its mint must agree");
    assert!(
        failure.contains("WrongState") || failure.contains("6"),
        "expected a state error, got: {failure}"
    );

    // And naming the YES mint fails too, because they hold none of it.
    let other = claim_with(
        &mut world,
        &settled.market,
        settled.market.vault,
        settled.market.yes_mint,
        true,
        &settled.loser,
        settled.loser_outcome,
        settled.loser_collateral,
    );
    assert!(other.is_err(), "a NO holder cannot present a YES position");
}

#[test]
fn the_vault_is_never_owed_more_than_it_holds() {
    let mut world = World::new(NOW);
    let settled = settled_market(&mut world, [1u8; 32]);

    let vault_before = world.token_balance(&settled.market.vault);
    assert_eq!(vault_before, 4_000_000, "both stakes are in the vault");

    claim_with(
        &mut world,
        &settled.market,
        settled.market.vault,
        settled.market.yes_mint,
        true,
        &settled.winner,
        settled.winner_outcome,
        settled.winner_collateral,
    )
    .expect("the winner claims");

    // The loser's position is worth nothing, but closing it must still succeed
    // and must not overdraw what is left.
    let _ = claim_with(
        &mut world,
        &settled.market,
        settled.market.vault,
        settled.market.no_mint,
        false,
        &settled.loser,
        settled.loser_outcome,
        settled.loser_collateral,
    );

    let paid_out = world.token_balance(&settled.winner_collateral)
        + world.token_balance(&settled.loser_collateral);
    let market = read_market(&world, &settled.market.address);
    assert!(
        paid_out + market.fee_total <= vault_before,
        "paid {paid_out} plus fees {} exceeds the {vault_before} held",
        market.fee_total
    );
}

// -- Moving a settled market ------------------------------------------------

#[test]
fn a_resolved_market_cannot_be_voided() {
    let mut world = World::new(NOW);
    let settled = settled_market(&mut world, [1u8; 32]);

    let attacker = funded(&mut world);
    world.warp_to(SETTLE_AT + 100_000);

    let failure =
        void(&mut world, &settled.market, &attacker).expect_err("a settled market is final");
    assert!(
        failure.contains("VoidConditionNotMet"),
        "expected VoidConditionNotMet, got: {failure}"
    );

    let market = read_market(&world, &settled.market.address);
    assert_eq!(
        market.status,
        prediction_market::state::MarketStatus::Resolved
    );
}

#[test]
fn a_voided_market_cannot_be_resolved_afterwards() {
    let mut world = World::new(NOW);
    let market = create_market(&mut world, [1u8; 32], STRIKE_BELOW);
    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 1);

    let a = funded(&mut world);
    let b = funded(&mut world);
    deposit(&mut world, &market, &a, true, 1_000_000);
    deposit(&mut world, &market, &b, false, 1_000_000);

    // Nobody cranks in time, so the market voids for want of a snapshot.
    world.warp_to(SETTLE_AT + i64::from(GRACE) + 1);
    void(&mut world, &market, &a).expect("an abandoned market voids");

    let failure =
        resolve(&mut world, &market, &a).expect_err("a voided market cannot then resolve");
    assert!(
        failure.contains("WrongState") || failure.contains("AccountNotInitialized"),
        "expected a state error, got: {failure}"
    );
}

#[test]
fn a_voided_market_cannot_be_snapshotted_afterwards() {
    let mut world = World::new(NOW);
    let market = create_market(&mut world, [1u8; 32], STRIKE_BELOW);
    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 1);

    let a = funded(&mut world);
    let b = funded(&mut world);
    deposit(&mut world, &market, &a, true, 1_000_000);
    deposit(&mut world, &market, &b, false, 1_000_000);

    world.warp_to(SETTLE_AT + i64::from(GRACE) + 1);
    void(&mut world, &market, &a).expect("an abandoned market voids");

    world.refresh_rings(world.now(), TICK_AT_100);
    let failure = snapshot(&mut world, &market, &a).expect_err("a voided market takes no snapshot");
    assert!(
        failure.contains("WrongState") || failure.contains("OutsideSnapshotWindow"),
        "expected a state error, got: {failure}"
    );
}

// -- Taking over governance -------------------------------------------------

#[test]
fn a_stranger_cannot_accept_an_authority_nomination() {
    let mut world = World::new(NOW);
    let heir = Keypair::new();
    let attacker = funded(&mut world);

    let authority = world.authority.insecure_clone();
    world.must_send(
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::Governance {
                config: config_pda(),
                authority: authority.pubkey(),
            }
            .to_account_metas(None),
            data: prediction_market::instruction::NominateAuthority {
                next: heir.pubkey(),
            }
            .data(),
        },
        &[&authority],
    );

    let failure = world
        .send(
            Instruction {
                program_id: prediction_market::ID,
                accounts: prediction_market::accounts::AcceptAuthority {
                    config: config_pda(),
                    next_authority: attacker.pubkey(),
                }
                .to_account_metas(None),
                data: prediction_market::instruction::AcceptAuthority {}.data(),
            },
            &[&attacker],
        )
        .expect_err("only the nominee may accept");
    assert!(
        failure.contains("NotAuthorized") || failure.contains("ConstraintAddress"),
        "expected an authorisation failure, got: {failure}"
    );
}

#[test]
fn a_stranger_cannot_pause_the_protocol() {
    let mut world = World::new(NOW);
    let attacker = funded(&mut world);

    let failure = world
        .send(
            Instruction {
                program_id: prediction_market::ID,
                accounts: prediction_market::accounts::Governance {
                    config: config_pda(),
                    authority: attacker.pubkey(),
                }
                .to_account_metas(None),
                data: prediction_market::instruction::SetPaused { paused: true }.data(),
            },
            &[&attacker],
        )
        .expect_err("only governance may pause");
    assert!(
        failure.contains("NotAuthorized") || failure.contains("ConstraintHasOne"),
        "expected an authorisation failure, got: {failure}"
    );
}

#[test]
fn parameters_cannot_be_adopted_before_their_timelock_expires() {
    let mut world = World::new(NOW);
    let authority = world.authority.insecure_clone();

    let mut proposed = world.params();
    proposed.fee_bps = 400;

    world.must_send(
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::Governance {
                config: config_pda(),
                authority: authority.pubkey(),
            }
            .to_account_metas(None),
            data: prediction_market::instruction::ProposeParams { params: proposed }.data(),
        },
        &[&authority],
    );

    let failure = world
        .send(
            Instruction {
                program_id: prediction_market::ID,
                accounts: prediction_market::accounts::Governance {
                    config: config_pda(),
                    authority: authority.pubkey(),
                }
                .to_account_metas(None),
                data: prediction_market::instruction::AdoptParams {}.data(),
            },
            &[&authority],
        )
        .expect_err("the timelock has not run");
    assert!(
        failure.contains("TimelockPending"),
        "expected TimelockPending, got: {failure}"
    );
}

#[test]
fn a_market_keeps_the_parameters_it_was_created_under() {
    let mut world = World::new(NOW);
    let market = create_market(&mut world, [1u8; 32], STRIKE_BELOW);
    let original_fee = read_market(&world, &market.address).params.fee_bps;

    // Governance raises the fee and waits out the timelock. The market that
    // already holds money must be unaffected.
    let authority = world.authority.insecure_clone();
    let mut proposed = world.params();
    proposed.fee_bps = 400;
    world.must_send(
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::Governance {
                config: config_pda(),
                authority: authority.pubkey(),
            }
            .to_account_metas(None),
            data: prediction_market::instruction::ProposeParams { params: proposed }.data(),
        },
        &[&authority],
    );
    world.warp_to(NOW + 100_000);
    world.must_send(
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::Governance {
                config: config_pda(),
                authority: authority.pubkey(),
            }
            .to_account_metas(None),
            data: prediction_market::instruction::AdoptParams {}.data(),
        },
        &[&authority],
    );

    assert_eq!(
        read_market(&world, &market.address).params.fee_bps,
        original_fee,
        "an adopted parameter must not reach a market that already exists"
    );
}

// -- Working around a limit -------------------------------------------------

#[test]
fn the_per_side_cap_cannot_be_split_across_many_deposits() {
    let mut world = World::new(NOW);
    let market = create_market(&mut world, [1u8; 32], STRIKE_BELOW);
    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 1);

    let cap = read_market(&world, &market.address).cap_per_side;
    assert!(cap > 0, "the market declares a cap");

    // Fill the side to its limit, then try the smallest legal stake from fresh
    // accounts. The cap is on the side's total, not on any one depositor.
    let whale = funded(&mut world);
    deposit(&mut world, &market, &whale, true, cap);
    assert_eq!(read_market(&world, &market.address).staked_yes, cap);

    for _ in 0..3 {
        let straw = funded(&mut world);
        let collateral =
            world.place_token_account(world.collateral_mint, straw.pubkey(), MIN_STAKE);
        let outcome = world.place_token_account(market.yes_mint, straw.pubkey(), 0);
        let failure = world
            .send(
                Instruction {
                    program_id: prediction_market::ID,
                    accounts: prediction_market::accounts::Deposit {
                        market: market.address,
                        collateral: collateral_pda(&world.collateral_mint),
                        collateral_mint: world.collateral_mint,
                        vault: market.vault,
                        side_mint: market.yes_mint,
                        depositor_collateral: collateral,
                        depositor_outcome: outcome,
                        depositor: straw.pubkey(),
                        token_program: spl_token::ID,
                    }
                    .to_account_metas(None),
                    data: prediction_market::instruction::Deposit {
                        side_is_yes: true,
                        amount: MIN_STAKE,
                    }
                    .data(),
                },
                &[&straw],
            )
            .expect_err("the cap is on the side, not on the depositor");
        assert!(
            failure.contains("CapExceeded"),
            "expected CapExceeded, got: {failure}"
        );
    }

    assert_eq!(
        read_market(&world, &market.address).staked_yes,
        cap,
        "nothing got past the cap"
    );
}

#[test]
fn swept_dust_cannot_be_redirected_away_from_the_treasury() {
    let mut world = World::new(NOW);
    let settled = settled_market(&mut world, [1u8; 32]);

    // Long enough that claiming has closed and the sweep is legitimate.
    world.warp_to(SETTLE_AT + 200 * 86_400);

    let attacker = funded(&mut world);
    let their_account = world.place_token_account(world.collateral_mint, attacker.pubkey(), 0);

    let failure = world
        .send(
            Instruction {
                program_id: prediction_market::ID,
                accounts: prediction_market::accounts::SweepDust {
                    config: config_pda(),
                    market: settled.market.address,
                    vault: settled.market.vault,
                    treasury_token: their_account,
                    caller: attacker.pubkey(),
                    token_program: spl_token::ID,
                }
                .to_account_metas(None),
                data: prediction_market::instruction::SweepDust {}.data(),
            },
            &[&attacker],
        )
        .expect_err("dust belongs to the treasury");
    assert!(
        failure.contains("NotAuthorized"),
        "expected NotAuthorized, got: {failure}"
    );
    assert_eq!(world.token_balance(&their_account), 0);
}

// -- Governance levers that had no coverage --------------------------------

fn update_feed(
    world: &mut World,
    ring: &Pubkey,
    depth_quote: u64,
    enabled: bool,
    signer: &Keypair,
) -> Result<(), String> {
    world.send(
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::UpdateFeed {
                config: config_pda(),
                authority: signer.pubkey(),
                feed: feed_pda(ring),
            }
            .to_account_metas(None),
            data: prediction_market::instruction::UpdateFeed {
                depth_quote,
                enabled,
            }
            .data(),
        },
        &[signer],
    )
}

fn read_feed(world: &World, ring: &Pubkey) -> prediction_market::state::Feed {
    let account = world.svm.get_account(&feed_pda(ring)).expect("feed exists");
    prediction_market::state::Feed::deserialize(&mut &account.data[8..]).expect("decodes")
}

#[test]
fn a_stranger_cannot_re_attest_a_feeds_depth() {
    let mut world = World::new(NOW);
    let ring = world.rings[0];
    let before = read_feed(&world, &ring).depth_quote;

    let attacker = funded(&mut world);
    let failure = update_feed(&mut world, &ring, 1, true, &attacker)
        .expect_err("depth is governance's to attest");
    assert!(
        failure.contains("NotAuthorized"),
        "expected NotAuthorized, got: {failure}"
    );
    assert_eq!(read_feed(&world, &ring).depth_quote, before);
}

#[test]
fn a_disabled_feed_cannot_back_a_new_market() {
    let mut world = World::new(NOW);
    let ring = world.rings[0];
    let authority = world.authority.insecure_clone();

    // The kill switch: a source that has gone wrong is withdrawn from use.
    update_feed(&mut world, &ring, 1_000_000, false, &authority).expect("governance may disable");
    assert!(!read_feed(&world, &ring).enabled);

    let mut accounts = prediction_market::accounts::CreateMarket {
        config: config_pda(),
        collateral: collateral_pda(&world.collateral_mint),
        mint: world.collateral_mint,
        collateral_mint: world.collateral_mint,
        market: market_pda(&[9u8; 32]),
        spec: child_pda(b"spec", &market_pda(&[9u8; 32])),
        vault: child_pda(b"vault", &market_pda(&[9u8; 32])),
        yes_mint: child_pda(b"yes", &market_pda(&[9u8; 32])),
        no_mint: child_pda(b"no", &market_pda(&[9u8; 32])),
        creator: authority.pubkey(),
        treasury: world.treasury,
        token_program: spl_token::ID,
        system_program: anchor_lang::solana_program::system_program::ID,
    }
    .to_account_metas(None);
    accounts.extend(
        world
            .rings
            .iter()
            .map(|r| AccountMeta::new_readonly(feed_pda(r), false)),
    );

    let failure = world
        .send(
            Instruction {
                program_id: prediction_market::ID,
                accounts,
                data: prediction_market::instruction::CreateMarket {
                    args: prediction_market::instructions::CreateMarketArgs {
                        market_id: [9u8; 32],
                        settle_at: SETTLE_AT,
                        strike: Q64::from_int(STRIKE_BELOW).raw(),
                        ramp_bps: RAMP_BPS,
                        feeds: world.feed_refs(),
                        bytecode: median_of_three(),
                        rules_uri: "https://example.test/rules/1".to_string(),
                    },
                }
                .data(),
            },
            &[&authority],
        )
        .expect_err("a disabled source may not back a market");
    assert!(
        failure.contains("FeedNotActive"),
        "expected FeedNotActive, got: {failure}"
    );
}

#[test]
fn re_attested_depth_does_not_reach_markets_that_already_exist() {
    let mut world = World::new(NOW);
    let market = create_market(&mut world, [1u8; 32], STRIKE_BELOW);
    let cap_before = read_market(&world, &market.address).cap_per_side;

    // Governance halves what it says the thinnest source is worth. A market
    // already holding money keeps the cap it was created under.
    let authority = world.authority.insecure_clone();
    let ring = world.rings[0];
    let depth = read_feed(&world, &ring).depth_quote;
    update_feed(&mut world, &ring, depth / 2, true, &authority).expect("governance may re-attest");

    assert_eq!(
        read_market(&world, &market.address).cap_per_side,
        cap_before,
        "an existing market's cap must not move"
    );

    // A market created afterwards is capped against the new figure.
    world.warp_to(NOW + 1);
    let later = create_market(&mut world, [2u8; 32], STRIKE_BELOW);
    assert!(
        read_market(&world, &later.address).cap_per_side < cap_before,
        "a market created after the change should be capped lower"
    );
}

#[test]
fn a_stranger_cannot_withdraw_a_collateral_mint() {
    let mut world = World::new(NOW);
    let attacker = funded(&mut world);
    let collateral = collateral_pda(&world.collateral_mint);

    let failure = world
        .send(
            Instruction {
                program_id: prediction_market::ID,
                accounts: prediction_market::accounts::UpdateCollateral {
                    config: config_pda(),
                    authority: attacker.pubkey(),
                    collateral,
                }
                .to_account_metas(None),
                data: prediction_market::instruction::UpdateCollateral {
                    min_stake: 1,
                    enabled: false,
                }
                .data(),
            },
            &[&attacker],
        )
        .expect_err("collateral approval is governance's");
    assert!(
        failure.contains("NotAuthorized"),
        "expected NotAuthorized, got: {failure}"
    );
}

#[test]
fn withdrawing_a_collateral_mint_never_strands_money_already_deposited() {
    let mut world = World::new(NOW);
    let settled = settled_market(&mut world, [1u8; 32]);

    // Governance withdraws the mint *after* people have staked. Paying out what
    // is already in the vault must not depend on the mint still being approved.
    let authority = world.authority.insecure_clone();
    world.must_send(
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::UpdateCollateral {
                config: config_pda(),
                authority: authority.pubkey(),
                collateral: collateral_pda(&world.collateral_mint),
            }
            .to_account_metas(None),
            data: prediction_market::instruction::UpdateCollateral {
                min_stake: 1_000,
                enabled: false,
            }
            .data(),
        },
        &[&authority],
    );

    claim_with(
        &mut world,
        &settled.market,
        settled.market.vault,
        settled.market.yes_mint,
        true,
        &settled.winner,
        settled.winner_outcome,
        settled.winner_collateral,
    )
    .expect("a winner must still be paid");
    assert!(world.token_balance(&settled.winner_collateral) > 1_000_000);
}
