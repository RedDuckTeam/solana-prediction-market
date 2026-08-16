//! Spawns a real validator with the world already in place.
//!
//! LiteSVM pins down most behaviour through the same VM, but not what the
//! runtime does *around* an instruction: the loader, transaction size and
//! signature limits, account locks, rent, fees, and a clock at one second per
//! second. So this brings up `solana-test-validator` and drives it over RPC.
//!
//! Governance state is written into genesis rather than transacted: a feed
//! becomes usable one timelock after registration, and a test cannot wait an
//! hour. The registration instructions are exercised separately.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anchor_lang::{AnchorSerialize, Discriminator};
use solana_sdk::{
    native_token::LAMPORTS_PER_SOL,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};

use super::rpc::Rpc;

use prediction_market::state::{Collateral, Config, Feed, FeedKind, MarketParams};

/// Timings the live run uses. Every one is the smallest the protocol permits,
/// because each second here is a second of wall clock.
pub const TWAP_WINDOW: u32 = 120;
pub const GRACE: u32 = 300;
pub const SKEW: u32 = 60;
pub const MAX_SEGMENT: u32 = 60;
pub const MIN_OBSERVATIONS: u16 = 5;
pub const CREATION_COOLDOWN: u32 = 0;
pub const FEE_BPS: u16 = 200;
pub const RAMP_BPS: u16 = 50;
pub const KEEPER_REWARD: u64 = 10_000_000;
pub const CREATION_FEE: u64 = 5_000_000;
pub const TIMELOCK: u32 = 3_600;

pub const COLLATERAL_DECIMALS: u8 = 6;
pub const TOKEN_DECIMALS: u8 = 9;
/// A pool quoting a 9-decimal token in a 6-decimal one at about 100.
pub const TICK_AT_100: i32 = -23_028;

/// Seconds between the validator starting and the market settling.
///
/// `lock_at` is `settle_at - TWAP_WINDOW - SKEW`, so this leaves a minute of
/// real time to create the market and place both bets before betting closes.
pub const SECONDS_TO_SETTLEMENT: i64 = 240;

pub struct Localnet {
    validator: Child,
    pub rpc: Rpc,
    pub payer: Keypair,
    pub authority: Keypair,
    pub treasury: Pubkey,
    pub raydium: Pubkey,
    pub pyth: Pubkey,
    pub collateral_mint: Pubkey,
    pub rings: Vec<Pubkey>,
    pub pools: Vec<Pubkey>,
    pub pyth_feed_id: [u8; 32],
    pub pyth_account: Pubkey,
    /// A pool and ring placed at genesis but left out of the registry, so the
    /// registration instructions can be exercised against something real.
    pub spare_pool: Pubkey,
    pub spare_ring: Pubkey,
    /// The instant every market in this run settles at.
    pub settle_at: i64,
    workdir: PathBuf,
}

