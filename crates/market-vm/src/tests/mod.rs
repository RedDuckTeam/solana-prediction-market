use market_math::{MathError, Q64};
use proptest::prelude::*;
use sha2::Digest;

use crate::{verify, Encoder, EvalContext, HostHasher, Op, VmError, MAX_OPS};

/// Room for any program these tests build, so that sizing a buffer is never
/// the thing a test is measuring.
pub(crate) const CODE: usize = 256;

mod opcodes;

pub(crate) struct TestHasher;

impl HostHasher for TestHasher {
    fn sha256(&self, data: &[u8]) -> [u8; 32] {
        sha2::Sha256::digest(data).into()
    }

    fn keccak256(&self, data: &[u8]) -> [u8; 32] {
        use tiny_keccak::Hasher;
        let mut out = [0u8; 32];
        let mut keccak = tiny_keccak::Keccak::v256();
        keccak.update(data);
        keccak.finalize(&mut out);
        out
    }
}

pub(crate) fn context<'a>(inputs: &'a [Q64]) -> EvalContext<'a, TestHasher> {
    EvalContext {
        inputs,
        settle_at: 1_800_000_000,
        hasher: &TestHasher,
    }
}

fn run(code: &[u8], inputs: &[Q64]) -> Result<Q64, VmError> {
    verify(code, inputs.len())
        .expect("program should verify")
        .execute(&context(inputs))
}

/// The canonical v1 market: "median of three feeds is above the strike".
fn above_strike(strike: Q64) -> ([u8; CODE], usize) {
    let mut buffer = [0u8; CODE];
    let mut encoder = Encoder::new(&mut buffer);
    encoder
        .push_input(0)
        .and_then(|e| e.push_input(1))
        .and_then(|e| e.push_input(2))
        .and_then(|e| e.median(3))
        .and_then(|e| e.push_const(strike))
        .and_then(|e| e.op(Op::GreaterThan))
        .and_then(|e| e.bool_to_score())
        .expect("encoding fits");
    let len = encoder.len();
    (buffer, len)
}

fn price(units: i64) -> Q64 {
    Q64::from_int(units)
}

#[test]
fn canonical_above_strike_market_resolves_both_ways() {
    let (buffer, len) = above_strike(price(100));
    let code = &buffer[..len];

    // Median is 101 -> YES.
    assert_eq!(
        run(code, &[price(99), price(101), price(140)]),
        Ok(Q64::ONE)
    );
    // Median is 100, and the comparison is strict -> NO.
    assert_eq!(
        run(code, &[price(100), price(100), price(100)]),
        Ok(Q64::ZERO)
    );
    // One captured source cannot move the median.
    assert_eq!(
        run(code, &[price(99), price(98), price(1_000_000)]),
        Ok(Q64::ZERO)
    );
    assert_eq!(run(code, &[price(101), price(102), price(0)]), Ok(Q64::ONE));
}

#[test]
fn median_ignores_a_single_extreme_source() {
    let (buffer, len) = above_strike(price(100));
    let code = &buffer[..len];
    // Whichever position the captured feed occupies, the honest pair decides.
    for corrupted in 0..3 {
        let mut inputs = [price(101), price(102), price(103)];
        inputs[corrupted] = price(0);
        assert_eq!(
            run(code, &inputs),
            Ok(Q64::ONE),
            "corrupted index {corrupted}"
        );
        let mut inputs = [price(1), price(2), price(3)];
        inputs[corrupted] = price(1_000_000);
        assert_eq!(
            run(code, &inputs),
            Ok(Q64::ZERO),
            "corrupted index {corrupted}"
        );
    }
}

#[test]
fn median_of_an_even_count_is_the_midpoint() {
    let mut buffer = [0u8; CODE];
    let mut encoder = Encoder::new(&mut buffer);
    encoder
        .push_input(0)
        .and_then(|e| e.push_input(1))
        .and_then(|e| e.push_input(2))
        .and_then(|e| e.push_input(3))
        .and_then(|e| e.median(4))
        .and_then(|e| e.push_const(price(25)))
        .and_then(|e| e.op(Op::Equal))
        .and_then(|e| e.bool_to_score())
        .expect("encoding fits");
    let code = encoder.code();
    // Sorted: 10, 20, 30, 40 -> midpoint of 20 and 30 is 25.
    assert_eq!(
        run(code, &[price(40), price(10), price(30), price(20)]),
        Ok(Q64::ONE)
    );
}

