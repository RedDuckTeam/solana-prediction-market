//! One test per opcode, checking the exact value it produces.
//!
//! These exist because an audit demonstrated their absence: seven opcodes were
//! deliberately broken -- `WITHIN` made inclusive at its upper end, `MEAN` left
//! unaveraged, `ABS` turned into `NEGATE`, `AND` swapped with `OR`, `MIN` with
//! `MAX`, `HASH256` pointed at Keccak, `PUSH_TIME` replaced by zero -- and the
//! entire suite still passed. Coverage that a mutation survives is not coverage.
//!
//! Every case below is written to fail against a specific plausible mistake,
//! not merely to exercise the instruction.

use market_math::{MathError, Q64};
use sha2::Digest;

use super::{context, TestHasher, CODE};
use crate::{verify, Encoder, HostHasher, Op, VmError};

/// Builds a program over `inputs.len()` declared feeds and evaluates it.
fn eval(
    inputs: &[Q64],
    build: impl FnOnce(&mut Encoder) -> Result<(), VmError>,
) -> Result<Q64, VmError> {
    let mut buffer = [0u8; CODE];
    let mut encoder = Encoder::new(&mut buffer);
    build(&mut encoder).expect("encoding fits");
    verify(encoder.code(), inputs.len())?.execute(&context(inputs))
}

fn n(value: i64) -> Q64 {
    Q64::from_int(value)
}

/// Applies a binary operator to two declared inputs.
fn binary(op: Op, lhs: i64, rhs: i64) -> Result<Q64, VmError> {
    eval(&[n(lhs), n(rhs)], |e| {
        e.push_input(0)?;
        e.push_input(1)?;
        e.op(op)?;
        Ok(())
    })
}

/// Applies a comparison to two declared inputs and scores it one or zero.
fn compare(op: Op, lhs: i64, rhs: i64) -> bool {
    let score = eval(&[n(lhs), n(rhs)], |e| {
        e.push_input(0)?;
        e.push_input(1)?;
        e.op(op)?;
        e.bool_to_score()?;
        Ok(())
    })
    .expect("comparisons cannot abort");
    score == Q64::ONE
}

/// Applies a logical operator to two booleans, encoded as comparisons.
fn logical(op: Op, lhs: bool, rhs: bool) -> bool {
    let truth = |value: bool| if value { 1i64 } else { 0 };
    let score = eval(&[n(truth(lhs)), n(truth(rhs))], |e| {
        // `input > 0` turns each input into a boolean without a second opcode.
        e.push_input(0)?;
        e.push_const(Q64::ZERO)?;
        e.op(Op::GreaterThan)?;
        e.push_input(1)?;
        e.push_const(Q64::ZERO)?;
        e.op(Op::GreaterThan)?;
        e.op(op)?;
        e.bool_to_score()?;
        Ok(())
    })
    .expect("logic cannot abort");
    score == Q64::ONE
}

mod arithmetic {
    use super::*;

    #[test]
    fn add_and_sub() {
        assert_eq!(binary(Op::Add, 7, 5), Ok(n(12)));
        assert_eq!(binary(Op::Add, 7, -5), Ok(n(2)));
        // Operand order matters: `lhs - rhs`, not the other way round.
        assert_eq!(binary(Op::Sub, 7, 5), Ok(n(2)));
        assert_eq!(binary(Op::Sub, 5, 7), Ok(n(-2)));
    }

    #[test]
    fn mul_and_div() {
        assert_eq!(binary(Op::Mul, 7, 5), Ok(n(35)));
        assert_eq!(binary(Op::Mul, -7, 5), Ok(n(-35)));
        assert_eq!(binary(Op::Div, 35, 5), Ok(n(7)));
        // Order again, and the sign.
        assert_eq!(
            binary(Op::Div, 5, 35),
            Ok(Q64::from_raw(2_635_249_153_387_078_802))
        );
        assert_eq!(binary(Op::Div, -35, 5), Ok(n(-7)));
        assert_eq!(
            binary(Op::Div, 1, 0),
            Err(VmError::Math(MathError::DivisionByZero))
        );
    }

    #[test]
    fn modulo_carries_the_sign_of_the_divisor() {
        assert_eq!(binary(Op::Modulo, 7, 3), Ok(n(1)));
        assert_eq!(binary(Op::Modulo, -7, 3), Ok(n(2)));
        assert_eq!(binary(Op::Modulo, 7, -3), Ok(n(-2)));
        assert_eq!(
            binary(Op::Modulo, 1, 0),
            Err(VmError::Math(MathError::DivisionByZero))
        );
    }

