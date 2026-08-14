/**
 * Predicate bytecode, mirroring `crates/market-vm`. Opcode numbers are
 * permanent on-chain format — `spec_hash` commits to the exact bytes and
 * markets outlive releases, so never renumber. Verification happens on chain.
 */

export const Op = {
  PushInput: 0x01,
  PushConst: 0x02,
  PushBytes32: 0x03,
  PushTime: 0x04,

  Add: 0x10,
  Sub: 0x11,
  Mul: 0x12,
  Div: 0x13,
  Modulo: 0x14,
  Min: 0x15,
  Max: 0x16,
  Abs: 0x17,
  Negate: 0x18,

  Equal: 0x20,
  NotEqual: 0x21,
  LessThan: 0x22,
  GreaterThan: 0x23,
  LessThanOrEqual: 0x24,
  GreaterThanOrEqual: 0x25,
  Within: 0x26,

  And: 0x30,
  Or: 0x31,
  Xor: 0x32,
  Not: 0x33,

  Sha256: 0x40,
  Keccak256: 0x41,
  Hash256: 0x42,
  NumToBytes: 0x43,
  BytesEqual: 0x44,

  Select: 0x50,
  Median: 0x51,
  Mean: 0x52,
  Clamp: 0x53,
} as const;

export type Op = (typeof Op)[keyof typeof Op];

const i128ToLe = (value: bigint): Uint8Array => {
  const bytes = new Uint8Array(16);
  let remaining = value < 0n ? (1n << 128n) + value : value;
  for (let i = 0; i < 16; i += 1) {
    bytes[i] = Number(remaining & 0xffn);
    remaining >>= 8n;
  }
  return bytes;
};

export class PredicateBuilder {
  private readonly bytes: number[] = [];

  /** Pushes the resolved price of the declared source at `index`. */
  pushInput(index: number): this {
    this.bytes.push(Op.PushInput, index);
    return this;
  }

  /** Pushes a Q64.64 literal. */
  pushConst(raw: bigint): this {
    this.bytes.push(Op.PushConst, ...i128ToLe(raw));
    return this;
  }

  pushBytes32(value: Uint8Array): this {
    if (value.length !== 32) throw new Error('expected 32 bytes');
    this.bytes.push(Op.PushBytes32, ...value);
    return this;
  }

  /** An opcode that takes no immediate. */
  op(op: Op): this {
    if (op === Op.PushInput || op === Op.PushConst || op === Op.PushBytes32) {
      const name = Object.keys(Op).find((key) => Op[key as keyof typeof Op] === op);
      throw new Error(`${name ?? `0x${op.toString(16)}`} takes an operand`);
    }
    this.bytes.push(op);
    return this;
  }

  median(arity: number): this {
    this.bytes.push(Op.Median, arity);
    return this;
  }

  mean(arity: number): this {
    this.bytes.push(Op.Mean, arity);
    return this;
  }

  build(): Uint8Array {
    return Uint8Array.from(this.bytes);
  }
}

/**
 * The canonical market. A predicate produces a measurement; comparing it to a
 * strike is the protocol's job, so this stops at the median.
 */
export const medianOfSources = (count: number): Uint8Array => {
  const builder = new PredicateBuilder();
  for (let index = 0; index < count; index += 1) builder.pushInput(index);
  return builder.median(count).build();
};