impl Drop for Localnet {
    fn drop(&mut self) {
        let _ = self.validator.kill();
        let _ = self.validator.wait();
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is after the epoch")
        .as_secs() as i64
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Serialises an account the way `solana-test-validator --account-dir` wants it.
fn write_account(dir: &PathBuf, address: &Pubkey, owner: &Pubkey, data: &[u8]) {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(data);
    let json = format!(
        r#"{{"pubkey":"{address}","account":{{"lamports":10000000000,"data":["{encoded}","base64"],"owner":"{owner}","executable":false,"rentEpoch":0,"space":{}}}}}"#,
        data.len()
    );
    fs::write(dir.join(format!("{address}.json")), json).expect("account fixture written");
}

/// A `PoolState` naming its pair and its ring, as `register_feed` reads one.
///
/// The token side of the pair is synthesised from the pool's own address so
/// every pool prices a distinct pair; nothing on the settlement path reads it.
fn pool_bytes(pool: &Pubkey, ring: &Pubkey, collateral_mint: &Pubkey) -> Vec<u8> {
    market_feeds::write_pool_state(
        &pool.to_bytes(),
        &collateral_mint.to_bytes(),
        &ring.to_bytes(),
        TOKEN_DECIMALS,
        COLLATERAL_DECIMALS,
    )
    .to_vec()
}

/// An observation ring holding a constant tick, ending just after settlement.
fn ring_bytes(pool: &Pubkey, tick: i32, newest_at: i64) -> Vec<u8> {
    const OBSERVATIONS: usize = 100;
    const STEP: u32 = 15;
    const LEN: usize = 4_483;
    const DISCRIMINATOR: [u8; 8] = [0x7a, 0xae, 0xc5, 0x35, 0x81, 0x09, 0xa5, 0x84];

    let oldest = newest_at - ((OBSERVATIONS as i64 - 1) * i64::from(STEP));
    let mut data = vec![0u8; LEN];
    data[..8].copy_from_slice(&DISCRIMINATOR);
    data[8] = 1;
    data[17..19].copy_from_slice(&((OBSERVATIONS - 1) as u16).to_le_bytes());
    data[19..51].copy_from_slice(pool.as_ref());

    let mut timestamp = oldest as u32;
    let mut cumulative = 0i64;
    for position in 0..OBSERVATIONS {
        let base = 51 + position * 44;
        data[base..base + 4].copy_from_slice(&timestamp.to_le_bytes());
        data[base + 4..base + 12].copy_from_slice(&cumulative.to_le_bytes());
        cumulative = cumulative.wrapping_add(i64::from(tick) * i64::from(STEP));
        timestamp += STEP;
    }
    data
}

/// A Pyth TWAP account for the window this run settles over.
pub fn pyth_bytes(feed_id: &[u8; 32], settle_at: i64, price: i64, conf: u64, down: u32) -> Vec<u8> {
    const LEN: usize = 112;
    const DISCRIMINATOR: [u8; 8] = [0x68, 0xc0, 0xbc, 0x48, 0xf6, 0xa6, 0x0c, 0x51];
    let mut data = vec![0u8; LEN];
    data[..8].copy_from_slice(&DISCRIMINATOR);
    data[8..40].copy_from_slice(&[0x99u8; 32]);
    data[40..72].copy_from_slice(feed_id);
    data[72..80].copy_from_slice(&(settle_at - i64::from(TWAP_WINDOW)).to_le_bytes());
    data[80..88].copy_from_slice(&settle_at.to_le_bytes());
    data[88..96].copy_from_slice(&price.to_le_bytes());
    data[96..104].copy_from_slice(&conf.to_le_bytes());
    data[104..108].copy_from_slice(&(-8i32).to_le_bytes());
    data[108..112].copy_from_slice(&down.to_le_bytes());
    data
}

fn anchor_account<T: AnchorSerialize + Discriminator>(value: &T) -> Vec<u8> {
    let mut data = T::DISCRIMINATOR.to_vec();
    value.serialize(&mut data).expect("account serialises");
    data
}

/// The collateral mint, with an authority the test can fund accounts from.
///
/// A real deployment would point this at an existing mint like USDC, which has
/// its own authority and no relationship to this protocol.
fn mint_bytes(decimals: u8, authority: Pubkey) -> Vec<u8> {
    use solana_sdk::program_pack::Pack;
    let mut data = vec![0u8; spl_token::state::Mint::LEN];
    spl_token::state::Mint {
        mint_authority: Some(authority).into(),
        supply: 0,
        decimals,
        is_initialized: true,
        freeze_authority: None.into(),
    }
    .pack_into_slice(&mut data);
    data
}

pub fn params() -> MarketParams {
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

pub fn config_pda() -> Pubkey {
    Pubkey::find_program_address(&[b"config"], &prediction_market::ID).0
}

pub fn collateral_pda(mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"collateral", mint.as_ref()], &prediction_market::ID).0
}

pub fn feed_pda(source_id: &[u8; 32]) -> Pubkey {
    Pubkey::find_program_address(&[b"feed", source_id], &prediction_market::ID).0
}

pub fn market_pda(market_id: &[u8; 32]) -> Pubkey {
    Pubkey::find_program_address(&[b"market", market_id], &prediction_market::ID).0
}

pub fn child_pda(prefix: &[u8], market: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[prefix, market.as_ref()], &prediction_market::ID).0
}