    #[test]
    fn min_and_max_are_not_each_other() {
        assert_eq!(binary(Op::Min, 3, 7), Ok(n(3)));
        assert_eq!(binary(Op::Max, 3, 7), Ok(n(7)));
        // Reversed operands must give the same answers, or one of them is
        // secretly "pick the left".
        assert_eq!(binary(Op::Min, 7, 3), Ok(n(3)));
        assert_eq!(binary(Op::Max, 7, 3), Ok(n(7)));
        assert_eq!(binary(Op::Min, -7, 3), Ok(n(-7)));
        assert_eq!(binary(Op::Max, -7, 3), Ok(n(3)));
    }

    #[test]
    fn abs_is_not_negate() {
        let unary = |op: Op, value: i64| {
            eval(&[n(value)], |e| {
                e.push_input(0)?;
                e.op(op)?;
                Ok(())
            })
        };
        // The pair that separates them: on a positive input, abs keeps it and
        // negate flips it.
        assert_eq!(unary(Op::Abs, 5), Ok(n(5)));
        assert_eq!(unary(Op::Negate, 5), Ok(n(-5)));
        assert_eq!(unary(Op::Abs, -5), Ok(n(5)));
        assert_eq!(unary(Op::Negate, -5), Ok(n(5)));

        assert_eq!(unary(Op::Abs, 0), Ok(Q64::ZERO));
        assert_eq!(
            eval(&[Q64::MIN], |e| {
                e.push_input(0)?;
                e.op(Op::Abs)?;
                Ok(())
            }),
            Err(VmError::Math(MathError::Overflow)),
            "the most negative value has no positive counterpart"
        );
    }
}

mod comparison {
    use super::*;

    #[test]
    fn strict_and_non_strict_differ_exactly_at_equality() {
        // The equal case is the only one that tells `<` from `<=`.
        assert!(!compare(Op::LessThan, 5, 5));
        assert!(compare(Op::LessThanOrEqual, 5, 5));
        assert!(!compare(Op::GreaterThan, 5, 5));
        assert!(compare(Op::GreaterThanOrEqual, 5, 5));

        assert!(compare(Op::LessThan, 4, 5));
        assert!(!compare(Op::LessThan, 6, 5));
        assert!(compare(Op::GreaterThan, 6, 5));
        assert!(!compare(Op::GreaterThan, 4, 5));
    }

    #[test]
    fn equality_and_its_negation() {
        assert!(compare(Op::Equal, 5, 5));
        assert!(!compare(Op::Equal, 5, 6));
        assert!(!compare(Op::NotEqual, 5, 5));
        assert!(compare(Op::NotEqual, 5, 6));
        assert!(compare(Op::Equal, -5, -5));
    }

    #[test]
    fn within_is_half_open() {
        let within = |value: i64, low: i64, high: i64| {
            eval(&[n(value), n(low), n(high)], |e| {
                e.push_input(0)?;
                e.push_input(1)?;
                e.push_input(2)?;
                e.op(Op::Within)?;
                e.bool_to_score()?;
                Ok(())
            })
            .expect("within cannot abort")
                == Q64::ONE
        };
        assert!(within(5, 1, 10));
        // The two boundaries are the whole test: inclusive below, exclusive
        // above, as in Bitcoin Script.
        assert!(within(1, 1, 10), "the lower bound is included");
        assert!(!within(10, 1, 10), "the upper bound is excluded");
        assert!(!within(0, 1, 10));
        assert!(!within(11, 1, 10));
        assert!(!within(5, 10, 1), "an inverted range contains nothing");
    }
}

mod logic {
    use super::*;

    #[test]
    fn and_or_xor_have_distinct_truth_tables() {
        // The (true, false) row is what separates AND from OR; the (true, true)
        // row is what separates either from XOR.
        assert_eq!(
            [
                logical(Op::And, false, false),
                logical(Op::And, false, true),
                logical(Op::And, true, false),
                logical(Op::And, true, true)
            ],
            [false, false, false, true]
        );
        assert_eq!(
            [
                logical(Op::Or, false, false),
                logical(Op::Or, false, true),
                logical(Op::Or, true, false),
                logical(Op::Or, true, true)
            ],
            [false, true, true, true]
        );
        assert_eq!(
            [
                logical(Op::Xor, false, false),
                logical(Op::Xor, false, true),
                logical(Op::Xor, true, false),
                logical(Op::Xor, true, true)
            ],
            [false, true, true, false]
        );
    }

