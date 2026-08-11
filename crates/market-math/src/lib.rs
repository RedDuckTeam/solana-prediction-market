//! Q64.64 fixed-point arithmetic. Every price and money computation goes here.
//!
//! Three test-enforced properties: overflow is a `Result`, never a wrap;
//! rounding floors toward negative infinity even for negative operands, where
//! Rust's native division would truncate toward zero and disagree with the
//! browser; and no floating point outside `#[cfg(test)]`.
//!
//! `no_std` and Solana-free, so one object code backs program, tests and wasm.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

mod q64;
mod tables;
mod tick;
mod u256;

#[cfg(test)]
mod pow_vectors;

pub use q64::Q64;
pub use tables::MAX_TICK;
pub use tick::pow_1_0001;

/// Every way arithmetic in this crate can fail.
///
/// These map onto a market-level abort: a predicate that produces one of these
/// resolves the market to `Void` with a full refund, never to a winning side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathError {
    /// The exact result does not fit in the destination type.
    Overflow,
    /// Division or modulo by zero.
    DivisionByZero,
    /// A tick outside +/-[`MAX_TICK`], where Q64.64 stops being meaningful.
    TickOutOfRange,
    /// A base-10 rescale exponent outside +/-[`q64::MAX_POW10_EXP`].
    ExponentOutOfRange,
}

impl core::fmt::Display for MathError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            MathError::Overflow => "arithmetic overflow",
            MathError::DivisionByZero => "division by zero",
            MathError::TickOutOfRange => "tick out of range",
            MathError::ExponentOutOfRange => "base-10 exponent out of range",
        };
        f.write_str(s)
    }
}
