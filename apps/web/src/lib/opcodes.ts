import { Op } from '@prediction-market/sdk';

/**
 * The instruction set as the editor needs it, mirroring `step()` in
 * `crates/market-vm/src/verify.rs`. Duplicated because an editor must know what
 * may connect before a program exists to verify; the wasm verifier still has
 * the last word, so a mistake here is a rejected graph, not a stuck market.
 */
export type PortType = 'num' | 'bool' | 'bytes';

export interface OpSpec {
  op: number;
  label: string;
  hint: string;
  group: 'source' | 'arithmetic' | 'comparison' | 'logic' | 'hashing' | 'choice';
  /** Ports in the order a reader expects, top to bottom. */
  inputs: PortType[];
  /**
   * Order matters and is not guessable: `Subtract` is `a - b`, and wired the
   * other way round it is a different market that still verifies.
   */
  portNames?: string[];
  output: PortType;
  /** `arity` widens the input list; `const` carries a number in the bytecode. */
  immediate?: 'arity' | 'const' | 'input-index';
}

export const OPCODES: OpSpec[] = [
  // -- Sources -------------------------------------------------------
  {
    op: Op.PushInput,
    label: 'Token price',
    hint: 'One of the price sources this market declares, time-weighted over its window.',
    group: 'source',
    inputs: [],
    output: 'num',
    immediate: 'input-index',
  },
  {
    op: Op.PushConst,
    label: 'Number',
    hint: 'A fixed value written into the market and never changed.',
    group: 'source',
    inputs: [],
    output: 'num',
    immediate: 'const',
  },
  {
    op: Op.PushTime,
    label: 'Settlement time',
    hint: 'The instant this market settles at, as a number.',
    group: 'source',
    inputs: [],
    output: 'num',
  },

  // -- Arithmetic ----------------------------------------------------
  { op: Op.Add, label: 'Add', hint: 'a + b', group: 'arithmetic', inputs: ['num', 'num'], output: 'num' },
  { op: Op.Sub, label: 'Subtract', hint: 'a − b', group: 'arithmetic', inputs: ['num', 'num'], output: 'num', portNames: ['a', 'b'] },
  { op: Op.Mul, label: 'Multiply', hint: 'a × b', group: 'arithmetic', inputs: ['num', 'num'], output: 'num' },
  { op: Op.Div, label: 'Divide', hint: 'a ÷ b. Dividing by zero aborts, and the market voids.', group: 'arithmetic', inputs: ['num', 'num'], output: 'num', portNames: ['a', 'b'] },
  { op: Op.Modulo, label: 'Remainder', hint: 'a mod b — this is what makes an odd/even market.', group: 'arithmetic', inputs: ['num', 'num'], output: 'num', portNames: ['a', 'b'] },
  { op: Op.Min, label: 'Minimum', hint: 'the smaller of two', group: 'arithmetic', inputs: ['num', 'num'], output: 'num' },
  { op: Op.Max, label: 'Maximum', hint: 'the larger of two', group: 'arithmetic', inputs: ['num', 'num'], output: 'num' },
  { op: Op.Abs, label: 'Absolute', hint: 'distance from zero', group: 'arithmetic', inputs: ['num'], output: 'num' },
  { op: Op.Negate, label: 'Negate', hint: '−a', group: 'arithmetic', inputs: ['num'], output: 'num' },
  {
    op: Op.Median,
    label: 'Median',
    hint: 'The middle value. This is what makes one captured source unable to decide a market.',
    group: 'arithmetic',
    inputs: ['num', 'num', 'num'],
    output: 'num',
    immediate: 'arity',
  },
  {
    op: Op.Mean,
    label: 'Average',
    hint: 'The arithmetic mean. An outlier moves it, which a median resists.',
    group: 'arithmetic',
    inputs: ['num', 'num', 'num'],
    output: 'num',
    immediate: 'arity',
  },
  { op: Op.Clamp, label: 'Clamp', hint: 'x confined to [low, high]', group: 'arithmetic', inputs: ['num', 'num', 'num'], output: 'num', portNames: ['value', 'low', 'high'] },

  // -- Comparison ----------------------------------------------------
  { op: Op.GreaterThan, label: 'Greater than', hint: 'a > b', group: 'comparison', inputs: ['num', 'num'], output: 'bool', portNames: ['a', 'b'] },
  { op: Op.LessThan, label: 'Less than', hint: 'a < b', group: 'comparison', inputs: ['num', 'num'], output: 'bool', portNames: ['a', 'b'] },
  { op: Op.GreaterThanOrEqual, label: 'At least', hint: 'a ≥ b', group: 'comparison', inputs: ['num', 'num'], output: 'bool', portNames: ['a', 'b'] },
  { op: Op.LessThanOrEqual, label: 'At most', hint: 'a ≤ b', group: 'comparison', inputs: ['num', 'num'], output: 'bool', portNames: ['a', 'b'] },
  { op: Op.Equal, label: 'Equal', hint: 'a = b', group: 'comparison', inputs: ['num', 'num'], output: 'bool' },
  { op: Op.NotEqual, label: 'Not equal', hint: 'a ≠ b', group: 'comparison', inputs: ['num', 'num'], output: 'bool' },
  { op: Op.Within, label: 'Within', hint: 'low ≤ x < high. Inclusive below, exclusive above.', group: 'comparison', inputs: ['num', 'num', 'num'], output: 'bool', portNames: ['value', 'low', 'high'] },

  // -- Logic ---------------------------------------------------------
  { op: Op.And, label: 'And', hint: 'both', group: 'logic', inputs: ['bool', 'bool'], output: 'bool' },
  { op: Op.Or, label: 'Or', hint: 'either', group: 'logic', inputs: ['bool', 'bool'], output: 'bool' },
  { op: Op.Xor, label: 'Exclusive or', hint: 'one but not both', group: 'logic', inputs: ['bool', 'bool'], output: 'bool' },
  { op: Op.Not, label: 'Not', hint: 'the opposite', group: 'logic', inputs: ['bool'], output: 'bool' },

  // -- Hashing -------------------------------------------------------
  { op: Op.NumToBytes, label: 'To bytes', hint: 'a number as the 32 bytes a hash takes', group: 'hashing', inputs: ['num'], output: 'bytes' },
  { op: Op.Sha256, label: 'SHA-256', hint: 'one round', group: 'hashing', inputs: ['bytes'], output: 'bytes' },
  { op: Op.Hash256, label: 'HASH256', hint: 'SHA-256 applied twice, as in Bitcoin Script', group: 'hashing', inputs: ['bytes'], output: 'bytes' },
  { op: Op.Keccak256, label: 'Keccak-256', hint: 'the hash Ethereum uses', group: 'hashing', inputs: ['bytes'], output: 'bytes' },
  { op: Op.BytesEqual, label: 'Bytes equal', hint: 'two hashes match', group: 'hashing', inputs: ['bytes', 'bytes'], output: 'bool' },

  // -- Choice --------------------------------------------------------
  {
    op: Op.Select,
    label: 'Choose',
    hint: 'If the condition holds, take the first, else the second. Both are evaluated — there is no branching, which is what keeps the cost knowable before anyone bets.',
    group: 'choice',
    inputs: ['bool', 'num', 'num'],
    output: 'num',
    portNames: ['if', 'then', 'else'],
  },
];

export const SPEC_BY_OP = new Map(OPCODES.map((spec) => [spec.op, spec]));

export const GROUP_LABELS: Record<OpSpec['group'], string> = {
  source: 'Sources',
  arithmetic: 'Arithmetic',
  comparison: 'Comparison',
  logic: 'Logic',
  hashing: 'Hashing',
  choice: 'Choice',
};

export const PORT_COLOUR: Record<PortType, string> = {
  num: 'var(--port-num)',
  bool: 'var(--port-bool)',
  bytes: 'var(--port-bytes)',
};

export const PORT_LABEL: Record<PortType, string> = {
  num: 'number',
  bool: 'yes/no',
  bytes: 'bytes',
};
