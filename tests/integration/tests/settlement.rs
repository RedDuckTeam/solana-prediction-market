//! The paths that move money out: fees, dust, rent, and the creator's bond.
//!
//! These run last in a market's life and are the least exercised in practice,
//! which is why they get their own file. Everything here is about who is
//! *allowed* to receive something, and what happens to value nobody claimed.

mod harness;

use anchor_lang::{AnchorDeserialize, InstructionData, ToAccountMetas};
use harness::*;
use market_math::Q64;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signature::Keypair, signature::Signer};

const NOW: i64 = 1_800_000_000;
const SETTLE_AT: i64 = NOW + 2_000;
const CLAIM_WINDOW: i64 = 90 * 86_400;

struct Market {
    address: Pubkey,
    yes_mint: Pubkey,
    no_mint: Pubkey,
    vault: Pubkey,
}

fn create_market(world: &mut World, id_byte: u8) -> Market {
    let id = [id_byte; 32];
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
                strike: Q64::from_int(90).raw(),
                ramp_bps: RAMP_BPS,
                feeds: world.feed_refs(),
                bytecode: median_of_three(),
                rules_uri: "https://example.test/rules/settlement".to_string(),
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

fn settle(world: &mut World, market: &Market) -> Keypair {
    world.refresh_rings(SETTLE_AT, TICK_AT_100);
    world.warp_to(SETTLE_AT + 10);
    let keeper = Keypair::new();
    world.svm.airdrop(&keeper.pubkey(), 1_000_000_000).unwrap();

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
    world.must_send(
        Instruction {
            program_id: prediction_market::ID,
            accounts,
            data: prediction_market::instruction::Snapshot {}.data(),
        },
        &[&keeper],
    );

    world.must_send(
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::Resolve {
                market: market.address,
                spec: child_pda(b"spec", &market.address),
                snapshot: child_pda(b"snapshot", &market.address),
                resolver: keeper.pubkey(),
            }
            .to_account_metas(None),
            data: prediction_market::instruction::Resolve {}.data(),
        },
        &[&keeper],
    );
    keeper
}

fn collect_fee(
    world: &mut World,
    market: &Market,
    recipient: prediction_market::instructions::FeeRecipient,
    destination: Pubkey,
    caller: &Keypair,
) -> Result<(), String> {
    world.send(
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::CollectFee {
                config: config_pda(),
                market: market.address,
                vault: market.vault,
                destination,
                caller: caller.pubkey(),
                token_program: spl_token::ID,
            }
            .to_account_metas(None),
            data: prediction_market::instruction::CollectFee { recipient }.data(),
        },
        &[caller],
    )
}

fn read_market(world: &World, address: &Pubkey) -> prediction_market::state::Market {
    let account = world.svm.get_account(address).expect("market exists");
    prediction_market::state::Market::deserialize(&mut &account.data[8..]).expect("market decodes")
}

#[test]
fn every_fee_share_reaches_the_party_it_belongs_to() {
    let mut world = World::new(NOW);
    let market = create_market(&mut world, 21);

    let alice = Keypair::new();
    let bob = Keypair::new();
    world.svm.airdrop(&alice.pubkey(), 1_000_000_000).unwrap();
    world.svm.airdrop(&bob.pubkey(), 1_000_000_000).unwrap();
    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 10);
    deposit(&mut world, &market, &alice, true, 1_000_000);
    deposit(&mut world, &market, &bob, false, 4_000_000);

    let keeper = settle(&mut world, &market);
    let state = read_market(&world, &market.address);
    assert_eq!(
        state.fee_total, 80_000,
        "two percent of the four million that moved"
    );

    // The three shares add back to the fee exactly: the keeper's cut absorbs
    // whatever the other two floors left behind.
    assert_eq!(
        state.fee_owed_treasury + state.fee_owed_creator + state.fee_owed_keeper,
        state.fee_total
    );

    let treasury_account = world.place_token_account(world.collateral_mint, world.treasury, 0);
    let creator_account =
        world.place_token_account(world.collateral_mint, world.authority.pubkey(), 0);
    let keeper_account = world.place_token_account(world.collateral_mint, keeper.pubkey(), 0);

    use prediction_market::instructions::FeeRecipient;
    collect_fee(
        &mut world,
        &market,
        FeeRecipient::Treasury,
        treasury_account,
        &keeper,
    )
    .unwrap();
    collect_fee(
        &mut world,
        &market,
        FeeRecipient::Creator,
        creator_account,
        &keeper,
    )
    .unwrap();
    collect_fee(
        &mut world,
        &market,
        FeeRecipient::Keeper,
        keeper_account,
        &keeper,
    )
    .unwrap();

    assert_eq!(
        world.token_balance(&treasury_account),
        state.fee_owed_treasury
    );
    assert_eq!(
        world.token_balance(&creator_account),
        state.fee_owed_creator
    );
    assert_eq!(world.token_balance(&keeper_account), state.fee_owed_keeper);

    // Collected once, and only once.
    assert!(
        collect_fee(
            &mut world,
            &market,
            FeeRecipient::Treasury,
            treasury_account,
            &keeper
        )
        .is_err(),
        "a share already paid is no longer owed"
    );
}