#[test]
fn arithmetic_abort_is_an_error_not_a_side() {
    let mut buffer = [0u8; CODE];
    let mut encoder = Encoder::new(&mut buffer);
    encoder
        .push_input(0)
        .and_then(|e| e.push_input(1))
        .and_then(|e| e.push_input(2))
        .and_then(|e| e.median(3))
        .and_then(|e| e.push_const(Q64::ZERO))
        .and_then(|e| e.op(Op::Div))
        .and_then(|e| e.push_const(Q64::ZERO))
        .and_then(|e| e.op(Op::GreaterThan))
        .and_then(|e| e.bool_to_score())
        .expect("encoding fits");
    let code = encoder.code();

    // Division by zero must surface as an abort. The market voids; it does not
    // silently resolve to NO.
    assert_eq!(
        run(code, &[price(1), price(2), price(3)]),
        Err(VmError::Math(MathError::DivisionByZero))
    );
}

#[test]
fn parity_market_is_expressible_even_though_v1_does_not_list_it() {
    // "the whole-dollar price is odd": floor(median) mod 2 == 1. Here to prove
    // the machine is general, not because such a market is a good idea.
    let mut buffer = [0u8; CODE];
    let mut encoder = Encoder::new(&mut buffer);
    encoder
        .push_input(0)
        .and_then(|e| e.push_input(1))
        .and_then(|e| e.push_input(2))
        .and_then(|e| e.median(3))
        .and_then(|e| e.push_const(Q64::from_int(2)))
        .and_then(|e| e.op(Op::Modulo))
        .and_then(|e| e.push_const(Q64::ONE))
        .and_then(|e| e.op(Op::GreaterThanOrEqual))
        .and_then(|e| e.bool_to_score())
        .expect("encoding fits");
    let code = encoder.code();

    assert_eq!(run(code, &[price(7), price(7), price(9)]), Ok(Q64::ONE));
    assert_eq!(run(code, &[price(8), price(8), price(8)]), Ok(Q64::ZERO));
}

#[test]
fn select_picks_the_branch_without_evaluating_control_flow() {
    let mut buffer = [0u8; CODE];
    let mut encoder = Encoder::new(&mut buffer);
    // (input0 > 50 ? input1 : input2) > 10
    encoder
        .push_input(0)
        .and_then(|e| e.push_const(price(50)))
        .and_then(|e| e.op(Op::GreaterThan))
        .and_then(|e| e.push_input(1))
        .and_then(|e| e.push_input(2))
        .and_then(|e| e.op(Op::Select))
        .and_then(|e| e.push_const(price(10)))
        .and_then(|e| e.op(Op::GreaterThan))
        .and_then(|e| e.bool_to_score())
        .expect("encoding fits");
    let code = encoder.code();

    assert_eq!(run(code, &[price(60), price(20), price(5)]), Ok(Q64::ONE));
    assert_eq!(run(code, &[price(40), price(20), price(5)]), Ok(Q64::ZERO));
}

#[test]
fn hashing_matches_reference_implementations() {
    let mut buffer = [0u8; CODE];
    let mut encoder = Encoder::new(&mut buffer);
    let target: [u8; 32] = sha2::Sha256::digest(exec_test_support::NUM_ONE_BYTES).into();
    encoder
        .push_input(0)
        .and_then(|e| e.push_input(1))
        .and_then(|e| e.push_input(2))
        .and_then(|e| e.median(3))
        .and_then(|e| e.op(Op::NumToBytes))
        .and_then(|e| e.op(Op::Sha256))
        .and_then(|e| e.push_bytes32(&target))
        .and_then(|e| e.op(Op::BytesEqual))
        .and_then(|e| e.bool_to_score())
        .expect("encoding fits");
    let code = encoder.code();

    assert_eq!(run(code, &[Q64::ONE, Q64::ONE, Q64::ONE]), Ok(Q64::ONE));
    assert_eq!(run(code, &[price(2), price(2), price(2)]), Ok(Q64::ZERO));
}

