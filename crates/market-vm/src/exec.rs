//! The interpreter.
//!
//! Runs a program that [`crate::verify`] has already accepted. It re-checks
//! nothing that verification proved, but it still cannot panic on hostile
//! input: every pop is checked and every arithmetic operation returns a
//! `Result`, so calling it on unverified bytes is merely useless, not unsafe.

use market_math::Q64;

use crate::decode::Decoder;
use crate::op::{Op, Operand};
use crate::verify::VerifiedProgram;
use crate::{HostHasher, VmError, MAX_OPS, MAX_STACK};

/// Everything a predicate is allowed to observe.
///
/// Note what is absent: no clock, no account data, no randomness. A predicate
/// is a pure function of the snapshot, which is what makes a resolution
/// reproducible from archived chain state years later.
pub struct EvalContext<'a, H: HostHasher> {
    /// Resolved feed prices, in declaration order.
    pub inputs: &'a [Q64],
    /// The market's settlement instant, in whole seconds since the epoch.
    pub settle_at: i64,
    pub hasher: &'a H,
}

impl VerifiedProgram<'_> {
    /// Evaluates the predicate and returns its score.
    ///
    /// The score is whatever the expression tree computes -- a price, a median
    /// of composed prices, a parity bit. Turning it into a payout is the
    /// protocol's job, not the predicate's: the settlement band has to be
    /// checkable on chain, and a band buried in bytecode is not.
    pub fn execute<H: HostHasher>(&self, ctx: &EvalContext<'_, H>) -> Result<Q64, VmError> {
        if ctx.inputs.len() != usize::from(self.input_count) {
            return Err(VmError::InputCountMismatch);
        }
        run(self.code, ctx)
    }
}

fn run<H: HostHasher>(code: &[u8], ctx: &EvalContext<'_, H>) -> Result<Q64, VmError> {
    let mut stack = Stack::default();
    let mut decoder = Decoder::new(code);
    let mut steps = 0usize;

    while let Some(instruction) = decoder.next() {
        let (op, operand) = instruction?;
        steps += 1;
        if steps > MAX_OPS {
            return Err(VmError::StepLimitExceeded);
        }
        step(&mut stack, op, operand, ctx)?;
    }

    let result = stack.pop()?;
    if stack.depth != 0 {
        return Err(VmError::ResultNotSingleton);
    }
    match result {
        Value::Num(score) => Ok(score),
        _ => Err(VmError::ResultNotScore),
    }
}