impl Localnet {
    /// Brings up a validator with the program deployed and governance already
    /// settled, and returns once it answers.
    pub fn start() -> Localnet {
        let root = workspace_root();
        let program = root.join("target/deploy/prediction_market.so");
        assert!(
            program.exists(),
            "run `anchor build` before the live-node tests"
        );

        let workdir = std::env::temp_dir().join(format!("e2e-tests-{}", std::process::id()));
        let _ = fs::remove_dir_all(&workdir);
        fs::create_dir_all(&workdir).expect("workdir");

        let payer = Keypair::new();
        let authority = Keypair::new();
        let treasury = Pubkey::new_unique();
        let raydium = Pubkey::new_unique();
        let pyth = Pubkey::new_unique();
        let collateral_mint = Pubkey::new_unique();
        let pyth_feed_id = [0x5au8; 32];
        let pyth_account = Pubkey::new_unique();
        let settle_at = now() + SECONDS_TO_SETTLEMENT;

        let genesis = workdir.join("accounts");
        fs::create_dir_all(&genesis).expect("genesis dir");

        // The collateral mint, and the two registries as governance would have
        // left them: effective in the past, so markets can use them now.
        write_account(
            &genesis,
            &collateral_mint,
            &spl_token::ID,
            &mint_bytes(COLLATERAL_DECIMALS, authority.pubkey()),
        );
        write_account(
            &genesis,
            &config_pda(),
            &prediction_market::ID,
            &anchor_account(&Config {
                bump: Pubkey::find_program_address(&[b"config"], &prediction_market::ID).1,
                authority: authority.pubkey(),
                pending_authority: Pubkey::default(),
                treasury,
                paused: false,
                params: params(),
                pending_params: MarketParams::default(),
                pending_effective_at: 0,
                has_pending: false,
                timelock: TIMELOCK,
                raydium_clmm_program: raydium,
                pyth_receiver_program: pyth,
                markets_created: 0,
            }),
        );
        write_account(
            &genesis,
            &collateral_pda(&collateral_mint),
            &prediction_market::ID,
            &anchor_account(&Collateral {
                bump: Pubkey::find_program_address(
                    &[b"collateral", collateral_mint.as_ref()],
                    &prediction_market::ID,
                )
                .1,
                mint: collateral_mint,
                decimals: COLLATERAL_DECIMALS,
                enabled: true,
                min_stake: 1_000,
            }),
        );

        let mut rings = Vec::new();
        let mut pools = Vec::new();
        for _ in 0..3 {
            let pool = Pubkey::new_unique();
            let ring = Pubkey::new_unique();
            write_account(&genesis, &pool, &raydium, &pool_bytes(&pool, &ring, &collateral_mint));
            write_account(
                &genesis,
                &ring,
                &raydium,
                &ring_bytes(&pool, TICK_AT_100, settle_at + 60),
            );
            write_account(
                &genesis,
                &feed_pda(&ring.to_bytes()),
                &prediction_market::ID,
                &anchor_account(&Feed {
                    bump: Pubkey::find_program_address(&[b"feed", ring.as_ref()], &prediction_market::ID).1,
                    kind: FeedKind::RaydiumClmm,
                    source_id: ring.to_bytes(),
                    pool,
                    token0_mint: Pubkey::default(),
                    token1_mint: Pubkey::default(),
                    token0_decimals: TOKEN_DECIMALS,
                    token1_decimals: COLLATERAL_DECIMALS,
                    depth_quote: 1_000_000_000_000,
                    effective_at: 0,
                    enabled: true,
                    label: [0u8; 32],
                }),
            );
            pools.push(pool);
            rings.push(ring);
        }

        // An unregistered pool, so `register_feed` has a genuine source to
        // probe during the run.
        let spare_pool = Pubkey::new_unique();
        let spare_ring = Pubkey::new_unique();
        write_account(
            &genesis,
            &spare_pool,
            &raydium,
            &pool_bytes(&spare_pool, &spare_ring, &collateral_mint),
        );
        write_account(
            &genesis,
            &spare_ring,
            &raydium,
            &ring_bytes(&spare_pool, TICK_AT_100, settle_at + 60),
        );

        // A dormant Pyth feed, for the mixed-source run.
        write_account(
            &genesis,
            &feed_pda(&pyth_feed_id),
            &prediction_market::ID,
            &anchor_account(&Feed {
                bump: Pubkey::find_program_address(&[b"feed", &pyth_feed_id], &prediction_market::ID).1,
                kind: FeedKind::PythTwap,
                source_id: pyth_feed_id,
                pool: Pubkey::default(),
                token0_mint: Pubkey::default(),
                token1_mint: Pubkey::default(),
                token0_decimals: 0,
                token1_decimals: 0,
                depth_quote: 1_000_000_000_000,
                effective_at: 0,
                enabled: true,
                label: [0u8; 32],
            }),
        );
        write_account(
            &genesis,
            &pyth_account,
            &pyth,
            &pyth_bytes(&pyth_feed_id, settle_at, 10_000_000_000, 10_000_000, 0),
        );

        let ledger = workdir.join("ledger");
        let mut command = Command::new("solana-test-validator");
        command
            .arg("--reset")
            .arg("--quiet")
            .args(["--ledger", ledger.to_str().expect("path")])
            .args(["--bpf-program", &prediction_market::ID.to_string(), program.to_str().expect("path")])
            .args(["--account-dir", genesis.to_str().expect("path")]);
        let mut validator = command
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("solana-test-validator is on PATH");

        let rpc = Rpc::new("http://127.0.0.1:8899");
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            if rpc.is_healthy() && rpc.slot().unwrap_or(0) > 1 {
                break;
            }
            assert!(Instant::now() < deadline, "validator did not come up");
            assert!(
                validator.try_wait().expect("wait").is_none(),
                "validator exited during startup"
            );
            std::thread::sleep(Duration::from_millis(500));
        }