    #[test]
    fn not_inverts() {
        let negated = |value: i64| {
            eval(&[n(value)], |e| {
                e.push_input(0)?;
                e.push_const(Q64::ZERO)?;
                e.op(Op::GreaterThan)?;
                e.op(Op::Not)?;
                e.bool_to_score()?;
                Ok(())
            })
            .expect("not cannot abort")
                == Q64::ONE
        };
        assert!(negated(0));
        assert!(!negated(1));
    }
}

mod bytes {
    use super::*;

    /// Hashes the 32-byte encoding of a number with `op` and compares it to
    /// `expected`, scoring one when they match.
    fn digest_matches(op: Op, value: i64, expected: [u8; 32]) -> bool {
        eval(&[n(value)], |e| {
            e.push_input(0)?;
            e.op(Op::NumToBytes)?;
            e.op(op)?;
            e.push_bytes32(&expected)?;
            e.op(Op::BytesEqual)?;
            e.bool_to_score()?;
            Ok(())
        })
        .expect("hashing cannot abort")
            == Q64::ONE
    }

    fn num_bytes(value: i64) -> [u8; 32] {
        let raw = Q64::from_int(value).raw();
        let mut out = [if raw < 0 { 0xff } else { 0x00 }; 32];
        out[16..].copy_from_slice(&raw.to_be_bytes());
        out
    }

    #[test]
    fn each_hash_is_the_algorithm_it_names() {
        let preimage = num_bytes(7);
        let sha: [u8; 32] = sha2::Sha256::digest(preimage).into();
        let double_sha: [u8; 32] = sha2::Sha256::digest(sha).into();
        let keccak = TestHasher.keccak256(&preimage);

        assert!(digest_matches(Op::Sha256, 7, sha));
        assert!(digest_matches(Op::Keccak256, 7, keccak));
        assert!(digest_matches(Op::Hash256, 7, double_sha));

        // The three must not be interchangeable. `HASH256` in particular is a
        // double SHA-256, not "some other hash".
        assert_ne!(sha, keccak);
        assert_ne!(sha, double_sha);
        assert!(!digest_matches(Op::Hash256, 7, sha), "HASH256 hashes twice");
        assert!(
            !digest_matches(Op::Hash256, 7, keccak),
            "HASH256 is not Keccak"
        );
        assert!(!digest_matches(Op::Sha256, 7, keccak));
    }

    #[test]
    fn num_to_bytes_is_big_endian_and_sign_extended() {
        // Positive and negative encodings must differ in the padding, or a
        // predicate could confuse a number with its complement.
        assert_eq!(num_bytes(1)[..16], [0u8; 16]);
        assert_eq!(num_bytes(-1)[..16], [0xffu8; 16]);
        assert!(digest_matches(
            Op::Sha256,
            -1,
            sha2::Sha256::digest(num_bytes(-1)).into()
        ));
    }

    #[test]
    fn bytes_equal_distinguishes_literals() {
        let matches = |left: [u8; 32], right: [u8; 32]| {
            eval(&[n(1)], |e| {
                // The declared input has to be read, so fold it in harmlessly.
                e.push_input(0)?;
                e.op(Op::NumToBytes)?;
                e.op(Op::Sha256)?;
                e.push_bytes32(&left)?;
                e.op(Op::BytesEqual)?;
                e.push_bytes32(&right)?;
                e.push_bytes32(&right)?;
                e.op(Op::BytesEqual)?;
                e.op(Op::And)?;
                e.bool_to_score()?;
                Ok(())
            })
            .expect("comparison cannot abort")
                == Q64::ONE
        };
        let hashed: [u8; 32] = sha2::Sha256::digest(num_bytes(1)).into();
        assert!(matches(hashed, [3u8; 32]));
        assert!(!matches([9u8; 32], [3u8; 32]));
    }
}

mod aggregation {
    use super::*;

    #[test]
    fn mean_divides_by_the_count() {
        // Sum is nine; the answer is three. Forgetting the division gives nine.
        let mean = eval(&[n(1), n(2), n(6)], |e| {
            e.push_input(0)?;
            e.push_input(1)?;
            e.push_input(2)?;
            e.mean(3)?;
            Ok(())
        });
        assert_eq!(mean, Ok(n(3)));

        // And it is a mean, not a median: an outlier must move it.
        let skewed = eval(&[n(1), n(2), n(300)], |e| {
            e.push_input(0)?;
            e.push_input(1)?;
            e.push_input(2)?;
            e.mean(3)?;
            Ok(())
        });
        assert_eq!(skewed, Ok(n(101)));
    }

