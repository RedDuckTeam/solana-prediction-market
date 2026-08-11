//! The predicate machine: a stack program over a market's declared prices,
//! written once, hashed into the market, evaluated once at resolution.
//!
//! * No control flow — `SELECT` evaluates both arms, so instruction count is
//!   running time and both are known before any money enters.
//! * Verified once, at creation, by abstract interpretation; the interpreter
//!   re-checks nothing.
//! * No allocation and no panics.
//!
//! A runtime failure aborts and voids the market, never returns `false`: a buggy
//! predicate must not be payable to either side.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

mod decode;
mod encode;
mod exec;
mod op;
mod verify;

#[cfg(test)]
mod tests;

pub use encode::Encoder;
pub use exec::EvalContext;
pub use op::Op;
pub use verify::{verify, VerifiedProgram};

use market_math::MathError;

/// Sized so a `MarketSpec` with the longest feed list and program still fits a
/// single 10240-byte account creation, with no realloc.
pub const MAX_CODE_LEN: usize = 2048;

pub const MAX_OPS: usize = 256;

/// Bounded by the BPF stack, not by expressiveness. A slot is a 40-byte `Value`,
/// so 64 would put 2.5 KiB in a 4 KiB frame. A median over eight inputs peaks
/// at eight.
pub const MAX_STACK: usize = 32;

pub const MAX_INPUTS: usize = 8;

/// A program that reads nothing is not a predicate. How many *sources* a market
/// must consult is an economic question and belongs to the protocol.
pub const MIN_INPUTS: usize = 1;

/// The static type of a stack slot.
///
/// `Bool` is tracked apart from `Num` though both are a `Q64` at runtime, so the
/// verifier can reject `ADD` on a comparison result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Num,
    Bool,
    Bytes,
}

/// Verification errors reach the creator before the market exists; runtime
/// errors void it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmError {
    // -- Verification --------------------------------------------------
    CodeTooLong,
    TooManyOps,
    UnknownOpcode(u8),
    TruncatedOperand,
    StackUnderflow,
    StackOverflow,
    TypeMismatch,
    InvalidInputIndex,
    InvalidAggregateArity,
    ResultNotSingleton,
    ResultNotScore,
    /// A declared input that nothing reads: a source that appears to be in the
    /// median but influences nothing.
    UnusedInput(u8),
    InputCountOutOfRange,

    // -- Runtime -------------------------------------------------------
    Math(MathError),
    /// Unreachable for a verified program; kept as a second line of defence.
    StepLimitExceeded,
    /// Running verified bytecode against a different-sized input vector would
    /// let a market declare eight sources and consult three.
    InputCountMismatch,
}

impl From<MathError> for VmError {
    fn from(e: MathError) -> Self {
        VmError::Math(e)
    }
}

/// Hashing supplied by the host: syscalls on chain, plain crates elsewhere.
///
/// RIPEMD-160 is deliberately absent -- Solana has no syscall for it, and a
/// software implementation buys nothing SHA-256 does not already give.
pub trait HostHasher {
    fn sha256(&self, data: &[u8]) -> [u8; 32];
    fn keccak256(&self, data: &[u8]) -> [u8; 32];
}