#[test]
fn a_fee_cannot_be_redirected_to_someone_it_is_not_owed_to() {
    let mut world = World::new(NOW);
    let market = create_market(&mut world, 22);

    let alice = Keypair::new();
    let bob = Keypair::new();
    world.svm.airdrop(&alice.pubkey(), 1_000_000_000).unwrap();
    world.svm.airdrop(&bob.pubkey(), 1_000_000_000).unwrap();
    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 10);
    deposit(&mut world, &market, &alice, true, 1_000_000);
    deposit(&mut world, &market, &bob, false, 4_000_000);
    let keeper = settle(&mut world, &market);

    // Anyone may push a payout -- it is permissionless on purpose -- but the
    // destination has to belong to the party the share is owed to.
    let thief = Keypair::new();
    let thief_account = world.place_token_account(world.collateral_mint, thief.pubkey(), 0);
    use prediction_market::instructions::FeeRecipient;
    assert!(collect_fee(
        &mut world,
        &market,
        FeeRecipient::Treasury,
        thief_account,
        &keeper
    )
    .is_err());
    assert!(collect_fee(
        &mut world,
        &market,
        FeeRecipient::Creator,
        thief_account,
        &keeper
    )
    .is_err());
    assert!(collect_fee(
        &mut world,
        &market,
        FeeRecipient::Keeper,
        thief_account,
        &keeper
    )
    .is_err());
    assert_eq!(world.token_balance(&thief_account), 0);
}

#[test]
fn dust_is_swept_only_after_claiming_closes() {
    let mut world = World::new(NOW);
    let market = create_market(&mut world, 23);

    let alice = Keypair::new();
    let bob = Keypair::new();
    world.svm.airdrop(&alice.pubkey(), 1_000_000_000).unwrap();
    world.svm.airdrop(&bob.pubkey(), 1_000_000_000).unwrap();
    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 10);
    // Amounts chosen so the pro-rata division cannot come out even.
    deposit(&mut world, &market, &alice, true, 333_333);
    deposit(&mut world, &market, &bob, false, 1_000_000);
    let keeper = settle(&mut world, &market);

    let treasury_account = world.place_token_account(world.collateral_mint, world.treasury, 0);
    let sweep = |world: &mut World| {
        world.send(
            Instruction {
                program_id: prediction_market::ID,
                accounts: prediction_market::accounts::SweepDust {
                    config: config_pda(),
                    market: market.address,
                    vault: market.vault,
                    treasury_token: treasury_account,
                    caller: keeper.pubkey(),
                    token_program: spl_token::ID,
                }
                .to_account_metas(None),
                data: prediction_market::instruction::SweepDust {}.data(),
            },
            &[&keeper],
        )
    };

    assert!(
        sweep(&mut world).is_err(),
        "holders must have their full window to claim first"
    );

    let resolved_at = read_market(&world, &market.address).resolved_at;
    world.warp_to(resolved_at + CLAIM_WINDOW + 1);
    sweep(&mut world).expect("sweeping is allowed once claiming has closed");

    // Everything left in the vault -- rounding dust and anything never claimed
    // -- ends up with the treasury, and the vault is empty.
    assert_eq!(world.token_balance(&market.vault), 0);
    assert!(world.token_balance(&treasury_account) > 0);
    assert!(sweep(&mut world).is_err(), "there is nothing left to sweep");
}

