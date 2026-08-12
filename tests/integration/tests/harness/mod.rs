//! A whole world in memory: the program, a collateral mint, a stand-in for
//! Raydium, and three observation rings.
//!
//! The Raydium program address is a `Config` field rather than a constant, and
//! that pays off here: the tests own an address, place synthetic rings under it,
//! and exercise the real parser against the real wire format without a fork or a
//! network. Nothing about the price path is mocked -- the bytes are laid out
//! exactly as Raydium's `update()` would leave them.
//!
//! Each test binary compiles this module separately, so whatever only one of
//! them uses looks dead to the rest.
#![allow(dead_code)]

use anchor_lang::{InstructionData, ToAccountMetas};
use litesvm::LiteSVM;
use solana_compute_budget_interface::ComputeBudgetInstruction;
use solana_sdk::{
    account::Account,
    clock::Clock,
    instruction::Instruction,
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

use prediction_market::state::{FeedRef, MarketParams};

/// Where `anchor build` leaves the program.
const PROGRAM_PATH: &str = "../../target/deploy/prediction_market.so";

pub const TIMELOCK: u32 = 3_600;
pub const TWAP_WINDOW: u32 = 900;
pub const GRACE: u32 = 300;
pub const SKEW: u32 = 60;
pub const MAX_SEGMENT: u32 = 450;
pub const MIN_OBSERVATIONS: u16 = 5;
pub const CREATION_COOLDOWN: u32 = 300;
pub const FEE_BPS: u16 = 200;
pub const RAMP_BPS: u16 = 50;
pub const KEEPER_REWARD: u64 = 10_000_000;
pub const CREATION_FEE: u64 = 5_000_000;

/// USDC-like: six decimals.
pub const COLLATERAL_DECIMALS: u8 = 6;
/// The token being priced: nine decimals, like most SPL launches.
pub const TOKEN_DECIMALS: u8 = 9;

/// Tick of a pool quoting a 9-decimal token in a 6-decimal one at ~100.
///
/// The raw ratio is 0.1, so the tick is negative -- the ordinary case, and the
/// one where truncating instead of flooring costs a basis point.
pub const TICK_AT_100: i32 = -23_028;

pub struct World {
    pub svm: LiteSVM,
    pub payer: Keypair,
    pub authority: Keypair,
    pub treasury: Pubkey,
    pub raydium: Pubkey,
    pub pyth: Pubkey,
    pub collateral_mint: Pubkey,
    pub token_mint: Pubkey,
    pub rings: Vec<Pubkey>,
    pub pools: Vec<Pubkey>,
    /// A registered Pyth instrument, and the account a crank would post for it.
    pub pyth_feed_id: [u8; 32],
    pub pyth_account: Pubkey,
}

pub fn config_pda() -> Pubkey {
    Pubkey::find_program_address(&[b"config"], &prediction_market::ID).0
}

pub fn collateral_pda(mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"collateral", mint.as_ref()], &prediction_market::ID).0
}

pub fn feed_pda(ring: &Pubkey) -> Pubkey {
    feed_pda_from_id(&ring.to_bytes())
}

/// Feeds are seeded by their source identifier, whatever kind of source it is:
/// a ring's address for Raydium, an instrument id for Pyth.
pub fn feed_pda_from_id(source_id: &[u8; 32]) -> Pubkey {
    Pubkey::find_program_address(&[b"feed", source_id], &prediction_market::ID).0
}

pub fn market_pda(market_id: &[u8; 32]) -> Pubkey {
    Pubkey::find_program_address(&[b"market", market_id], &prediction_market::ID).0
}

pub fn child_pda(prefix: &[u8], market: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[prefix, market.as_ref()], &prediction_market::ID).0
}