/// The settlement ramp from red-team finding A7, written in the instruction
/// set rather than hard-coded in the resolver.
///
/// `share_yes = clamp((price - (K - d)) / 2d, 0, 1)` with strike `K = 100` and
/// band `d = 1`. Outside the band it is indistinguishable from a binary market;
/// inside it, a manipulator's gain is linear in how far they push the price,
/// while the cost of pushing it is convex. That is what makes near-the-money
/// manipulation negative expected value instead of nearly free.
fn ramp_around_100() -> ([u8; CODE], usize) {
    let mut buffer = [0u8; CODE];
    let mut encoder = Encoder::new(&mut buffer);
    encoder
        .push_input(0)
        .and_then(|e| e.push_input(1))
        .and_then(|e| e.push_input(2))
        .and_then(|e| e.median(3))
        .and_then(|e| e.push_const(price(99))) // K - d
        .and_then(|e| e.op(Op::Sub))
        .and_then(|e| e.push_const(price(2))) // 2d
        .and_then(|e| e.op(Op::Div))
        .and_then(|e| e.push_const(Q64::ZERO))
        .and_then(|e| e.push_const(Q64::ONE))
        .and_then(|e| e.op(Op::Clamp))
        .expect("encoding fits");
    let len = encoder.len();
    (buffer, len)
}

#[test]
fn ramp_pays_out_continuously_across_the_strike() {
    let (buffer, len) = ramp_around_100();
    let code = &buffer[..len];
    assert!(verify(code, 3).is_ok());

    let half = Q64::from_raw(1i128 << 63);
    let cases = [
        (price(50), Q64::ZERO), // far below: behaves exactly like binary NO
        (price(99), Q64::ZERO), // band edge
        (price(100), half),     // at the strike: pot splits evenly
        (price(101), Q64::ONE), // band edge
        (price(500), Q64::ONE), // far above: behaves exactly like binary YES
    ];
    for (observed, expected) in cases {
        assert_eq!(
            run(code, &[observed, observed, observed]),
            Ok(expected),
            "price {observed:?}"
        );
    }
}

#[test]
fn ramp_share_is_monotonic_and_never_leaves_the_unit_interval() {
    let (buffer, len) = ramp_around_100();
    let code = &buffer[..len];

    let mut previous = Q64::ZERO;
    // Walk the band in tenths, plus far outside it in both directions.
    for tenths in -20i64..=220 {
        let observed = Q64::from_int(985 + tenths)
            .div(Q64::from_int(10))
            .expect("no division by zero");
        let share = run(code, &[observed, observed, observed]).expect("no abort");
        assert!(share >= Q64::ZERO && share <= Q64::ONE, "share {share:?}");
        assert!(share >= previous, "not monotonic at {observed:?}");
        previous = share;
    }
    assert_eq!(previous, Q64::ONE);
}

mod verification {
    use super::*;

    fn verify_code(code: &[u8], inputs: usize) -> Result<(), VmError> {
        verify(code, inputs).map(|_| ())
    }

    #[test]
    fn accepts_the_canonical_program() {
        let (buffer, len) = above_strike(price(100));
        let verified = verify(&buffer[..len], 3).expect("verifies");
        assert_eq!(verified.input_count, 3);
        assert_eq!(verified.ops, 9);
        assert_eq!(verified.max_stack_depth, 3);
    }

    #[test]
    fn rejects_an_unread_input() {
        // A fourth feed is declared but the program only reads three: it would
        // appear in the market's source list while influencing nothing.
        let (buffer, len) = above_strike(price(100));
        assert_eq!(verify_code(&buffer[..len], 4), Err(VmError::UnusedInput(3)));
    }

    #[test]
    fn rejects_too_few_or_too_many_inputs() {
        // A program that reads nothing is not a predicate, and more than the
        // machine's ceiling cannot be addressed by a one-byte operand.
        let (buffer, len) = above_strike(price(100));
        assert_eq!(
            verify_code(&buffer[..len], 0),
            Err(VmError::InputCountOutOfRange)
        );
        assert_eq!(
            verify_code(&buffer[..len], crate::MAX_INPUTS + 1),
            Err(VmError::InputCountOutOfRange)
        );
        // Declaring fewer than the program reads is a different fault, and is
        // reported as one.
        assert_eq!(
            verify_code(&buffer[..len], 2),
            Err(VmError::InvalidInputIndex)
        );
    }

