//! Values identical for every deployment. Anything a deployment might want to
//! differ -- fees, windows, the programs it reads -- is governed via `Config`.

use anchor_lang::constant;

/// Gathered so a typo cannot open a second namespace: `"markets"` in one
/// instruction and `"market"` in another derives two accounts and no complaint.
pub mod seeds {
    pub const CONFIG: &[u8] = b"config";
    pub const COLLATERAL: &[u8] = b"collateral";
    pub const FEED: &[u8] = b"feed";
    pub const MARKET: &[u8] = b"market";
    pub const SPEC: &[u8] = b"spec";
    pub const SNAPSHOT: &[u8] = b"snapshot";
    pub const VAULT: &[u8] = b"vault";
    pub const YES_MINT: &[u8] = b"yes";
    pub const NO_MINT: &[u8] = b"no";
}

/// Durations, and the one place a test build differs from a real one.
pub mod time {
    /// Shortens the governance timelock only, so an end-to-end run need not wait
    /// an hour. Nothing that protects money -- averaging windows, grace and
    /// claim periods -- is touched.
    #[cfg(feature = "test")]
    pub const IS_TEST: bool = true;
    #[cfg(not(feature = "test"))]
    pub const IS_TEST: bool = false;

    pub const ONE_MINUTE: u32 = 60;
    pub const ONE_HOUR: u32 = 60 * ONE_MINUTE;
    pub const ONE_DAY: u32 = 24 * ONE_HOUR;

    /// Shortest governance delay a deployment may configure.
    pub const MIN_TIMELOCK: u32 = if IS_TEST { 5 } else { ONE_HOUR };

    /// Longest, so a deployment cannot be frozen by setting an absurd delay.
    pub const MAX_TIMELOCK: u32 = 30 * ONE_DAY;
}

/// Three, so a median survives one source being captured. Raising it costs
/// coverage; lowering it costs the median its point.
///
/// `#[constant]` puts it in the IDL, which is where the client reads it from —
/// a second copy in the front end would drift the first time this changes.
#[constant]
pub const MIN_MARKET_FEEDS: u8 = 3;

/// How the protocol fee splits, in basis points of the fee. The crank's cut is
/// small because it is already paid a fixed reward from the bond either way.
pub const FEE_SHARE_TREASURY_BPS: u64 = 7_000;
pub const FEE_SHARE_CREATOR_BPS: u64 = 2_500;
