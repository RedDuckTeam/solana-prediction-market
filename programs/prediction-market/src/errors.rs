//! Program errors, and how the pure crates' errors map onto them.
//!
//! Lossy in one direction only: a caller learns enough to fix a transaction and
//! nothing that helps them steer a market. Every arithmetic failure inside a
//! predicate collapses to [`MarketError::PredicateAborted`] — the market voids
//! either way.

use anchor_lang::prelude::*;
use market_core::CoreError;
use market_feeds::FeedError;
use market_vm::VmError;

#[error_code]
pub enum MarketError {
    // -- Governance ----------------------------------------------------
    #[msg("Program is paused")]
    Paused,
    #[msg("Caller is not the governance authority")]
    NotAuthorized,
    #[msg("Timelock has not elapsed")]
    TimelockPending,
    #[msg("No parameter change is pending")]
    NothingPending,
    #[msg("Parameter is outside its permitted range")]
    ParameterOutOfRange,

    // -- Feed registry -------------------------------------------------
    #[msg("Feed is not yet effective or has been disabled")]
    FeedNotActive,
    #[msg("Feed does not belong to the pool it names")]
    FeedPoolMismatch,
    #[msg("Feed accounts were not supplied in the order the spec declares")]
    FeedAccountsMismatch,
    #[msg("A feed is listed more than once")]
    DuplicateFeed,

    // -- Market creation -----------------------------------------------
    #[msg("Collateral mint is not registered")]
    CollateralNotRegistered,
    #[msg("Market spec does not hash to the value recorded on the market")]
    SpecHashMismatch,
    #[msg("Settlement time is in the past or too far out")]
    SettlementTimeInvalid,
    #[msg("Predicate failed static verification")]
    PredicateInvalid,
    #[msg("Predicate declares a settlement band narrower than governance allows")]
    RampTooNarrow,
    #[msg("Rules URI is too long")]
    RulesUriTooLong,

    // -- Lifecycle -----------------------------------------------------
    #[msg("Action is not allowed in the market's current state")]
    WrongState,
    #[msg("Market is not accepting deposits right now")]
    DepositsClosed,
    #[msg("Snapshot may only be taken between settlement and the grace deadline")]
    OutsideSnapshotWindow,
    #[msg("Market cannot be voided yet")]
    VoidConditionNotMet,
    #[msg("Deposit would push this side past its cap")]
    CapExceeded,
    #[msg("Deposit amount is zero")]
    ZeroAmount,

    // -- Resolution ----------------------------------------------------
    #[msg("A declared price feed could not be read")]
    FeedUnreadable,
    #[msg("Predicate aborted; the market voids and refunds")]
    PredicateAborted,

    // -- Claiming ------------------------------------------------------
    #[msg("Nothing to claim for this side")]
    NothingToClaim,
    #[msg("Claim window has closed")]
    ClaimWindowClosed,
    #[msg("Claim window is still open")]
    ClaimWindowOpen,
    #[msg("Vault is not empty")]
    VaultNotEmpty,

    // -- Arithmetic ----------------------------------------------------
    #[msg("Arithmetic overflow")]
    Overflow,

    // Appended, never inserted: a variant's position is its error number, and
    // markets outlive releases.
    #[msg("Stake is below the minimum this collateral accepts")]
    BelowMinimumStake,
}

impl From<CoreError> for MarketError {
    fn from(error: CoreError) -> Self {
        match error {
            CoreError::CapExceeded => MarketError::CapExceeded,
            CoreError::ScheduleTooLong | CoreError::ScheduleInvalid => {
                MarketError::SettlementTimeInvalid
            }
            CoreError::FeeRateOutOfRange => MarketError::ParameterOutOfRange,
            CoreError::EmptySide | CoreError::BurnExceedsStake => MarketError::NothingToClaim,
            CoreError::ShareOutOfRange | CoreError::Overflow => MarketError::Overflow,
        }
    }
}

impl From<VmError> for MarketError {
    fn from(error: VmError) -> Self {
        match error {
            // Runtime failures void the market; everything else is a
            // verification failure that should have been caught at creation.
            VmError::Math(_) | VmError::StepLimitExceeded | VmError::InputCountMismatch => {
                MarketError::PredicateAborted
            }
            _ => MarketError::PredicateInvalid,
        }
    }
}

impl From<FeedError> for MarketError {
    fn from(_: FeedError) -> Self {
        // Every way a feed can fail means the same thing to the protocol: this
        // price is not available, so the market cannot settle honestly.
        MarketError::FeedUnreadable
    }
}