    #[test]
    fn rejects_an_out_of_range_input_index() {
        let mut buffer = [0u8; CODE];
        let mut encoder = Encoder::new(&mut buffer);
        encoder.push_input(7).expect("encoding fits");
        assert_eq!(
            verify_code(encoder.code(), 3),
            Err(VmError::InvalidInputIndex)
        );
    }

    #[test]
    fn accepts_a_bare_measurement() {
        // A score is what a predicate is for: the median itself, with no
        // verdict attached.
        let mut buffer = [0u8; CODE];
        let mut encoder = Encoder::new(&mut buffer);
        encoder
            .push_input(0)
            .and_then(|e| e.push_input(1))
            .and_then(|e| e.push_input(2))
            .and_then(|e| e.median(3))
            .expect("encoding fits");
        assert!(verify(encoder.code(), 3).is_ok());
    }

    #[test]
    fn rejects_a_bare_verdict() {
        // A comparison alone scores 0 or 1, and any ordinary strike sits far
        // above both, so such a market would always pay the same side. The
        // author has to convert deliberately and choose a strike to match.
        let mut buffer = [0u8; CODE];
        let mut encoder = Encoder::new(&mut buffer);
        encoder
            .push_input(0)
            .and_then(|e| e.push_input(1))
            .and_then(|e| e.push_input(2))
            .and_then(|e| e.median(3))
            .and_then(|e| e.push_const(price(100)))
            .and_then(|e| e.op(Op::GreaterThan))
            .expect("encoding fits");
        assert_eq!(verify_code(encoder.code(), 3), Err(VmError::ResultNotScore));

        // ...and with the conversion it verifies.
        let mut buffer = [0u8; CODE];
        let mut encoder = Encoder::new(&mut buffer);
        encoder
            .push_input(0)
            .and_then(|e| e.push_input(1))
            .and_then(|e| e.push_input(2))
            .and_then(|e| e.median(3))
            .and_then(|e| e.push_const(price(100)))
            .and_then(|e| e.op(Op::GreaterThan))
            .and_then(|e| e.bool_to_score())
            .expect("encoding fits");
        assert!(verify(encoder.code(), 3).is_ok());
    }

    #[test]
    fn rejects_a_bytes_result() {
        let mut buffer = [0u8; CODE];
        let mut encoder = Encoder::new(&mut buffer);
        encoder
            .push_input(0)
            .and_then(|e| e.push_input(1))
            .and_then(|e| e.push_input(2))
            .and_then(|e| e.median(3))
            .and_then(|e| e.op(Op::NumToBytes))
            .expect("encoding fits");
        assert_eq!(verify_code(encoder.code(), 3), Err(VmError::ResultNotScore));
    }

    #[test]
    fn rejects_leftover_stack() {
        let mut buffer = [0u8; CODE];
        let mut encoder = Encoder::new(&mut buffer);
        encoder
            .push_input(0)
            .and_then(|e| e.push_input(1))
            .and_then(|e| e.push_input(2))
            .and_then(|e| e.op(Op::GreaterThan))
            .expect("encoding fits");
        assert_eq!(
            verify_code(encoder.code(), 3),
            Err(VmError::ResultNotSingleton)
        );
    }

    #[test]
    fn rejects_type_confusion() {
        let mut buffer = [0u8; CODE];
        let mut encoder = Encoder::new(&mut buffer);
        // Comparing two numbers yields a Bool; adding Bools is not allowed.
        encoder
            .push_input(0)
            .and_then(|e| e.push_input(1))
            .and_then(|e| e.op(Op::GreaterThan))
            .and_then(|e| e.push_input(2))
            .and_then(|e| e.op(Op::Add))
            .expect("encoding fits");
        assert_eq!(verify_code(encoder.code(), 3), Err(VmError::TypeMismatch));
    }

    #[test]
    fn rejects_select_with_mismatched_branches() {
        let mut buffer = [0u8; CODE];
        let mut encoder = Encoder::new(&mut buffer);
        let zeros = [0u8; 32];
        encoder
            .push_input(0)
            .and_then(|e| e.push_input(1))
            .and_then(|e| e.op(Op::GreaterThan))
            .and_then(|e| e.push_input(2))
            .and_then(|e| e.push_bytes32(&zeros))
            .and_then(|e| e.op(Op::Select))
            .expect("encoding fits");
        assert_eq!(verify_code(encoder.code(), 3), Err(VmError::TypeMismatch));
    }