#[test]
fn a_donation_to_the_vault_cannot_brick_a_settlement() {
    let mut world = World::new(NOW);
    let market = create_market(&mut world, 24);

    let alice = Keypair::new();
    let bob = Keypair::new();
    world.svm.airdrop(&alice.pubkey(), 1_000_000_000).unwrap();
    world.svm.airdrop(&bob.pubkey(), 1_000_000_000).unwrap();
    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 10);
    let (alice_collateral, alice_outcome) = deposit(&mut world, &market, &alice, true, 1_000_000);
    deposit(&mut world, &market, &bob, false, 1_000_000);

    // Anyone can transfer into the vault. If the program ever compared its
    // balance against an expected figure, one lamport would stop every payout.
    world.credit_token_account(market.vault, 1);

    settle(&mut world, &market);
    world
        .send(
            Instruction {
                program_id: prediction_market::ID,
                accounts: prediction_market::accounts::Claim {
                    market: market.address,
                    vault: market.vault,
                    side_mint: market.yes_mint,
                    holder_outcome: alice_outcome,
                    holder_collateral: alice_collateral,
                    holder: alice.pubkey(),
                    token_program: spl_token::ID,
                }
                .to_account_metas(None),
                data: prediction_market::instruction::Claim { side_is_yes: true }.data(),
            },
            &[&alice],
        )
        .expect("the winner is paid regardless of the donation");
    assert!(world.token_balance(&alice_collateral) > 1_000_000);
}

#[test]
fn a_void_never_forfeits_the_bond() {
    // The bond is escrow for the cranks, not a fine. From here a snapshot
    // missed for want of a keeper and one missed because a feed could not be
    // read are indistinguishable -- the ring that could have told them apart
    // is overwritten by the time anyone asks -- so forfeiting on
    // `SnapshotMissed` would fine creators for feed failures and for
    // governance disabling a feed under them. Whatever the cranks did not
    // spend goes back with the rent at `close_market`; the spam deterrent is
    // the non-refundable creation fee.
    let void_case = |empty_side: bool| {
        let mut world = World::new(NOW);
        let market = create_market(&mut world, if empty_side { 25 } else { 26 });

        let alice = Keypair::new();
        world.svm.airdrop(&alice.pubkey(), 1_000_000_000).unwrap();
        world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 10);
        deposit(&mut world, &market, &alice, true, 1_000_000);
        if !empty_side {
            let bob = Keypair::new();
            world.svm.airdrop(&bob.pubkey(), 1_000_000_000).unwrap();
            deposit(&mut world, &market, &bob, false, 1_000_000);
        }

        // Let the whole grace period lapse without a snapshot.
        world.warp_to(SETTLE_AT + i64::from(GRACE) + 1);
        let caller = Keypair::new();
        world.svm.airdrop(&caller.pubkey(), 1_000_000_000).unwrap();

        let treasury_before = world
            .svm
            .get_account(&world.treasury)
            .map(|account| account.lamports)
            .unwrap_or_default();
        world
            .send(
                Instruction {
                    program_id: prediction_market::ID,
                    accounts: prediction_market::accounts::VoidMarket {
                        market: market.address,
                        caller: caller.pubkey(),
                    }
                    .to_account_metas(None),
                    data: prediction_market::instruction::Void {}.data(),
                },
                &[&caller],
            )
            .expect("void succeeds");
        let treasury_after = world
            .svm
            .get_account(&world.treasury)
            .map(|account| account.lamports)
            .unwrap_or_default();
        (
            treasury_after - treasury_before,
            read_market(&world, &market.address),
        )
    };

    // One side empty: known before settlement, nobody's fault.
    let (gained, state) = void_case(true);
    assert_eq!(
        state.status_reason,
        prediction_market::state::VoidCause::EmptySide
    );
    assert_eq!(gained, 0, "an empty side moves nothing to the treasury");
    assert!(state.bond_lamports > 0, "the bond is still the creator's");

    // Both sides staked and nobody cranked: still the creator's bond, because
    // an unreadable feed reaches this exact state and cannot be told apart.
    let (gained, state) = void_case(false);
    assert_eq!(
        state.status_reason,
        prediction_market::state::VoidCause::SnapshotMissed
    );
    assert_eq!(gained, 0, "a missed snapshot is not a fine");
    assert!(state.bond_lamports > 0, "the bond returns with the rent");
}