    #[test]
    fn median_ignores_the_outlier_that_mean_follows() {
        let median = eval(&[n(1), n(2), n(300)], |e| {
            e.push_input(0)?;
            e.push_input(1)?;
            e.push_input(2)?;
            e.median(3)?;
            Ok(())
        });
        assert_eq!(
            median,
            Ok(n(2)),
            "the median is the middle, not the average"
        );
    }

    #[test]
    fn aggregation_leaves_the_rest_of_the_stack_alone() {
        // Aggregation happens in place on the operand stack, so a value pushed
        // before the aggregated window must survive it untouched.
        let result = eval(&[n(10), n(1), n(2), n(6)], |e| {
            e.push_input(0)?;
            e.push_input(1)?;
            e.push_input(2)?;
            e.push_input(3)?;
            e.mean(3)?;
            e.op(Op::Add)?;
            Ok(())
        });
        assert_eq!(result, Ok(n(13)));
    }
}

mod selection {
    use super::*;

    #[test]
    fn clamp_confines_and_documents_its_inverted_case() {
        let clamp = |value: i64, low: i64, high: i64| {
            eval(&[n(value), n(low), n(high)], |e| {
                e.push_input(0)?;
                e.push_input(1)?;
                e.push_input(2)?;
                e.op(Op::Clamp)?;
                Ok(())
            })
        };
        assert_eq!(clamp(5, 1, 10), Ok(n(5)));
        assert_eq!(clamp(0, 1, 10), Ok(n(1)));
        assert_eq!(clamp(11, 1, 10), Ok(n(10)));
        assert_eq!(clamp(1, 1, 10), Ok(n(1)));
        assert_eq!(clamp(10, 1, 10), Ok(n(10)));
        // Crossed bounds: the upper one is applied last and therefore wins.
        // Documented on the opcode; pinned here so it cannot drift silently.
        assert_eq!(clamp(7, 10, 5), Ok(n(5)));
    }

    #[test]
    fn select_takes_the_consequent_when_the_condition_holds() {
        let select = |condition: bool| {
            eval(&[n(if condition { 1 } else { 0 }), n(111), n(222)], |e| {
                e.push_input(0)?;
                e.push_const(Q64::ZERO)?;
                e.op(Op::GreaterThan)?;
                e.push_input(1)?;
                e.push_input(2)?;
                e.op(Op::Select)?;
                Ok(())
            })
        };
        assert_eq!(select(true), Ok(n(111)), "true takes the first alternative");
        assert_eq!(select(false), Ok(n(222)));
    }
}

mod producers {
    use super::*;

    #[test]
    fn push_time_reads_the_settlement_instant() {
        // A constant zero would pass any test that only checked the program
        // ran, so the value itself is asserted.
        let settle_at = context(&[n(1)]).settle_at;
        let time = eval(&[n(1)], |e| {
            e.push_input(0)?;
            e.push_time()?;
            e.op(Op::Add)?;
            e.push_input(0)?;
            e.op(Op::Sub)?;
            Ok(())
        });
        assert_eq!(time, Ok(Q64::from_int(settle_at)));
        assert_ne!(settle_at, 0, "the fixture must not make zero look right");
    }

    #[test]
    fn push_const_survives_the_round_trip_at_the_extremes() {
        for raw in [0i128, 1, -1, i128::MAX, i128::MIN, 1 << 64, -(1 << 64)] {
            let value = Q64::from_raw(raw);
            let echoed = eval(&[n(1)], |e| {
                e.push_input(0)?;
                e.push_const(value)?;
                e.op(Op::Max)?;
                e.push_input(0)?;
                e.op(Op::Max)?;
                Ok(())
            });
            let expected = value.max(n(1));
            assert_eq!(echoed, Ok(expected), "constant {raw}");
        }
    }

    #[test]
    fn push_input_reads_the_slot_it_names() {
        for index in 0..3u8 {
            let picked = eval(&[n(10), n(20), n(30)], |e| {
                e.push_input(index)?;
                // The other two must still be read for the program to verify.
                e.push_input((index + 1) % 3)?;
                e.push_input((index + 2) % 3)?;
                e.op(Op::Min)?;
                e.op(Op::Min)?;
                e.push_input(index)?;
                e.op(Op::Max)?;
                Ok(())
            });
            assert_eq!(picked, Ok(n(10 * (i64::from(index) + 1))), "input {index}");
        }
    }
}