    #[test]
    fn rejects_stack_underflow() {
        assert_eq!(
            verify_code(&[Op::Add.to_byte()], 3),
            Err(VmError::StackUnderflow)
        );
    }

    #[test]
    fn rejects_unknown_and_truncated_instructions() {
        assert_eq!(verify_code(&[0xEE], 3), Err(VmError::UnknownOpcode(0xEE)));
        assert_eq!(
            verify_code(&[Op::PushInput.to_byte()], 3),
            Err(VmError::TruncatedOperand)
        );
        assert_eq!(
            verify_code(&[Op::PushConst.to_byte(), 0x00], 3),
            Err(VmError::TruncatedOperand)
        );
    }

    #[test]
    fn rejects_zero_arity_aggregation() {
        assert_eq!(
            verify_code(&[Op::Median.to_byte(), 0], 3),
            Err(VmError::InvalidAggregateArity)
        );
    }

    #[test]
    fn rejects_programs_that_are_too_long() {
        let too_long = [Op::PushTime.to_byte(); crate::MAX_CODE_LEN + 1];
        assert_eq!(verify_code(&too_long, 3), Err(VmError::CodeTooLong));

        // Repeating a stack-neutral opcode isolates the instruction-count
        // limit; repeating a pushing one would trip StackOverflow first.
        let mut too_many = [Op::Abs.to_byte(); MAX_OPS + 2];
        too_many[0] = Op::PushTime.to_byte();
        assert_eq!(verify_code(&too_many, 3), Err(VmError::TooManyOps));
    }

    #[test]
    fn rejects_stack_overflow() {
        let deep = [Op::PushTime.to_byte(); crate::MAX_STACK + 1];
        assert_eq!(verify_code(&deep, 3), Err(VmError::StackOverflow));
    }
}

proptest! {
    /// Verification is total: any byte string is either accepted or rejected,
    /// and never panics.
    #[test]
    fn verify_never_panics(code in proptest::collection::vec(any::<u8>(), 0..300)) {
        let _ = verify(&code, 3);
    }

    /// The safety claim the whole design rests on: if a program verifies, the
    /// interpreter runs it to a definite answer or a definite abort. It never
    /// panics, and it never overflows the stack it was proved not to.
    #[test]
    fn verified_programs_always_terminate_cleanly(
        code in proptest::collection::vec(any::<u8>(), 0..300),
        a in any::<i128>(),
        b in any::<i128>(),
        c in any::<i128>(),
    ) {
        if let Ok(program) = verify(&code, 3) {
            let inputs = [Q64::from_raw(a), Q64::from_raw(b), Q64::from_raw(c)];
            let outcome = program.execute(&context(&inputs));
            prop_assert!(
                !matches!(outcome, Err(VmError::StackOverflow | VmError::StepLimitExceeded)),
                "verification should have ruled this out: {outcome:?}"
            );
        }
    }

    /// Encoding then decoding a constant is lossless.
    #[test]
    fn constants_round_trip(raw in any::<i128>()) {
        let mut buffer = [0u8; CODE];
        let mut encoder = Encoder::new(&mut buffer);
        encoder
            .push_input(0)
            .and_then(|e| e.push_input(1))
            .and_then(|e| e.push_input(2))
            .and_then(|e| e.median(3))
            .and_then(|e| e.push_const(Q64::from_raw(raw)))
            .and_then(|e| e.op(Op::Equal))
            .and_then(|e| e.bool_to_score())
            .expect("encoding fits");
        let code = encoder.code();
        let value = Q64::from_raw(raw);
        prop_assert_eq!(run(code, &[value, value, value]), Ok(Q64::ONE));
    }
}

/// Values that tests need to share with the encoder.
mod exec_test_support {
    /// The 32-byte encoding of `Q64::ONE`, as `NumToBytes` produces it.
    pub const NUM_ONE_BYTES: &[u8; 32] = &{
        let mut out = [0u8; 32];
        let raw = (1i128 << 64).to_be_bytes();
        let mut i = 0;
        while i < 16 {
            out[16 + i] = raw[i];
            i += 1;
        }
        out
    };
}