impl World {
    /// Boots the ledger with the program, a mint, and three price sources whose
    /// registration timelock has already run out.
    pub fn new(now: i64) -> World {
        let mut svm = LiteSVM::new();
        svm.add_program_from_file(prediction_market::ID, PROGRAM_PATH)
            .expect("run `anchor build` first");

        let payer = Keypair::new();
        let authority = Keypair::new();
        svm.airdrop(&payer.pubkey(), 1_000 * 1_000_000_000).unwrap();
        svm.airdrop(&authority.pubkey(), 1_000 * 1_000_000_000)
            .unwrap();

        let mut world = World {
            svm,
            payer,
            authority,
            treasury: Pubkey::new_unique(),
            raydium: Pubkey::new_unique(),
            pyth: Pubkey::new_unique(),
            collateral_mint: Pubkey::new_unique(),
            token_mint: Pubkey::new_unique(),
            rings: Vec::new(),
            pools: Vec::new(),
            pyth_feed_id: [0x5a; 32],
            pyth_account: Pubkey::new_unique(),
        };

        // Registration happens far enough back that the timelock has expired by
        // the time markets are created.
        let registered_at = now - 10_000;
        world.warp_to(registered_at);

        world.place_mint(world.collateral_mint, COLLATERAL_DECIMALS);
        world.place_mint(world.token_mint, TOKEN_DECIMALS);
        world.initialize_config();
        world.register_collateral();

        for _ in 0..3 {
            let pool = Pubkey::new_unique();
            let ring = Pubkey::new_unique();
            world.place_pool(pool, ring);
            world.place_ring(ring, pool, TICK_AT_100, registered_at + 30);
            world.register_feed(pool, ring, registered_at);
            world.pools.push(pool);
            world.rings.push(ring);
        }

        world.register_pyth_feed(registered_at);
        world.warp_to(now);
        world
    }

