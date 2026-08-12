//! What each instruction costs, pinned down.
//!
//! Compute is correctness here, not performance: a transaction gets 200 000
//! units unless it asks for more, and a snapshot over three rings does not fit.
//! If settling ever stops fitting the 1 400 000 a transaction may request,
//! markets become unsettleable and all of them void. Nothing on a host machine
//! has a compute meter, so these numbers are asserted rather than observed.

mod harness;

use anchor_lang::{InstructionData, ToAccountMetas};
use harness::*;
use market_math::Q64;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signature::Keypair, signature::Signer};

const NOW: i64 = 1_800_000_000;
const SETTLE_AT: i64 = NOW + 2_000;

/// The ceiling a transaction may request, and therefore the hard limit.
const TRANSACTION_LIMIT: u64 = 1_400_000;

/// What a client should ask for when settling. Generous against the measured
/// cost, and still comfortably inside the ceiling.
const RECOMMENDED_SNAPSHOT_BUDGET: u64 = 400_000;

fn create_market(world: &mut World, feeds: usize) -> (Pubkey, Pubkey, Pubkey, Pubkey) {
    let id = [77u8; 32];
    let address = market_pda(&id);
    let references: Vec<_> = world.feed_refs().into_iter().take(feeds).collect();

    let mut accounts = prediction_market::accounts::CreateMarket {
        config: config_pda(),
        collateral: collateral_pda(&world.collateral_mint),
        mint: world.collateral_mint,
        collateral_mint: world.collateral_mint,
        market: address,
        spec: child_pda(b"spec", &address),
        vault: child_pda(b"vault", &address),
        yes_mint: child_pda(b"yes", &address),
        no_mint: child_pda(b"no", &address),
        creator: world.authority.pubkey(),
        treasury: world.treasury,
        token_program: spl_token::ID,
        system_program: anchor_lang::solana_program::system_program::ID,
    }
    .to_account_metas(None);
    accounts.extend(references.iter().map(|reference| {
        solana_sdk::instruction::AccountMeta::new_readonly(reference.feed, false)
    }));

    let instruction = Instruction {
        program_id: prediction_market::ID,
        accounts,
        data: prediction_market::instruction::CreateMarket {
            args: prediction_market::instructions::CreateMarketArgs {
                market_id: id,
                settle_at: SETTLE_AT,
                strike: Q64::from_int(90).raw(),
                ramp_bps: RAMP_BPS,
                feeds: references,
                bytecode: median_of_three(),
                rules_uri: "https://example.test/rules/budget".to_string(),
            },
        }
        .data(),
    };
    let authority = world.authority.insecure_clone();
    world.must_send(instruction, &[&authority]);
    (
        address,
        child_pda(b"vault", &address),
        child_pda(b"yes", &address),
        child_pda(b"no", &address),
    )
}

