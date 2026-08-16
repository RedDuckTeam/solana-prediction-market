//! The whole protocol, once, against a running validator.
//!
//! ```text
//! anchor build && cargo test -p e2e-tests --test localnet -- --ignored --nocapture
//! ```
//!
//! Boots a node and waits out real minutes: the chain's clock cannot be warped.
//! One test rather than many, because the clock only moves forward and so the
//! order of events is the fixture -- splitting it would need a validator per
//! phase and would lose the only thing this proves over the in-process suite.
//!
//! Expected refusals are checked in place, where they become expected.

mod support {
    pub mod rpc;
    pub mod validator;
}
use support::validator;

use anchor_lang::{AnchorDeserialize, InstructionData, ToAccountMetas};
use market_math::Q64;
use solana_compute_budget_interface::ComputeBudgetInstruction;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    native_token::LAMPORTS_PER_SOL,
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use anchor_lang::solana_program::system_instruction;
use validator::*;

use prediction_market::instructions::FeeRecipient;
use prediction_market::state::{FeedRef, MarketStatus, VoidCause};
use validator::COLLATERAL_DECIMALS;

const STAKE_YES: u64 = 1_000_000;
const STAKE_NO: u64 = 4_000_000;
/// Far below the pool's ~100, so the ramp saturates and YES takes the pot.
const STRIKE: i64 = 90;

struct Client<'a> {
    net: &'a Localnet,
}

impl Client<'_> {
    fn send(&self, instruction: Instruction, signers: &[&Keypair]) -> Result<(), String> {
        self.net
            .rpc
            .submit(
                &[
                    ComputeBudgetInstruction::set_compute_unit_limit(600_000),
                    instruction,
                ],
                &self.net.payer,
                signers,
            )
            .map_err(|error| error.to_string())
    }

    fn must(&self, what: &str, instruction: Instruction, signers: &[&Keypair]) {
        if let Err(error) = self.send(instruction, signers) {
            panic!("{what} should have succeeded: {error}");
        }
        println!("  ok   {what}");
    }

    fn must_fail(&self, what: &str, instruction: Instruction, signers: &[&Keypair]) {
        match self.send(instruction, signers) {
            Ok(()) => panic!("{what} should have been refused, and was not"),
            Err(_) => println!("  ok   refused: {what}"),
        }
    }

    /// Creates a funded token account the plain way: allocate, then initialise.
    fn token_account(&self, mint: &Pubkey, owner: &Keypair, amount: u64) -> Pubkey {
        let account = Keypair::new();
        let rent = self
            .net
            .rpc
            .rent_exempt_minimum(spl_token::state::Account::LEN)
            .expect("rent");
        let mut instructions = vec![
            system_instruction::create_account(
                &self.net.payer.pubkey(),
                &account.pubkey(),
                rent,
                spl_token::state::Account::LEN as u64,
                &spl_token::ID,
            ),
            spl_token::instruction::initialize_account3(
                &spl_token::ID,
                &account.pubkey(),
                mint,
                &owner.pubkey(),
            )
            .expect("initialise"),
        ];
        if amount > 0 {
            // The genesis mint names the governance key as its authority, so
            // the test can hand out balances the way an exchange would.
            instructions.push(spl_token::instruction::mint_to(
                &spl_token::ID,
                mint,
                &account.pubkey(),
                &self.net.authority.pubkey(),
                &[],
                amount,
            ).expect("mint"));
        }
        // The mint authority only signs when there is something to mint;
        // offering a signature the message does not call for is itself an error.
        let mut signers: Vec<&Keypair> = vec![&account];
        if amount > 0 {
            signers.push(&self.net.authority);
        }
        self.net
            .rpc
            .submit(&instructions, &self.net.payer, &signers)
            .expect("token account created");
        account.pubkey()
    }

    fn balance(&self, account: &Pubkey) -> u64 {
        let data = self.net.rpc.account_data(account).expect("account");
        spl_token::state::Account::unpack(&data).expect("token account").amount
    }

    fn market(&self, address: &Pubkey) -> prediction_market::state::Market {
        let data = self.net.rpc.account_data(address).expect("market");
        prediction_market::state::Market::deserialize(&mut &data[8..]).expect("market decodes")
    }
}

