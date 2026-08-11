//! Static verification by abstract interpretation, once in `create_market`: the
//! program decodes, never over- or underflows the stack, is type-correct, and
//! leaves exactly one readable score.
//!
//! With no control flow the type stack after each instruction is unique, so this
//! is one linear pass and no fixed point. That is why `SELECT` replaced `IF`.

use crate::decode::Decoder;
use crate::op::{Op, Operand};
use crate::{Type, VmError, MAX_CODE_LEN, MAX_INPUTS, MAX_OPS, MAX_STACK, MIN_INPUTS};

/// A program proved safe to interpret, and the bytes it was proved over.
///
/// Execution hangs off this type, not a free function, so unverified bytes
/// cannot be run: the only way to obtain one is [`verify`]. It also pins the
/// input count that was checked, which carries "every declared feed is read"
/// into execution instead of leaving it at creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedProgram<'a> {
    pub(crate) code: &'a [u8],
    /// Number of instructions. Also the exact interpreter step count.
    pub ops: u16,
    /// Peak operand stack depth reached.
    pub max_stack_depth: u8,
    /// Number of declared inputs, every one of which the program reads.
    pub input_count: u8,
}

/// Verifies `code` against a market declaring `input_count` price feeds.
pub fn verify(code: &[u8], input_count: usize) -> Result<VerifiedProgram<'_>, VmError> {
    if !(MIN_INPUTS..=MAX_INPUTS).contains(&input_count) {
        return Err(VmError::InputCountOutOfRange);
    }
    if code.len() > MAX_CODE_LEN {
        return Err(VmError::CodeTooLong);
    }

    let mut stack = TypeStack::default();
    let mut decoder = Decoder::new(code);
    let mut ops: u16 = 0;
    let mut inputs_read: u16 = 0;

    while let Some(instruction) = decoder.next() {
        let (op, operand) = instruction?;
        ops = ops.checked_add(1).ok_or(VmError::TooManyOps)?;
        if ops as usize > MAX_OPS {
            return Err(VmError::TooManyOps);
        }
        if let Operand::Index(index) = operand {
            if index as usize >= input_count {
                return Err(VmError::InvalidInputIndex);
            }
            inputs_read |= 1 << index;
        }
        step(&mut stack, op, operand)?;
    }

    for index in 0..input_count {
        if inputs_read & (1 << index) == 0 {
            return Err(VmError::UnusedInput(index as u8));
        }
    }

    // A predicate must leave a measurement, never a verdict. Deciding who won
    // is the protocol's job -- it applies the settlement band around the strike
    // -- and a program that hands back a bare `Bool` has already decided,
    // yielding a score of 0 or 1 that any ordinary strike sits far above. Such
    // a market would silently pay one side always. A program that genuinely
    // wants a two-valued score writes `SELECT` over two constants, which is
    // explicit and forces its author to pick a strike that suits it.
    match stack.types() {
        [Type::Num] => Ok(VerifiedProgram {
            code,
            ops,
            max_stack_depth: stack.peak,
            input_count: input_count as u8,
        }),
        [_] => Err(VmError::ResultNotScore),
        _ => Err(VmError::ResultNotSingleton),
    }
}

/// Applies one instruction's effect to the type stack.
fn step(stack: &mut TypeStack, op: Op, operand: Operand) -> Result<(), VmError> {
    use Op::*;
    match op {
        PushInput | PushConst | PushTime => stack.push(Type::Num),
        PushBytes32 => stack.push(Type::Bytes),

        Add | Sub | Mul | Div | Modulo | Min | Max => {
            stack.pop_expecting(Type::Num)?;
            stack.pop_expecting(Type::Num)?;
            stack.push(Type::Num)
        }
        Abs | Negate => {
            stack.pop_expecting(Type::Num)?;
            stack.push(Type::Num)
        }

        Equal | NotEqual | LessThan | GreaterThan | LessThanOrEqual | GreaterThanOrEqual => {
            stack.pop_expecting(Type::Num)?;
            stack.pop_expecting(Type::Num)?;
            stack.push(Type::Bool)
        }
        Within => {
            stack.pop_expecting(Type::Num)?;
            stack.pop_expecting(Type::Num)?;
            stack.pop_expecting(Type::Num)?;
            stack.push(Type::Bool)
        }

        And | Or | Xor => {
            stack.pop_expecting(Type::Bool)?;
            stack.pop_expecting(Type::Bool)?;
            stack.push(Type::Bool)
        }
        Not => {
            stack.pop_expecting(Type::Bool)?;
            stack.push(Type::Bool)
        }

        Sha256 | Keccak256 | Hash256 => {
            stack.pop_expecting(Type::Bytes)?;
            stack.push(Type::Bytes)
        }
        NumToBytes => {
            stack.pop_expecting(Type::Num)?;
            stack.push(Type::Bytes)
        }
        BytesEqual => {
            stack.pop_expecting(Type::Bytes)?;
            stack.pop_expecting(Type::Bytes)?;
            stack.push(Type::Bool)
        }

        Select => {
            // Stack order is `cond a b` with `b` on top.
            let b = stack.pop()?;
            let a = stack.pop()?;
            stack.pop_expecting(Type::Bool)?;
            if a != b {
                return Err(VmError::TypeMismatch);
            }
            stack.push(a)
        }
        Clamp => {
            stack.pop_expecting(Type::Num)?;
            stack.pop_expecting(Type::Num)?;
            stack.pop_expecting(Type::Num)?;
            stack.push(Type::Num)
        }
        Median | Mean => {
            let Operand::Arity(arity) = operand else {
                return Err(VmError::InvalidAggregateArity);
            };
            if arity == 0 {
                return Err(VmError::InvalidAggregateArity);
            }
            for _ in 0..arity {
                stack.pop_expecting(Type::Num)?;
            }
            stack.push(Type::Num)
        }
    }
}

/// The abstract stack: types only, no values.
struct TypeStack {
    slots: [Type; MAX_STACK],
    depth: u8,
    peak: u8,
}

impl Default for TypeStack {
    fn default() -> Self {
        TypeStack {
            slots: [Type::Num; MAX_STACK],
            depth: 0,
            peak: 0,
        }
    }
}

impl TypeStack {
    fn types(&self) -> &[Type] {
        &self.slots[..self.depth as usize]
    }

    fn push(&mut self, ty: Type) -> Result<(), VmError> {
        let depth = self.depth as usize;
        if depth >= MAX_STACK {
            return Err(VmError::StackOverflow);
        }
        self.slots[depth] = ty;
        self.depth += 1;
        self.peak = self.peak.max(self.depth);
        Ok(())
    }

    fn pop(&mut self) -> Result<Type, VmError> {
        self.depth = self.depth.checked_sub(1).ok_or(VmError::StackUnderflow)?;
        Ok(self.slots[self.depth as usize])
    }

    fn pop_expecting(&mut self, expected: Type) -> Result<(), VmError> {
        if self.pop()? == expected {
            Ok(())
        } else {
            Err(VmError::TypeMismatch)
        }
    }
}