        for account in [&payer, &authority] {
            rpc.airdrop(&account.pubkey(), 100 * LAMPORTS_PER_SOL)
                .expect("airdrop");
        }

        let mut log = fs::File::create(workdir.join("run.log")).expect("log");
        writeln!(log, "settle_at {settle_at}, started {}", now()).ok();
        println!("  ledger and genesis accounts under {}", workdir.display());

        Localnet {
            validator,
            rpc,
            payer,
            authority,
            treasury,
            raydium,
            pyth,
            collateral_mint,
            rings,
            pools,
            pyth_feed_id,
            pyth_account,
            spare_pool,
            spare_ring,
            settle_at,
            workdir,
        }
    }

    /// Blocks until the chain's own clock reaches `instant`.
    ///
    /// The chain's clock, not the host's: they drift, and every deadline in the
    /// protocol is read from the former.
    pub fn wait_until(&self, instant: i64) {
        let deadline = Instant::now() + Duration::from_secs(900);
        loop {
            if let Ok(chain_time) = self.rpc.block_time() {
                if chain_time >= instant {
                    return;
                }
            }
            assert!(Instant::now() < deadline, "chain never reached {instant}");
            std::thread::sleep(Duration::from_millis(400));
        }
    }

    pub fn chain_time(&self) -> i64 {
        self.rpc.block_time().expect("block time")
    }

}