fn step<H: HostHasher>(
    stack: &mut Stack,
    op: Op,
    operand: Operand,
    ctx: &EvalContext<'_, H>,
) -> Result<(), VmError> {
    use Op::*;
    match op {
        PushInput => {
            let Operand::Index(index) = operand else {
                return Err(VmError::InvalidInputIndex);
            };
            let value = ctx
                .inputs
                .get(index as usize)
                .ok_or(VmError::InputCountMismatch)?;
            stack.push(Value::Num(*value))
        }
        PushConst => {
            let Operand::Const(raw) = operand else {
                return Err(VmError::TypeMismatch);
            };
            stack.push(Value::Num(Q64::from_raw(raw)))
        }
        PushBytes32 => {
            let Operand::Bytes32(bytes) = operand else {
                return Err(VmError::TypeMismatch);
            };
            stack.push(Value::Bytes(bytes))
        }
        PushTime => stack.push(Value::Num(Q64::from_int(ctx.settle_at))),

        Add | Sub | Mul | Div | Modulo | Min | Max => {
            let (lhs, rhs) = stack.pop_two_nums()?;
            let result = match op {
                Add => lhs.add(rhs)?,
                Sub => lhs.sub(rhs)?,
                Mul => lhs.mul(rhs)?,
                Div => lhs.div(rhs)?,
                Modulo => lhs.rem(rhs)?,
                Min => lhs.min(rhs),
                _ => lhs.max(rhs),
            };
            stack.push(Value::Num(result))
        }
        Abs => {
            let value = stack.pop_num()?;
            stack.push(Value::Num(value.abs()?))
        }
        Negate => {
            let value = stack.pop_num()?;
            stack.push(Value::Num(value.neg()?))
        }

        Equal | NotEqual | LessThan | GreaterThan | LessThanOrEqual | GreaterThanOrEqual => {
            let (lhs, rhs) = stack.pop_two_nums()?;
            let result = match op {
                Equal => lhs == rhs,
                NotEqual => lhs != rhs,
                LessThan => lhs < rhs,
                GreaterThan => lhs > rhs,
                LessThanOrEqual => lhs <= rhs,
                _ => lhs >= rhs,
            };
            stack.push(Value::Bool(result))
        }
        Within => {
            // Pushed as `x lo hi`, so `hi` comes off first.
            let high = stack.pop_num()?;
            let low = stack.pop_num()?;
            let value = stack.pop_num()?;
            stack.push(Value::Bool(low <= value && value < high))
        }

        And | Or | Xor => {
            let rhs = stack.pop_bool()?;
            let lhs = stack.pop_bool()?;
            let result = match op {
                And => lhs && rhs,
                Or => lhs || rhs,
                _ => lhs != rhs,
            };
            stack.push(Value::Bool(result))
        }
        Not => {
            let value = stack.pop_bool()?;
            stack.push(Value::Bool(!value))
        }

        Sha256 | Keccak256 | Hash256 => {
            let input = stack.pop_bytes()?;
            let digest = match op {
                Sha256 => ctx.hasher.sha256(&input),
                Keccak256 => ctx.hasher.keccak256(&input),
                _ => ctx.hasher.sha256(&ctx.hasher.sha256(&input)),
            };
            stack.push(Value::Bytes(digest))
        }
        NumToBytes => {
            let value = stack.pop_num()?;
            stack.push(Value::Bytes(num_to_bytes(value)))
        }
        BytesEqual => {
            let rhs = stack.pop_bytes()?;
            let lhs = stack.pop_bytes()?;
            stack.push(Value::Bool(lhs == rhs))
        }

        Select => {
            let alternative = stack.pop()?;
            let consequent = stack.pop()?;
            let condition = stack.pop_bool()?;
            stack.push(if condition { consequent } else { alternative })
        }
        Clamp => {
            // Pushed as `x lo hi`.
            let high = stack.pop_num()?;
            let low = stack.pop_num()?;
            let value = stack.pop_num()?;
            stack.push(Value::Num(value.max(low).min(high)))
        }
        Median | Mean => {
            let Operand::Arity(arity) = operand else {
                return Err(VmError::InvalidAggregateArity);
            };
            let count = arity as usize;
            if count == 0 {
                return Err(VmError::InvalidAggregateArity);
            }
            // Aggregated in place on the operand stack. Copying into a scratch
            // array would be clearer but costs another kilobyte of a 4 KiB BPF
            // frame, which the interpreter cannot spare.
            let base = stack
                .depth
                .checked_sub(count)
                .ok_or(VmError::StackUnderflow)?;
            let window = stack.numeric_window(base, count)?;
            let result = if op == Median {
                sort_values(window);
                median_of_sorted(window)?
            } else {
                mean_of(window)?
            };
            stack.depth = base;
            stack.push(Value::Num(result))
        }
    }
}

/// The number in a slot.
///
/// Only ever called on a window returned by [`Stack::numeric_window`], which
/// has already rejected anything else. Keeping the check and the extraction in
/// separate places would mean a later reordering turned a type error into a
/// silent zero, so the only way to reach this is through that check.
fn as_num(value: &Value) -> Q64 {
    match value {
        Value::Num(number) => *number,
        _ => Q64::ZERO,
    }
}

/// Median of an already-sorted window.
///
/// For an even count the result is the midpoint of the two central elements,
/// computed as `low + (high - low)/2` rather than `(low + high)/2` so that
/// summing two large prices cannot overflow.
fn median_of_sorted(values: &[Value]) -> Result<Q64, VmError> {
    let middle = values.len() / 2;
    if values.len() % 2 == 1 {
        return Ok(as_num(&values[middle]));
    }
    let low = as_num(&values[middle - 1]);
    let high = as_num(&values[middle]);
    let half_span = high.sub(low)?.div(Q64::from_int(2))?;
    Ok(low.add(half_span)?)
}