struct Market {
    id: [u8; 32],
    address: Pubkey,
    yes_mint: Pubkey,
    no_mint: Pubkey,
    vault: Pubkey,
}

impl Market {
    fn new(id_byte: u8) -> Market {
        let id = [id_byte; 32];
        let address = market_pda(&id);
        Market {
            id,
            address,
            yes_mint: child_pda(b"yes", &address),
            no_mint: child_pda(b"no", &address),
            vault: child_pda(b"vault", &address),
        }
    }
}

fn median_of_three() -> Vec<u8> {
    let mut buffer = [0u8; 64];
    let mut encoder = market_vm::Encoder::new(&mut buffer);
    encoder
        .push_input(0)
        .and_then(|e| e.push_input(1))
        .and_then(|e| e.push_input(2))
        .and_then(|e| e.median(3))
        .expect("encoding fits");
    encoder.code().to_vec()
}

fn feed_refs(net: &Localnet) -> Vec<FeedRef> {
    net.rings
        .iter()
        .map(|ring| FeedRef { feed: feed_pda(&ring.to_bytes()), invert: false })
        .collect()
}

fn create_market_ix(net: &Localnet, market: &Market, strike: i64, feeds: Vec<FeedRef>) -> Instruction {
    let mut accounts = prediction_market::accounts::CreateMarket {
        config: config_pda(),
        collateral: collateral_pda(&net.collateral_mint),
        mint: net.collateral_mint,
        collateral_mint: net.collateral_mint,
        market: market.address,
        spec: child_pda(b"spec", &market.address),
        vault: market.vault,
        yes_mint: market.yes_mint,
        no_mint: market.no_mint,
        creator: net.authority.pubkey(),
        treasury: net.treasury,
        token_program: spl_token::ID,
        system_program: anchor_lang::solana_program::system_program::ID,
    }
    .to_account_metas(None);
    accounts.extend(feeds.iter().map(|f| AccountMeta::new_readonly(f.feed, false)));

    Instruction {
        program_id: prediction_market::ID,
        accounts,
        data: prediction_market::instruction::CreateMarket {
            args: prediction_market::instructions::CreateMarketArgs {
                market_id: market.id,
                settle_at: net.settle_at,
                strike: Q64::from_int(strike).raw(),
                ramp_bps: RAMP_BPS,
                feeds,
                bytecode: median_of_three(),
                rules_uri: "https://example.test/e2e".to_string(),
            },
        }
        .data(),
    }
}

fn deposit_ix(
    net: &Localnet,
    market: &Market,
    bettor: &Keypair,
    side_is_yes: bool,
    collateral: Pubkey,
    outcome: Pubkey,
    amount: u64,
) -> Instruction {
    Instruction {
        program_id: prediction_market::ID,
        accounts: prediction_market::accounts::Deposit {
            market: market.address,
            collateral: collateral_pda(&net.collateral_mint),
            collateral_mint: net.collateral_mint,
            vault: market.vault,
            side_mint: if side_is_yes { market.yes_mint } else { market.no_mint },
            depositor_collateral: collateral,
            depositor_outcome: outcome,
            depositor: bettor.pubkey(),
            token_program: spl_token::ID,
        }
        .to_account_metas(None),
        data: prediction_market::instruction::Deposit { side_is_yes, amount }.data(),
    }
}

fn snapshot_ix(net: &Localnet, market: &Market, keeper: &Keypair) -> Instruction {
    let mut accounts = prediction_market::accounts::TakeSnapshot {
        config: config_pda(),
        market: market.address,
        spec: child_pda(b"spec", &market.address),
        snapshot: child_pda(b"snapshot", &market.address),
        keeper: keeper.pubkey(),
        system_program: anchor_lang::solana_program::system_program::ID,
    }
    .to_account_metas(None);
    for ring in &net.rings {
        accounts.push(AccountMeta::new_readonly(feed_pda(&ring.to_bytes()), false));
        accounts.push(AccountMeta::new_readonly(*ring, false));
    }
    Instruction {
        program_id: prediction_market::ID,
        accounts,
        data: prediction_market::instruction::Snapshot {}.data(),
    }
}

fn resolve_ix(market: &Market, resolver: &Keypair) -> Instruction {
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
    }
}

