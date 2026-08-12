//! Instruction handlers, one module per instruction.
//!
//! Each module holds the `#[derive(Accounts)]` context beside the handler
//! that uses it, so the account checks and the logic they guard cannot drift
//! apart. Contexts serving several instructions live in `contexts`.

pub mod contexts;
pub use contexts::*;

pub mod initialize_config;
pub use initialize_config::*;

pub mod propose_params;
pub use propose_params::*;

pub mod adopt_params;
pub use adopt_params::*;

pub mod set_paused;
pub use set_paused::*;

pub mod nominate_authority;
pub use nominate_authority::*;

pub mod accept_authority;
pub use accept_authority::*;

pub mod register_collateral;
pub use register_collateral::*;

pub mod update_collateral;
pub use update_collateral::*;

pub mod register_feed;
pub use register_feed::*;

pub mod register_pyth_feed;
pub use register_pyth_feed::*;

pub mod update_feed;
pub use update_feed::*;

pub mod create_market;
pub use create_market::*;

pub mod deposit;
pub use deposit::*;

pub mod snapshot;
pub use snapshot::*;

pub mod resolve;
pub use resolve::*;

pub mod void;
pub use void::*;

pub mod claim;
pub use claim::*;

pub mod collect_fee;
pub use collect_fee::*;

pub mod sweep_dust;
pub use sweep_dust::*;

pub mod close_market;
pub use close_market::*;
