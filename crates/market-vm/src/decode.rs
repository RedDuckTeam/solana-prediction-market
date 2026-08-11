//! The single decoder shared by the verifier and the interpreter.
//!
//! Both passes must agree byte-for-byte on what a program says. The surest way
//! to guarantee that is to give them one implementation, so neither can drift
//! from the other.

use crate::op::{Op, Operand};
use crate::VmError;

pub(crate) struct Decoder<'a> {
    code: &'a [u8],
    position: usize,
}

impl<'a> Decoder<'a> {
    pub(crate) fn new(code: &'a [u8]) -> Self {
        Decoder { code, position: 0 }
    }

    pub(crate) fn is_done(&self) -> bool {
        self.position >= self.code.len()
    }

    /// Reads the next instruction, or `None` at the end of the program.
    pub(crate) fn next(&mut self) -> Option<Result<(Op, Operand), VmError>> {
        if self.is_done() {
            return None;
        }
        Some(self.read_instruction())
    }

    fn read_instruction(&mut self) -> Result<(Op, Operand), VmError> {
        let opcode = self.code[self.position];
        let op = Op::from_byte(opcode)?;
        self.position += 1;

        let operand_bytes = self
            .take(op.operand_len())
            .ok_or(VmError::TruncatedOperand)?;

        let operand = match op {
            Op::PushInput => Operand::Index(operand_bytes[0]),
            Op::Median | Op::Mean => Operand::Arity(operand_bytes[0]),
            Op::PushConst => {
                let mut raw = [0u8; 16];
                raw.copy_from_slice(operand_bytes);
                Operand::Const(i128::from_le_bytes(raw))
            }
            Op::PushBytes32 => {
                let mut raw = [0u8; 32];
                raw.copy_from_slice(operand_bytes);
                Operand::Bytes32(raw)
            }
            _ => Operand::None,
        };

        Ok((op, operand))
    }

    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.position.checked_add(len)?;
        let slice = self.code.get(self.position..end)?;
        self.position = end;
        Some(slice)
    }
}
