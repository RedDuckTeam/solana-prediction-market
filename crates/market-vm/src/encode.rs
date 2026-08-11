//! Writing bytecode.
//!
//! Used by tests, by the SDK, and by the market builder through the WebAssembly
//! bridge. Sharing the encoder with the decoder is what keeps the graph the
//! user drew and the bytes the chain hashes in agreement.

use market_math::Q64;

use crate::op::Op;
use crate::VmError;

/// Appends instructions to a caller-supplied buffer.
///
/// Borrowing the buffer rather than growing one keeps the crate allocation-free
/// so it can run inside a Solana program unchanged.
pub struct Encoder<'a> {
    buffer: &'a mut [u8],
    position: usize,
}

impl<'a> Encoder<'a> {
    pub fn new(buffer: &'a mut [u8]) -> Self {
        Encoder {
            buffer,
            position: 0,
        }
    }

    /// Emits an opcode that takes no immediate.
    ///
    /// Passing one that does is a programming error and is rejected rather
    /// than silently producing a truncated instruction.
    pub fn op(&mut self, op: Op) -> Result<&mut Self, VmError> {
        if op.operand_len() != 0 {
            return Err(VmError::TruncatedOperand);
        }
        self.write(&[op.to_byte()])
    }

    pub fn push_input(&mut self, index: u8) -> Result<&mut Self, VmError> {
        self.write(&[Op::PushInput.to_byte(), index])
    }

    pub fn push_const(&mut self, value: Q64) -> Result<&mut Self, VmError> {
        self.write(&[Op::PushConst.to_byte()])?
            .write(&value.raw().to_le_bytes())
    }

    pub fn push_bytes32(&mut self, value: &[u8; 32]) -> Result<&mut Self, VmError> {
        self.write(&[Op::PushBytes32.to_byte()])?.write(value)
    }

    pub fn push_time(&mut self) -> Result<&mut Self, VmError> {
        self.op(Op::PushTime)
    }

    /// Turns a boolean on top of the stack into a score of one or zero.
    ///
    /// A predicate has to end in a number, so a program built around a
    /// comparison says so explicitly here rather than having the conversion
    /// happen implicitly somewhere in the interpreter. Whoever writes this is
    /// then obliged to pick a strike and band that suit a two-valued score.
    pub fn bool_to_score(&mut self) -> Result<&mut Self, VmError> {
        self.push_const(Q64::ONE)?
            .push_const(Q64::ZERO)?
            .op(Op::Select)
    }

    pub fn median(&mut self, arity: u8) -> Result<&mut Self, VmError> {
        self.write(&[Op::Median.to_byte(), arity])
    }

    pub fn mean(&mut self, arity: u8) -> Result<&mut Self, VmError> {
        self.write(&[Op::Mean.to_byte(), arity])
    }

    pub fn len(&self) -> usize {
        self.position
    }

    pub fn is_empty(&self) -> bool {
        self.position == 0
    }

    /// The bytes written so far.
    pub fn code(&self) -> &[u8] {
        &self.buffer[..self.position]
    }

    fn write(&mut self, bytes: &[u8]) -> Result<&mut Self, VmError> {
        let end = self
            .position
            .checked_add(bytes.len())
            .ok_or(VmError::CodeTooLong)?;
        let target = self
            .buffer
            .get_mut(self.position..end)
            .ok_or(VmError::CodeTooLong)?;
        target.copy_from_slice(bytes);
        self.position = end;
        Ok(self)
    }
}
