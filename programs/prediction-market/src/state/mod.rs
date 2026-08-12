//! On-chain accounts, one module each, shaped by two rules.
//!
//! Hot state is split from cold: `Market` is small and written on every deposit,
//! `MarketSpec` is kilobytes and never written after creation. And parameters
//! are copied, not referenced, so a governance change cannot rewrite the rules
//! of a market that already holds money.

pub mod collateral;
pub use collateral::*;

pub mod config;
pub use config::*;

pub mod feed;
pub use feed::*;

pub mod market;
pub use market::*;

pub mod params;
pub use params::*;

pub mod snapshot;
pub use snapshot::*;
