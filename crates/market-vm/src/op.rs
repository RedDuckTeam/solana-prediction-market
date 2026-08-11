//! Opcodes and their stack signatures.

use crate::VmError;

/// A predicate instruction.
///
/// The numeric values are a permanent part of the on-chain format: a market's
/// `spec_hash` commits to the exact bytes, and markets outlive releases.
/// **Never renumber an existing opcode.** New opcodes take unused values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Op {
    // -- Producers -----------------------------------------------------
    /// `PUSH_INPUT i` -- the resolved price of declared feed `i`.
    PushInput = 0x01,
    /// `PUSH_CONST v` -- a Q64.64 literal, 16 bytes little-endian.
    PushConst = 0x02,
    /// `PUSH_BYTES32 b` -- a 32-byte literal.
    PushBytes32 = 0x03,
    /// The market's settlement instant, in whole seconds, as a number.
    PushTime = 0x04,

    // -- Arithmetic ----------------------------------------------------
    Add = 0x10,
    Sub = 0x11,
    Mul = 0x12,
    Div = 0x13,
    Modulo = 0x14,
    Min = 0x15,
    Max = 0x16,
    Abs = 0x17,
    Negate = 0x18,

    // -- Comparison ----------------------------------------------------
    Equal = 0x20,
    NotEqual = 0x21,
    LessThan = 0x22,
    GreaterThan = 0x23,
    LessThanOrEqual = 0x24,
    GreaterThanOrEqual = 0x25,
    /// `WITHIN x lo hi` -- true when `lo <= x < hi`, half-open like Bitcoin's.
    Within = 0x26,

    // -- Logic ---------------------------------------------------------
    And = 0x30,
    Or = 0x31,
    Xor = 0x32,
    Not = 0x33,

    // -- Bytes ---------------------------------------------------------
    Sha256 = 0x40,
    Keccak256 = 0x41,
    /// Double SHA-256, the `OP_HASH256` of Bitcoin Script.
    Hash256 = 0x42,
    /// Reinterpret a number as its 32-byte big-endian sign-extended form.
    NumToBytes = 0x43,
    BytesEqual = 0x44,

    // -- Selection and aggregation -------------------------------------
    /// `SELECT c a b` -- `a` when `c` is true, otherwise `b`.
    ///
    /// Replaces Bitcoin Script's `IF`/`NOTIF`/`ELSE`. Both alternatives are
    /// already on the stack, so cost is independent of the condition and the
    /// verifier needs no path analysis. For a side-effect-free predicate the
    /// expressive power is identical.
    Select = 0x50,
    /// `MEDIAN n` -- median of the top `n` numbers.
    Median = 0x51,
    /// `MEAN n` -- arithmetic mean of the top `n` numbers.
    Mean = 0x52,
    /// `CLAMP x lo hi` -- `x` confined to `[lo, hi]`. Not how the settlement
    /// band is expressed: a band in bytecode could not be checked on chain.
    ///
    /// With `lo > hi` the result is `hi`, since the upper bound applies last.
    /// Both bounds are runtime values, so the verifier cannot exclude that case
    /// and the order is fixed here instead.
    Clamp = 0x53,
}

impl Op {
    pub fn from_byte(byte: u8) -> Result<Op, VmError> {
        use Op::*;
        Ok(match byte {
            0x01 => PushInput,
            0x02 => PushConst,
            0x03 => PushBytes32,
            0x04 => PushTime,

            0x10 => Add,
            0x11 => Sub,
            0x12 => Mul,
            0x13 => Div,
            0x14 => Modulo,
            0x15 => Min,
            0x16 => Max,
            0x17 => Abs,
            0x18 => Negate,

            0x20 => Equal,
            0x21 => NotEqual,
            0x22 => LessThan,
            0x23 => GreaterThan,
            0x24 => LessThanOrEqual,
            0x25 => GreaterThanOrEqual,
            0x26 => Within,

            0x30 => And,
            0x31 => Or,
            0x32 => Xor,
            0x33 => Not,

            0x40 => Sha256,
            0x41 => Keccak256,
            0x42 => Hash256,
            0x43 => NumToBytes,
            0x44 => BytesEqual,

            0x50 => Select,
            0x51 => Median,
            0x52 => Mean,
            0x53 => Clamp,

            other => return Err(VmError::UnknownOpcode(other)),
        })
    }

    pub fn to_byte(self) -> u8 {
        self as u8
    }

    /// Width of this opcode's immediate operand, in bytes.
    pub fn operand_len(self) -> usize {
        match self {
            Op::PushInput | Op::Median | Op::Mean => 1,
            Op::PushConst => 16,
            Op::PushBytes32 => 32,
            _ => 0,
        }
    }
}

/// A decoded immediate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Operand {
    None,
    /// `PUSH_INPUT` feed index.
    Index(u8),
    /// `MEDIAN`/`MEAN` element count.
    Arity(u8),
    /// `PUSH_CONST` raw Q64.64 value.
    Const(i128),
    /// `PUSH_BYTES32` literal.
    Bytes32([u8; 32]),
}
