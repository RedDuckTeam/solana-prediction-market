//! The predicate verifier, compiled for the browser.
//!
//! The graph editor must reach the same verdict the chain will. A TypeScript
//! reimplementation could disagree, and the disagreement would surface as a
//! failed transaction the user had already paid for — so this is the real one.
//! Same crate, same rules, same errors.

use market_feeds::{raydium_twap, ClmmLimits, FeedError};
use market_math::Q64;
use market_vm::{
    verify, EvalContext, HostHasher, VmError, MAX_CODE_LEN, MAX_INPUTS, MAX_OPS, MAX_STACK,
    MIN_INPUTS,
};
use wasm_bindgen::prelude::*;

/// What a verified program costs, so the editor can show it before anyone signs.
#[wasm_bindgen]
pub struct Verified {
    ops: u16,
    max_stack_depth: u8,
    input_count: u8,
    bytes: usize,
}

#[wasm_bindgen]
impl Verified {
    #[wasm_bindgen(getter)]
    pub fn ops(&self) -> u16 {
        self.ops
    }
    #[wasm_bindgen(getter, js_name = maxStackDepth)]
    pub fn max_stack_depth(&self) -> u8 {
        self.max_stack_depth
    }
    #[wasm_bindgen(getter, js_name = inputCount)]
    pub fn input_count(&self) -> u8 {
        self.input_count
    }
    #[wasm_bindgen(getter)]
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

/// Verifies bytecode exactly as `create_market` will.
///
/// The error is a sentence rather than a code, because it is shown to whoever
/// is drawing the graph and they cannot look up a discriminant.
#[wasm_bindgen(js_name = verifyPredicate)]
pub fn verify_predicate(code: &[u8], input_count: usize) -> Result<Verified, JsError> {
    match verify(code, input_count) {
        Ok(program) => Ok(Verified {
            ops: program.ops,
            max_stack_depth: program.max_stack_depth,
            input_count: program.input_count,
            bytes: code.len(),
        }),
        Err(error) => Err(JsError::new(&describe(error))),
    }
}

/// The interpreter's hashing, matching the syscalls the chain build wires in.
struct WasmHasher;

impl HostHasher for WasmHasher {
    fn sha256(&self, data: &[u8]) -> [u8; 32] {
        use sha2::Digest;
        sha2::Sha256::digest(data).into()
    }