fn claim_ix(
    market: &Market,
    holder: &Keypair,
    side_is_yes: bool,
    outcome: Pubkey,
    collateral: Pubkey,
) -> Instruction {
    Instruction {
        program_id: prediction_market::ID,
        accounts: prediction_market::accounts::Claim {
            market: market.address,
            vault: market.vault,
            side_mint: if side_is_yes { market.yes_mint } else { market.no_mint },
            holder_outcome: outcome,
            holder_collateral: collateral,
            holder: holder.pubkey(),
            token_program: spl_token::ID,
        }
        .to_account_metas(None),
        data: prediction_market::instruction::Claim { side_is_yes }.data(),
    }
}

#[test]
#[ignore = "boots a validator and waits out real minutes"]
fn the_protocol_runs_end_to_end_on_a_live_validator() {
    let net = Localnet::start();
    let client = Client { net: &net };
    println!(
        "validator up; chain time {}, settling at {}",
        net.chain_time(),
        net.settle_at
    );

    let alice = Keypair::new();
    let bob = Keypair::new();
    let keeper = Keypair::new();
    for account in [&alice, &bob, &keeper] {
        net.rpc
            .airdrop(&account.pubkey(), 5 * LAMPORTS_PER_SOL)
            .expect("airdrop");
    }

    println!("\n-- governance --");
    // A fresh Pyth instrument: nothing to probe, since a price account does not
    // exist until someone posts one.
    client.must(
        "registering a Pyth instrument",
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::RegisterPythFeed {
                config: config_pda(),
                authority: net.authority.pubkey(),
                feed: feed_pda(&[0x77u8; 32]),
                payer: net.authority.pubkey(),
                system_program: anchor_lang::solana_program::system_program::ID,
            }
            .to_account_metas(None),
            data: prediction_market::instruction::RegisterPythFeed {
                args: prediction_market::instructions::RegisterPythFeedArgs {
                    feed_id: [0x77u8; 32],
                    depth_quote: 1_000_000_000_000,
                    label: [0u8; 32],
                },
            }
            .data(),
        },
        &[&net.authority],
    );

    // A pool feed, which *is* probed: registration reads the ring for real.
    let probe_to = net.chain_time() - 30;
    let register_pool_feed = |from: i64, to: i64| Instruction {
        program_id: prediction_market::ID,
        accounts: prediction_market::accounts::RegisterFeed {
            config: config_pda(),
            authority: net.authority.pubkey(),
            pool: net.spare_pool,
            observation_state: net.spare_ring,
            feed: feed_pda(&net.spare_ring.to_bytes()),
            payer: net.authority.pubkey(),
            system_program: anchor_lang::solana_program::system_program::ID,
        }
        .to_account_metas(None),
        data: prediction_market::instruction::RegisterFeed {
            args: prediction_market::instructions::RegisterFeedArgs {
                depth_quote: 1_000_000_000_000,
                label: [0u8; 32],
                probe_from: from,
                probe_to: to,
                probe_max_segment: MAX_SEGMENT,
                probe_min_observations: MIN_OBSERVATIONS,
            },
        }
        .data(),
    };
    client.must_fail(
        "registering a feed whose ring cannot serve the probe",
        register_pool_feed(probe_to - 100_000, probe_to),
        &[&net.authority],
    );
    client.must(
        "registering a pool feed, probe and all",
        register_pool_feed(probe_to - 120, probe_to),
        &[&net.authority],
    );

    // Newly registered feeds wait out the timelock before any market may name
    // them, so this one cannot be used by anything created today.
    let fresh = {
        let data = net.rpc.account_data(&feed_pda(&net.spare_ring.to_bytes())).expect("feed");
        prediction_market::state::Feed::deserialize(&mut &data[8..]).expect("feed decodes")
    };
    assert!(
        fresh.effective_at > net.chain_time(),
        "a feed registered now must not be usable now"
    );
    assert!(!fresh.is_active(net.chain_time()));

    let market = Market::new(1);

    println!("\n-- creation --");
    // Governance is not open to anyone.
    client.must_fail(
        "a stranger changing protocol parameters",
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::Governance {
                config: config_pda(),
                authority: alice.pubkey(),
            }
            .to_account_metas(None),
            data: prediction_market::instruction::SetPaused { paused: true }.data(),
        },
        &[&alice],
    );

    // A market naming the same source three times is not a median of three.
    let duplicated = vec![feed_refs(&net)[0]; 3];
    client.must_fail(
        "a market whose three sources are one source",
        create_market_ix(&net, &Market::new(9), STRIKE, duplicated),
        &[&net.authority],
    );

    // A band narrower than governance allows.
    let mut narrow = create_market_ix(&net, &Market::new(8), STRIKE, feed_refs(&net));
    narrow.data = prediction_market::instruction::CreateMarket {
        args: prediction_market::instructions::CreateMarketArgs {
            market_id: [8u8; 32],
            settle_at: net.settle_at,
            strike: Q64::from_int(STRIKE).raw(),
            ramp_bps: 1,
            feeds: feed_refs(&net),
            bytecode: median_of_three(),
            rules_uri: String::new(),
        },
    }
    .data();
    client.must_fail("a market with a settlement band of one basis point", narrow, &[&net.authority]);

    client.must_fail(
        "a market naming a feed still inside its timelock",
        create_market_ix(
            &net,
            &Market::new(7),
            STRIKE,
            vec![
                feed_refs(&net)[0],
                feed_refs(&net)[1],
                FeedRef { feed: feed_pda(&net.spare_ring.to_bytes()), invert: false },
            ],
        ),
        &[&net.authority],
    );

    client.must(
        "creating the market",
        create_market_ix(&net, &market, STRIKE, feed_refs(&net)),
        &[&net.authority],
    );

    let state = client.market(&market.address);
    assert_eq!(state.status, MarketStatus::Created);
    assert_eq!(state.staked_yes, 0);
    assert!(state.cap_per_side > 0);

    println!("\n-- betting --");
    let alice_collateral = client.token_account(&net.collateral_mint, &alice, STAKE_YES);
    let alice_outcome = client.token_account(&market.yes_mint, &alice, 0);
    let bob_collateral = client.token_account(&net.collateral_mint, &bob, STAKE_NO);
    let bob_outcome = client.token_account(&market.no_mint, &bob, 0);

    // Staking the wrong mint for the side named.
    client.must_fail(
        "betting YES while presenting the NO mint",
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::Deposit {
                market: market.address,
                collateral: collateral_pda(&net.collateral_mint),
                collateral_mint: net.collateral_mint,
                vault: market.vault,
                side_mint: market.no_mint,
                depositor_collateral: alice_collateral,
                depositor_outcome: alice_outcome,
                depositor: alice.pubkey(),
                token_program: spl_token::ID,
            }
            .to_account_metas(None),
            data: prediction_market::instruction::Deposit { side_is_yes: true, amount: 1_000 }.data(),
        },
        &[&alice],
    );

    client.must(
        "staking on YES",
        deposit_ix(&net, &market, &alice, true, alice_collateral, alice_outcome, STAKE_YES),
        &[&alice],
    );
    client.must(
        "staking on NO",
        deposit_ix(&net, &market, &bob, false, bob_collateral, bob_outcome, STAKE_NO),
        &[&bob],
    );

    let state = client.market(&market.address);
    assert_eq!(state.status, MarketStatus::Open);
    assert_eq!(state.staked_yes, STAKE_YES);
    assert_eq!(state.staked_no, STAKE_NO);
    assert_eq!(client.balance(&market.vault), STAKE_YES + STAKE_NO);
    assert_eq!(client.balance(&alice_outcome), STAKE_YES);

    // Settling before there is anything to settle.
    client.must_fail("snapshotting before settlement", snapshot_ix(&net, &market, &keeper), &[&keeper]);
    client.must_fail("resolving before a snapshot exists", resolve_ix(&market, &keeper), &[&keeper]);
    client.must_fail(
        "claiming before the market has settled",
        claim_ix(&market, &alice, true, alice_outcome, alice_collateral),
        &[&alice],
    );

    println!("\n-- betting closes --");
    let lock_at = net.settle_at - i64::from(TWAP_WINDOW) - i64::from(SKEW);
    net.wait_until(lock_at + 2);
    client.must_fail(
        "staking after the deadline",
        deposit_ix(&net, &market, &alice, true, alice_collateral, alice_outcome, 1_000),
        &[&alice],
    );
    client.must_fail(
        "voiding a market that is merely locked",
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::VoidMarket {
                market: market.address,
                caller: keeper.pubkey(),
            }
            .to_account_metas(None),
            data: prediction_market::instruction::Void {}.data(),
        },
        &[&keeper],
    );

    println!("\n-- settlement --");
    net.wait_until(net.settle_at + 2);
    client.must("taking the snapshot", snapshot_ix(&net, &market, &keeper), &[&keeper]);
    client.must_fail(
        "taking a second snapshot",
        snapshot_ix(&net, &market, &keeper),
        &[&keeper],
    );

    let state = client.market(&market.address);
    assert_eq!(state.status, MarketStatus::Snapshotted);
    assert_eq!(state.snapshot_keeper, keeper.pubkey());

    client.must("resolving", resolve_ix(&market, &keeper), &[&keeper]);
    client.must_fail("resolving twice", resolve_ix(&market, &keeper), &[&keeper]);

    let state = client.market(&market.address);
    assert_eq!(state.status, MarketStatus::Resolved);
    assert_eq!(state.status_reason, VoidCause::None);
    assert_eq!(state.share, Q64::ONE.raw(), "the strike sits far below the price");
    // Two percent of the four million that changed hands.
    assert_eq!(state.fee_total, 80_000);
    assert_eq!(state.pool_yes, 4_920_000);
    assert_eq!(state.pool_no, 0);
    assert_eq!(
        state.pool_yes + state.pool_no + state.fee_total,
        STAKE_YES + STAKE_NO,
        "the pot is conserved exactly"
    );

    println!("\n-- payouts --");
    client.must(
        "the winner claiming",
        claim_ix(&market, &alice, true, alice_outcome, alice_collateral),
        &[&alice],
    );
    assert_eq!(client.balance(&alice_collateral), 4_920_000);
    assert!(
        !net.rpc.account_exists(&alice_outcome),
        "claiming closes the outcome account and returns its rent"
    );

    client.must(
        "the loser closing a worthless position",
        claim_ix(&market, &bob, false, bob_outcome, bob_collateral),
        &[&bob],
    );
    assert_eq!(client.balance(&bob_collateral), 0);

    println!("\n-- fees --");
    let treasury_token = client.token_account(&net.collateral_mint, &net.authority, 0);
    let thief = Keypair::new();
    let thief_token = client.token_account(&net.collateral_mint, &thief, 0);
    let collect = |recipient: FeeRecipient, destination: Pubkey| Instruction {
        program_id: prediction_market::ID,
        accounts: prediction_market::accounts::CollectFee {
            config: config_pda(),
            market: market.address,
            vault: market.vault,
            destination,
            caller: keeper.pubkey(),
            token_program: spl_token::ID,
        }
        .to_account_metas(None),
        data: prediction_market::instruction::CollectFee { recipient }.data(),
    };
    client.must_fail(
        "a fee redirected to somebody it is not owed to",
        collect(FeeRecipient::Creator, thief_token),
        &[&keeper],
    );
    client.must(
        "the creator collecting their share",
        collect(FeeRecipient::Creator, treasury_token),
        &[&keeper],
    );
    client.must_fail(
        "collecting the same share twice",
        collect(FeeRecipient::Creator, treasury_token),
        &[&keeper],
    );
    assert_eq!(client.balance(&treasury_token), state.fee_owed_creator);

    println!("\n-- cleanup --");
    client.must_fail(
        "sweeping dust while holders may still claim",
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::SweepDust {
                config: config_pda(),
                market: market.address,
                vault: market.vault,
                treasury_token,
                caller: keeper.pubkey(),
                token_program: spl_token::ID,
            }
            .to_account_metas(None),
            data: prediction_market::instruction::SweepDust {}.data(),
        },
        &[&keeper],
    );
    client.must_fail(
        "closing a market whose vault still holds collateral",
        Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::CloseMarket {
                market: market.address,
                spec: child_pda(b"spec", &market.address),
                vault: market.vault,
                creator: net.authority.pubkey(),
                caller: keeper.pubkey(),
                token_program: spl_token::ID,
            }
            .to_account_metas(None),
            data: prediction_market::instruction::CloseMarket {}.data(),
        },
        &[&keeper],
    );

    println!("\nall phases completed on a live validator");
}