#[test]
fn settling_a_market_fits_inside_a_transaction_with_room_to_spare() {
    let mut world = World::new(NOW);
    let (market, vault, yes_mint, no_mint) = create_market(&mut world, 3);

    let alice = Keypair::new();
    let bob = Keypair::new();
    world.svm.airdrop(&alice.pubkey(), 1_000_000_000).unwrap();
    world.svm.airdrop(&bob.pubkey(), 1_000_000_000).unwrap();
    world.warp_to(NOW + i64::from(CREATION_COOLDOWN) + 10);

    let deposit_cost = {
        let collateral =
            world.place_token_account(world.collateral_mint, alice.pubkey(), 1_000_000);
        let outcome = world.place_token_account(yes_mint, alice.pubkey(), 0);
        world.measure(
            Instruction {
                program_id: prediction_market::ID,
                accounts: prediction_market::accounts::Deposit {
                    market,
                    collateral: collateral_pda(&world.collateral_mint),
                    collateral_mint: world.collateral_mint,
                    vault,
                    side_mint: yes_mint,
                    depositor_collateral: collateral,
                    depositor_outcome: outcome,
                    depositor: alice.pubkey(),
                    token_program: spl_token::ID,
                }
                .to_account_metas(None),
                data: prediction_market::instruction::Deposit {
                    side_is_yes: true,
                    amount: 1_000_000,
                }
                .data(),
            },
            &[&alice],
        )
    };
    {
        let collateral = world.place_token_account(world.collateral_mint, bob.pubkey(), 1_000_000);
        let outcome = world.place_token_account(no_mint, bob.pubkey(), 0);
        world.must_send(
            Instruction {
                program_id: prediction_market::ID,
                accounts: prediction_market::accounts::Deposit {
                    market,
                    collateral: collateral_pda(&world.collateral_mint),
                    collateral_mint: world.collateral_mint,
                    vault,
                    side_mint: no_mint,
                    depositor_collateral: collateral,
                    depositor_outcome: outcome,
                    depositor: bob.pubkey(),
                    token_program: spl_token::ID,
                }
                .to_account_metas(None),
                data: prediction_market::instruction::Deposit {
                    side_is_yes: false,
                    amount: 1_000_000,
                }
                .data(),
            },
            &[&bob],
        );
    }

    world.refresh_rings(SETTLE_AT, TICK_AT_100);
    world.warp_to(SETTLE_AT + 10);
    let keeper = Keypair::new();
    world.svm.airdrop(&keeper.pubkey(), 1_000_000_000).unwrap();

    let mut accounts = prediction_market::accounts::TakeSnapshot {
        config: config_pda(),
        market,
        spec: child_pda(b"spec", &market),
        snapshot: child_pda(b"snapshot", &market),
        keeper: keeper.pubkey(),
        system_program: anchor_lang::solana_program::system_program::ID,
    }
    .to_account_metas(None);
    accounts.extend(world.feed_account_metas());
    let snapshot_cost = world.measure(
        Instruction {
            program_id: prediction_market::ID,
            accounts,
            data: prediction_market::instruction::Snapshot {}.data(),
        },
        &[&keeper],
    );

    let resolve_cost = world.measure(
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::Resolve {
                market,
                spec: child_pda(b"spec", &market),
                snapshot: child_pda(b"snapshot", &market),
                resolver: keeper.pubkey(),
            }
            .to_account_metas(None),
            data: prediction_market::instruction::Resolve {}.data(),
        },
        &[&keeper],
    );

    // Printed so a change is visible in the log even when it stays in bounds.
    println!(
        "deposit {deposit_cost} CU, snapshot (3 rings) {snapshot_cost} CU, resolve {resolve_cost} CU"
    );

    // Snapshot is the expensive one: every segment of every ring is validated,
    // which is three hundred integer divisions before a price is even computed.
    assert!(
        snapshot_cost > 200_000,
        "snapshot now fits in the default budget ({snapshot_cost} CU) -- if that is \
         deliberate the documentation and the keeper should stop asking for more"
    );
    assert!(
        snapshot_cost < RECOMMENDED_SNAPSHOT_BUDGET,
        "snapshot costs {snapshot_cost} CU, above the {RECOMMENDED_SNAPSHOT_BUDGET} \
         a client is told to request"
    );

    // Eight feeds is the ceiling a market may declare. Extrapolating from the
    // three measured here, that has to stay inside what a transaction can ask
    // for, or the largest markets would be unsettleable and would all void.
    let per_feed = snapshot_cost / 3;
    let eight_feeds = per_feed * 8 + 20_000;
    assert!(
        eight_feeds < TRANSACTION_LIMIT,
        "a market with the maximum eight feeds would need about {eight_feeds} CU"
    );

    // The cheap instructions must stay cheap: an ordinary bet should never
    // need a client to think about compute at all.
    assert!(deposit_cost < 50_000, "deposit costs {deposit_cost} CU");
    assert!(resolve_cost < 100_000, "resolve costs {resolve_cost} CU");
}