    fn keccak256(&self, data: &[u8]) -> [u8; 32] {
        use tiny_keccak::Hasher;
        let mut output = [0u8; 32];
        let mut keccak = tiny_keccak::Keccak::v256();
        keccak.update(data);
        keccak.finalize(&mut output);
        output
    }
}

/// Runs a predicate over hypothetical prices, exactly as `resolve` will.
///
/// This is what lets the editor — and the template tests behind it — show the
/// score a draft would settle to, instead of promising that a graph which
/// merely *verifies* also *means* what its description says. Inputs and the
/// result are raw Q64.64 values carried as strings, because a JavaScript
/// number cannot hold an i128.
#[wasm_bindgen(js_name = evaluatePredicate)]
pub fn evaluate_predicate(
    code: &[u8],
    inputs_raw: Vec<String>,
    settle_at: i64,
) -> Result<String, JsError> {
    let program = verify(code, inputs_raw.len()).map_err(|error| JsError::new(&describe(error)))?;

    let mut inputs = [Q64::ZERO; MAX_INPUTS];
    for (slot, raw) in inputs.iter_mut().zip(&inputs_raw) {
        let parsed: i128 = raw
            .parse()
            .map_err(|_| JsError::new("an input is not a raw Q64.64 integer"))?;
        *slot = Q64::from_raw(parsed);
    }

    let score = program
        .execute(&EvalContext {
            inputs: &inputs[..inputs_raw.len()],
            settle_at,
            hasher: &WasmHasher,
        })
        .map_err(|error| JsError::new(&describe(error)))?;
    Ok(score.raw().to_string())
}

/// The limits the editor should enforce before it even asks.
#[wasm_bindgen(js_name = predicateLimits)]
pub fn predicate_limits() -> JsValue {
    let limits = format!(
        r#"{{"maxCodeLen":{MAX_CODE_LEN},"maxOps":{MAX_OPS},"maxStack":{MAX_STACK},"minInputs":{MIN_INPUTS},"maxInputs":{MAX_INPUTS}}}"#
    );
    JsValue::from_str(&limits)
}

fn describe(error: VmError) -> String {
    match error {
        VmError::CodeTooLong => format!("the program is longer than {MAX_CODE_LEN} bytes"),
        VmError::TooManyOps => format!("more than {MAX_OPS} instructions"),
        VmError::UnknownOpcode(byte) => format!("byte 0x{byte:02x} is not an opcode"),
        VmError::TruncatedOperand => "an instruction is missing its operand".into(),
        VmError::StackUnderflow => "an instruction takes more values than are available".into(),
        VmError::StackOverflow => format!("more than {MAX_STACK} values held at once"),
        VmError::TypeMismatch => "an instruction was given the wrong type of value".into(),
        VmError::InvalidInputIndex => "a price input that the market does not declare".into(),
        VmError::InvalidAggregateArity => "a median or mean over the wrong number of values".into(),
        VmError::ResultNotSingleton => "the program must end with exactly one value left".into(),
        VmError::ResultNotScore => {
            "the result has to be a number. A yes/no verdict is 0 or 1, which every \
             ordinary strike sits above, so such a market would always pay the same side"
                .into()
        }
        VmError::UnusedInput(index) => format!(
            "price input {index} is declared but never read, so it would appear to \
             secure the market while influencing nothing"
        ),
        VmError::InputCountOutOfRange => {
            format!("a market declares between {MIN_INPUTS} and {MAX_INPUTS} price inputs")
        }
        VmError::InputCountMismatch => "the input count does not match the program".into(),
        VmError::Math(_) => "the arithmetic cannot be carried out".into(),
        VmError::StepLimitExceeded => "the program does not finish inside its budget".into(),
    }
}

/// What a settlement would see in a pool's ring right now.
///
/// The editor shows whether a window is answerable yet, and the only honest way
/// to say so is to ask the reader that will be asked at settlement. Anything
/// else is a second opinion that can differ from the one that decides money.
#[wasm_bindgen]
pub struct Coverage {
    ok: bool,
    reason: String,
    observations_inside: u16,
}

#[wasm_bindgen]
impl Coverage {
    #[wasm_bindgen(getter)]
    pub fn ok(&self) -> bool {
        self.ok
    }
    #[wasm_bindgen(getter)]
    pub fn reason(&self) -> String {
        self.reason.clone()
    }
    #[wasm_bindgen(getter, js_name = observationsInside)]
    pub fn observations_inside(&self) -> u16 {
        self.observations_inside
    }
}

/// Reads a Raydium observation ring exactly as `snapshot` will.
#[wasm_bindgen(js_name = windowCoverage)]
pub fn window_coverage(
    ring: &[u8],
    pool: &[u8],
    from: i64,
    to: i64,
    max_segment: u32,
    min_observations: u16,
) -> Coverage {
    let mut expected = [0u8; 32];
    if pool.len() != 32 {
        return Coverage {
            ok: false,
            reason: "the pool address is not thirty-two bytes".into(),
            observations_inside: 0,
        };
    }
    expected.copy_from_slice(pool);

    let limits = ClmmLimits {
        max_segment,
        min_observations,
    };
    match raydium_twap(ring, &expected, from, to, limits) {
        Ok(reading) => Coverage {
            ok: true,
            reason: String::new(),
            observations_inside: reading.observations_inside,
        },
        Err(error) => Coverage {
            ok: false,
            reason: describe_feed(error),
            observations_inside: 0,
        },
    }
}

fn describe_feed(error: FeedError) -> String {
    match error {
        FeedError::WindowNotCovered => {
            "the window is not bracketed by observations yet — the pool has to be \
             traded through once more, at or after the window closes"
                .into()
        }
        FeedError::SegmentTooLong => {
            "one stretch of the window has no observation in it. Trade through the \
             pool inside the window to break it up"
                .into()
        }
        FeedError::WindowTooQuiet => "too few observations inside the window".into(),
        FeedError::BufferNotFull => "the pool has not been traded through enough times".into(),
        FeedError::NotInitialized => "this pool has never recorded a price".into(),
        FeedError::PoolMismatch => "that ring belongs to a different pool".into(),
        FeedError::EmptyWindow => "the window has no length".into(),
        other => format!("the pool cannot be read: {other:?}"),
    }
}