    /// Posts the TWAP account a keeper would produce for the Pyth feed.
    ///
    /// Its address is not fixed anywhere -- a Pyth account does not exist until
    /// someone creates it -- so the program has only its owner and its contents
    /// to go on. That is exactly what these tests exercise.
    pub fn place_pyth_twap(&mut self, settle_at: i64, price: i64, conf: u64, down_ratio: u32) {
        const LEN: usize = 112;
        const DISCRIMINATOR: [u8; 8] = [0x68, 0xc0, 0xbc, 0x48, 0xf6, 0xa6, 0x0c, 0x51];

        let mut data = vec![0u8; LEN];
        data[..8].copy_from_slice(&DISCRIMINATOR);
        data[8..40].copy_from_slice(&[0x99u8; 32]); // write_authority
        data[40..72].copy_from_slice(&self.pyth_feed_id);
        data[72..80].copy_from_slice(&(settle_at - i64::from(TWAP_WINDOW)).to_le_bytes());
        data[80..88].copy_from_slice(&settle_at.to_le_bytes());
        data[88..96].copy_from_slice(&price.to_le_bytes());
        data[96..104].copy_from_slice(&conf.to_le_bytes());
        data[104..108].copy_from_slice(&(-8i32).to_le_bytes());
        data[108..112].copy_from_slice(&down_ratio.to_le_bytes());

        let account = self.pyth_account;
        let owner = self.pyth;
        self.svm
            .set_account(
                account,
                Account {
                    lamports: 10_000_000,
                    data,
                    owner,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
    }

    fn register_pyth_feed(&mut self, _now: i64) {
        let feed = feed_pda_from_id(&self.pyth_feed_id);
        let instruction = Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::RegisterPythFeed {
                config: config_pda(),
                authority: self.authority.pubkey(),
                feed,
                payer: self.authority.pubkey(),
                system_program: anchor_lang::solana_program::system_program::ID,
            }
            .to_account_metas(None),
            data: prediction_market::instruction::RegisterPythFeed {
                args: prediction_market::instructions::RegisterPythFeedArgs {
                    feed_id: self.pyth_feed_id,
                    depth_quote: 1_000_000_000_000,
                    label: [0u8; 32],
                },
            }
            .data(),
        };
        let authority = self.authority.insecure_clone();
        self.must_send(instruction, &[&authority]);
    }

    /// Two pools and one oracle in a single median, which is what the feed
    /// registry is for: the kind of each source is governance's decision, and
    /// the predicate does not know or care which produced a number.
    pub fn mixed_feed_refs(&self) -> Vec<FeedRef> {
        vec![
            FeedRef {
                feed: feed_pda(&self.rings[0]),
                invert: false,
            },
            FeedRef {
                feed: feed_pda(&self.rings[1]),
                invert: false,
            },
            FeedRef {
                feed: feed_pda_from_id(&self.pyth_feed_id),
                invert: false,
            },
        ]
    }

    pub fn mixed_account_metas(&self) -> Vec<solana_sdk::instruction::AccountMeta> {
        use solana_sdk::instruction::AccountMeta;
        vec![
            AccountMeta::new_readonly(feed_pda(&self.rings[0]), false),
            AccountMeta::new_readonly(self.rings[0], false),
            AccountMeta::new_readonly(feed_pda(&self.rings[1]), false),
            AccountMeta::new_readonly(self.rings[1], false),
            AccountMeta::new_readonly(feed_pda_from_id(&self.pyth_feed_id), false),
            AccountMeta::new_readonly(self.pyth_account, false),
        ]
    }

    pub fn warp_to(&mut self, unix_timestamp: i64) {
        let mut clock: Clock = self.svm.get_sysvar();
        clock.unix_timestamp = unix_timestamp;
        clock.slot += 1;
        self.svm.set_sysvar(&clock);
    }

    pub fn now(&self) -> i64 {
        self.svm.get_sysvar::<Clock>().unix_timestamp
    }

    pub fn send(&mut self, instruction: Instruction, signers: &[&Keypair]) -> Result<(), String> {
        // Two identical instructions in one test would otherwise share a
        // signature and be rejected as a replay -- which happens whenever a
        // test asserts a call fails and then makes the same call succeed.
        self.svm.expire_blockhash();
        let mut all = vec![&self.payer];
        all.extend_from_slice(signers);
        // Reading several observation rings costs far more than the 200k a
        // transaction gets by default, so every client has to ask for more.
        let budget = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
        let transaction = Transaction::new_signed_with_payer(
            &[budget, instruction],
            Some(&self.payer.pubkey()),
            &all,
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(transaction)
            .map(|_| ())
            .map_err(|failure| format!("{:?}\n{}", failure.err, failure.meta.logs.join("\n")))
    }

    /// Compute units the last successful transaction consumed, for measuring.
    pub fn measure(&mut self, instruction: Instruction, signers: &[&Keypair]) -> u64 {
        self.svm.expire_blockhash();
        let mut all = vec![&self.payer];
        all.extend_from_slice(signers);
        let budget = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
        let transaction = Transaction::new_signed_with_payer(
            &[budget, instruction],
            Some(&self.payer.pubkey()),
            &all,
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(transaction)
            .map(|result| result.compute_units_consumed)
            .expect("transaction should succeed")
    }

    pub fn must_send(&mut self, instruction: Instruction, signers: &[&Keypair]) {
        if let Err(failure) = self.send(instruction, signers) {
            panic!("transaction failed: {failure}");
        }
    }

    // -- Account fabrication -------------------------------------------

    fn place_mint(&mut self, mint: Pubkey, decimals: u8) {
        let mut data = vec![0u8; spl_token::state::Mint::LEN];
        spl_token::state::Mint {
            mint_authority: None.into(),
            supply: 0,
            decimals,
            is_initialized: true,
            freeze_authority: None.into(),
        }
        .pack_into_slice(&mut data);
        self.svm
            .set_account(
                mint,
                Account {
                    lamports: 1_000_000_000,
                    data,
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
    }

    /// Creates a funded token account without going through the ATA program.
    ///
    /// The program checks a token account's mint and owner, never that it is a
    /// canonical associated account, so this is the same thing from its side
    /// and it keeps the tests free of setup noise.
    pub fn place_token_account(&mut self, mint: Pubkey, owner: Pubkey, amount: u64) -> Pubkey {
        let address = Pubkey::new_unique();
        let mut data = vec![0u8; spl_token::state::Account::LEN];
        spl_token::state::Account {
            mint,
            owner,
            amount,
            delegate: None.into(),
            state: spl_token::state::AccountState::Initialized,
            is_native: None.into(),
            delegated_amount: 0,
            close_authority: None.into(),
        }
        .pack_into_slice(&mut data);
        self.svm
            .set_account(
                address,
                Account {
                    lamports: 10_000_000,
                    data,
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        address
    }

    /// Adds tokens to an existing account without going through a transfer.
    ///
    /// Stands in for anyone sending into an account they do not control --
    /// which nothing prevents, and which the program therefore must survive.
    pub fn credit_token_account(&mut self, address: Pubkey, amount: u64) {
        let mut account = self
            .svm
            .get_account(&address)
            .expect("token account exists");
        let mut state =
            spl_token::state::Account::unpack(&account.data).expect("valid token account");
        state.amount += amount;
        state.pack_into_slice(&mut account.data);
        self.svm.set_account(address, account).unwrap();
    }

    pub fn token_balance(&self, address: &Pubkey) -> u64 {
        let account = self.svm.get_account(address).expect("token account exists");
        spl_token::state::Account::unpack(&account.data)
            .expect("valid token account")
            .amount
    }

    /// Places the pool account, laid out as Raydium's `PoolState`.
    ///
    /// Real bytes, not padding: registration reads the pair's mints, their
    /// decimals and the ring's address out of this account rather than taking
    /// them as arguments, so the fixture has to say what a pool says.
    pub fn place_pool(&mut self, pool: Pubkey, ring: Pubkey) {
        let data = market_feeds::write_pool_state(
            &self.token_mint.to_bytes(),
            &self.collateral_mint.to_bytes(),
            &ring.to_bytes(),
            TOKEN_DECIMALS,
            COLLATERAL_DECIMALS,
        );
        self.svm
            .set_account(
                pool,
                Account {
                    lamports: 50_000_000,
                    data: data.to_vec(),
                    owner: self.raydium,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
    }

    /// Writes a full observation ring holding a constant tick.
    ///
    /// Laid out exactly as Raydium's `update()` leaves it: a hundred entries
    /// fifteen seconds apart, the cumulative advancing by `tick * delta_time`,
    /// and the newest entry at `observation_index`.
    pub fn place_ring(&mut self, ring: Pubkey, pool: Pubkey, tick: i32, newest_at: i64) {
        self.place_ring_with_spacing(ring, pool, newest_at, 15, tick);
    }

    /// The same, but with a chosen gap between observations.
    ///
    /// Wide spacing models a pool nobody trades: the ring is full and fresh,
    /// yet a single segment can swallow the whole averaging window.
    pub fn place_ring_with_spacing(
        &mut self,
        ring: Pubkey,
        pool: Pubkey,
        newest_at: i64,
        step: u32,
        tick: i32,
    ) {
        const OBSERVATIONS: usize = 100;
        const LEN: usize = 4_483;
        const DISCRIMINATOR: [u8; 8] = [0x7a, 0xae, 0xc5, 0x35, 0x81, 0x09, 0xa5, 0x84];

        let oldest_at = newest_at - ((OBSERVATIONS as i64 - 1) * i64::from(step));
        let mut timestamps = Vec::with_capacity(OBSERVATIONS);
        let mut cumulatives = Vec::with_capacity(OBSERVATIONS);
        timestamps.push(oldest_at as u32);
        cumulatives.push(0i64);
        for segment in 0..OBSERVATIONS - 1 {
            let _ = segment;
            let previous_timestamp = timestamps[segment];
            let previous_cumulative = cumulatives[segment];
            timestamps.push(previous_timestamp + step);
            cumulatives.push(previous_cumulative.wrapping_add(i64::from(tick) * i64::from(step)));
        }

        let mut data = vec![0u8; LEN];
        data[..8].copy_from_slice(&DISCRIMINATOR);
        data[8] = 1; // initialized
        data[17..19].copy_from_slice(&((OBSERVATIONS - 1) as u16).to_le_bytes());
        data[19..51].copy_from_slice(pool.as_ref());
        for position in 0..OBSERVATIONS {
            let base = 51 + position * 44;
            data[base..base + 4].copy_from_slice(&timestamps[position].to_le_bytes());
            data[base + 4..base + 12].copy_from_slice(&cumulatives[position].to_le_bytes());
        }

        self.svm
            .set_account(
                ring,
                Account {
                    lamports: 50_000_000,
                    data,
                    owner: self.raydium,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
    }

    /// Re-writes every ring so it brackets `settle_at`, as it would by the time
    /// a market is due to settle.
    pub fn refresh_rings(&mut self, settle_at: i64, tick: i32) {
        for index in 0..self.rings.len() {
            let (ring, pool) = (self.rings[index], self.pools[index]);
            self.place_ring(ring, pool, tick, settle_at + 30);
        }
    }

    // -- Program calls -------------------------------------------------

    pub fn params(&self) -> MarketParams {
        MarketParams {
            fee_bps: FEE_BPS,
            feed_cap_bps: 500,
            min_ramp_bps: RAMP_BPS,
            twap_window: TWAP_WINDOW,
            grace: GRACE,
            skew: SKEW,
            max_segment: MAX_SEGMENT,
            min_observations: MIN_OBSERVATIONS,
            creation_cooldown: CREATION_COOLDOWN,
            claim_window: 90 * 86_400,
            pyth_window_tolerance: 5,
            max_confidence_bps: 100,
            max_down_slots_ratio: 50_000,
            keeper_reward: KEEPER_REWARD,
            creation_fee: CREATION_FEE,
        }
    }

    fn initialize_config(&mut self) {
        let instruction = Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::InitializeConfig {
                config: config_pda(),
                payer: self.authority.pubkey(),
                system_program: anchor_lang::solana_program::system_program::ID,
            }
            .to_account_metas(None),
            data: prediction_market::instruction::InitializeConfig {
                args: prediction_market::instructions::InitializeConfigArgs {
                    treasury: self.treasury,
                    raydium_clmm_program: self.raydium,
                    pyth_receiver_program: self.pyth,
                    timelock: TIMELOCK,
                    params: self.params(),
                },
            }
            .data(),
        };
        let authority = self.authority.insecure_clone();
        self.must_send(instruction, &[&authority]);
    }

    fn register_collateral(&mut self) {
        let instruction = Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::RegisterCollateral {
                config: config_pda(),
                authority: self.authority.pubkey(),
                mint: self.collateral_mint,
                collateral: collateral_pda(&self.collateral_mint),
                payer: self.authority.pubkey(),
                system_program: anchor_lang::solana_program::system_program::ID,
            }
            .to_account_metas(None),
            data: prediction_market::instruction::RegisterCollateral { min_stake: 1_000 }.data(),
        };
        let authority = self.authority.insecure_clone();
        self.must_send(instruction, &[&authority]);
    }

    fn register_feed(&mut self, pool: Pubkey, ring: Pubkey, now: i64) {
        let instruction = Instruction {
            program_id: prediction_market::ID,
            accounts: prediction_market::accounts::RegisterFeed {
                config: config_pda(),
                authority: self.authority.pubkey(),
                pool,
                observation_state: ring,
                feed: feed_pda(&ring),
                payer: self.authority.pubkey(),
                system_program: anchor_lang::solana_program::system_program::ID,
            }
            .to_account_metas(None),
            data: prediction_market::instruction::RegisterFeed {
                args: prediction_market::instructions::RegisterFeedArgs {
                    // Generous enough that the per-side cap never binds in
                    // tests that are not about the cap.
                    depth_quote: 1_000_000_000_000,
                    label: [0u8; 32],
                    probe_from: now - 300,
                    probe_to: now,
                    probe_max_segment: MAX_SEGMENT,
                    probe_min_observations: MIN_OBSERVATIONS,
                },
            }
            .data(),
        };
        let authority = self.authority.insecure_clone();
        self.must_send(instruction, &[&authority]);
    }

    pub fn feed_refs(&self) -> Vec<FeedRef> {
        self.rings
            .iter()
            .map(|ring| FeedRef {
                feed: feed_pda(ring),
                invert: false,
            })
            .collect()
    }

    /// `Feed` account followed by its ring, per declared feed, in order.
    pub fn feed_account_metas(&self) -> Vec<solana_sdk::instruction::AccountMeta> {
        self.rings
            .iter()
            .flat_map(|ring| {
                [
                    solana_sdk::instruction::AccountMeta::new_readonly(feed_pda(ring), false),
                    solana_sdk::instruction::AccountMeta::new_readonly(*ring, false),
                ]
            })
            .collect()
    }
}

/// "The median of three declared prices", the canonical v1 predicate.
pub fn median_of_three() -> Vec<u8> {
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