#[test]
fn an_aborting_predicate_voids_and_still_pays_the_resolver() {
    // The resolver's reward cannot depend on the outcome. If only a clean
    // resolution paid, nobody neutral would crank a market whose predicate
    // aborts, and settling it would fall to whichever side the void favours.
    let mut world = World::new(NOW);

    let id = [77u8; 32];
    let address = market_pda(&id);
    let vault = child_pda(b"vault", &address);

    // Statically valid -- reads every input, leaves one number -- but divides
    // by zero at runtime, which is exactly the failure that must void.
    let mut buffer = [0u8; 64];
    let mut encoder = market_vm::Encoder::new(&mut buffer);
    encoder
        .push_input(0)
        .and_then(|encoder| encoder.push_input(1))
        .and_then(|encoder| encoder.push_input(2))
        .and_then(|encoder| encoder.median(3))
        .and_then(|encoder| encoder.push_const(Q64::ZERO))
        .and_then(|encoder| encoder.op(market_vm::Op::Div))
        .expect("encoding fits");
    let aborting = encoder.code().to_vec();

    let mut accounts = prediction_market::accounts::CreateMarket {
        config: config_pda(),
        collateral: collateral_pda(&world.collateral_mint),
        mint: world.collateral_mint,
        collateral_mint: world.collateral_mint,
        market: address,
        spec: child_pda(b"spec", &address),
        vault,
        yes_mint: child_pda(b"yes", &address),
        no_mint: child_pda(b"no", &address),
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
    let authority = world.authority.insecure_clone();
    world.must_send(
        Instruction {
            program_id: prediction_market::ID,
            accounts,
            data: prediction_market::instruction::CreateMarket {
                args: prediction_market::instructions::CreateMarketArgs {
                    market_id: id,
                    settle_at: SETTLE_AT,
                    strike: Q64::from_int(90).raw(),
                    ramp_bps: RAMP_BPS,
                    feeds: world.feed_refs(),
                    bytecode: aborting,
                    rules_uri: "https://example.test/rules/aborting".to_string(),
                },
            }
            .data(),
        },
        &[&authority],
    );
    let market = Market {
        address,
        yes_mint: child_pda(b"yes", &address),
        no_mint: child_pda(b"no", &address),
        vault,
    };

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
    world.must_send(
        Instruction {
            program_id: prediction_market::ID,
            accounts,
            data: prediction_market::instruction::Snapshot {}.data(),
        },
        &[&keeper],
    );

    // A separate resolver, so the payment is visible on its own balance.
    let resolver = Keypair::new();
    world
        .svm
        .airdrop(&resolver.pubkey(), 1_000_000_000)
        .unwrap();
    let before = world.svm.get_account(&resolver.pubkey()).unwrap().lamports;
    world.must_send(
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
        &[&resolver],
    );
    let after = world.svm.get_account(&resolver.pubkey()).unwrap().lamports;

    let state = read_market(&world, &market.address);
    assert_eq!(state.status, prediction_market::state::MarketStatus::Void);
    assert_eq!(
        state.status_reason,
        prediction_market::state::VoidCause::PredicateAborted
    );
    // Reward received, minus the transaction fee the resolver paid to send it.
    assert!(
        after + 100_000 > before + KEEPER_REWARD,
        "the resolver of a voiding market was not paid: {before} -> {after}"
    );
    assert_eq!(
        state.bond_lamports, 0,
        "both cranks were paid from the bond"
    );
}