/// Arithmetic mean.
///
/// The running sum can overflow for extreme inputs; that is an abort, which
/// voids the market. Prices that would overflow a Q64.64 sum are far outside
/// anything a valid feed can produce.
fn mean_of(values: &[Value]) -> Result<Q64, VmError> {
    let mut total = Q64::ZERO;
    for value in values.iter() {
        total = total.add(as_num(value))?;
    }
    Ok(total.div(Q64::from_int(values.len() as i64))?)
}

/// Insertion sort: allocation-free, and optimal for the `n <= 8` this ever
/// sees.
fn sort_values(values: &mut [Value]) {
    for i in 1..values.len() {
        let mut j = i;
        while j > 0 && as_num(&values[j - 1]) > as_num(&values[j]) {
            values.swap(j - 1, j);
            j -= 1;
        }
    }
}

/// Sign-extended 32-byte big-endian encoding of the raw Q64.64 integer.
fn num_to_bytes(value: Q64) -> [u8; 32] {
    let raw = value.raw();
    let mut out = [if raw < 0 { 0xff } else { 0x00 }; 32];
    out[16..].copy_from_slice(&raw.to_be_bytes());
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Value {
    Num(Q64),
    Bool(bool),
    Bytes([u8; 32]),
}

struct Stack {
    slots: [Value; MAX_STACK],
    depth: usize,
}

impl Default for Stack {
    fn default() -> Self {
        Stack {
            slots: [Value::Num(Q64::ZERO); MAX_STACK],
            depth: 0,
        }
    }
}

impl Stack {
    fn push(&mut self, value: Value) -> Result<(), VmError> {
        if self.depth >= MAX_STACK {
            return Err(VmError::StackOverflow);
        }
        self.slots[self.depth] = value;
        self.depth += 1;
        Ok(())
    }

    fn pop(&mut self) -> Result<Value, VmError> {
        self.depth = self.depth.checked_sub(1).ok_or(VmError::StackUnderflow)?;
        Ok(self.slots[self.depth])
    }

    /// Borrows `count` slots starting at `base`, having proved every one of
    /// them holds a number.
    ///
    /// The proof and the borrow are the same operation on purpose: aggregation
    /// reads these slots without re-checking, and a check that lives somewhere
    /// else can be moved somewhere else again.
    fn numeric_window(&mut self, base: usize, count: usize) -> Result<&mut [Value], VmError> {
        let end = base.checked_add(count).ok_or(VmError::StackUnderflow)?;
        let window = self
            .slots
            .get_mut(base..end)
            .ok_or(VmError::StackUnderflow)?;
        for slot in window.iter() {
            if !matches!(slot, Value::Num(_)) {
                return Err(VmError::TypeMismatch);
            }
        }
        Ok(window)
    }

    fn pop_num(&mut self) -> Result<Q64, VmError> {
        match self.pop()? {
            Value::Num(value) => Ok(value),
            _ => Err(VmError::TypeMismatch),
        }
    }

    /// Pops two numbers and returns them in push order, so callers read
    /// `lhs OP rhs` the way the program was written.
    fn pop_two_nums(&mut self) -> Result<(Q64, Q64), VmError> {
        let rhs = self.pop_num()?;
        let lhs = self.pop_num()?;
        Ok((lhs, rhs))
    }

    fn pop_bool(&mut self) -> Result<bool, VmError> {
        match self.pop()? {
            Value::Bool(value) => Ok(value),
            _ => Err(VmError::TypeMismatch),
        }
    }

    fn pop_bytes(&mut self) -> Result<[u8; 32], VmError> {
        match self.pop()? {
            Value::Bytes(value) => Ok(value),
            _ => Err(VmError::TypeMismatch),
        }
    }
}
