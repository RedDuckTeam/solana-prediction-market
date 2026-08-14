import { Op } from '@prediction-market/sdk';

import type { GraphEdge, GraphNode } from '@/lib/compile';

/**
 * Starting points, so nobody meets an empty canvas. A predicate ends at a
 * measurement: comparing it to the strike is the protocol's job, which is why
 * none of these ends in a greater-than.
 *
 * Every template must build a graph that reads *all* `sourceCount` inputs —
 * the chain refuses a market whose predicate ignores a declared source — and
 * must mean what its description says. Both are held to by tests that run the
 * compiled bytecode through the real interpreter, not by care.
 */
export interface Template {
  id: string;
  name: string;
  description: string;
  /** How to read the result against the strike, for the summary line. */
  reading: string;
  build: (sourceCount: number) => { nodes: GraphNode[]; edges: GraphEdge[] };
}

const inputs = (count: number): { nodes: GraphNode[] } => ({
  nodes: Array.from({ length: count }, (_, index) => ({
    id: `src-${index}`,
    op: Op.PushInput,
    inputIndex: index,
  })),
});

const median = (sourceCount: number): { nodes: GraphNode[]; edges: GraphEdge[] } => {
  const nodes: GraphNode[] = [
    { id: 'median', op: Op.Median, arity: sourceCount },
    ...inputs(sourceCount).nodes,
  ];
  const edges: GraphEdge[] = Array.from({ length: sourceCount }, (_, index) => ({
    from: `src-${index}`,
    to: 'median',
    port: index,
  }));
  return { nodes, edges };
};

/**
 * A chain of two-operand `op` nodes folding `sources` inputs left to right.
 * The machine's `MIN`/`MAX` take two values, so an n-way fold is n−1 of them.
 */
const fold = (
  op: number,
  prefix: string,
  sourceCount: number,
): { nodes: GraphNode[]; edges: GraphEdge[]; root: string } => {
  const nodes: GraphNode[] = [];
  const edges: GraphEdge[] = [];
  let previous = 'src-0';
  for (let index = 1; index < sourceCount; index += 1) {
    const id = `${prefix}-${index}`;
    nodes.push({ id, op });
    edges.push({ from: previous, to: id, port: 0 });
    edges.push({ from: `src-${index}`, to: id, port: 1 });
    previous = id;
  }
  return { nodes, edges, root: previous };
};

export const TEMPLATES: Template[] = [
  {
    id: 'above',
    name: 'Above a price',
    description:
      'The median of every source. YES pays when it settles above the strike — ' +
      'and betting NO on this same market is how a "below" position is taken, ' +
      'so there is no separate below template.',
    reading: 'Yes wins if the median price is above the strike.',
    build: median,
  },
  {
    id: 'odd-even',
    name: 'Odd or even',
    description:
      'The median, floored to a whole unit, modulo two: exactly 1 when the ' +
      'whole-dollar price is odd, exactly 0 when even. Set the strike to 0.5 ' +
      'so either answer lands cleanly on one side of it.',
    reading: 'Yes wins when the whole-dollar price is odd. Strike 0.5.',
    build: (sourceCount) => {
      const base = median(sourceCount);
      return {
        nodes: [
          ...base.nodes,
          // floor(x) = x − (x mod 1): the machine has no FLOOR opcode, and its
          // modulo is floored, so the fractional part subtracts out exactly.
          { id: 'one', op: Op.PushConst, value: '1' },
          { id: 'frac', op: Op.Modulo },
          { id: 'floor', op: Op.Sub },
          { id: 'two', op: Op.PushConst, value: '2' },
          { id: 'parity', op: Op.Modulo },
        ],
        edges: [
          ...base.edges,
          { from: 'median', to: 'frac', port: 0 },
          { from: 'one', to: 'frac', port: 1 },
          { from: 'median', to: 'floor', port: 0 },
          { from: 'frac', to: 'floor', port: 1 },
          { from: 'floor', to: 'parity', port: 0 },
          { from: 'two', to: 'parity', port: 1 },
        ],
      };
    },
  },
  {
    id: 'converted',
    name: 'Priced through SOL',
    description:
      'Token legs times a quote leg: the median of every source but the last, ' +
      'multiplied by the last. Choose token/SOL pools first — marked B/A where ' +
      'the pool quotes the pair the other way round — and a SOL/dollar source ' +
      'last. This is how a token settles in dollars when no pool for that pair ' +
      'exists.',
    reading: 'Yes wins if the converted price is above the strike.',
    build: (sourceCount) => {
      if (sourceCount < 2) return median(sourceCount);
      const legs = sourceCount - 1;
      const nodes: GraphNode[] = [...inputs(sourceCount).nodes, { id: 'mul', op: Op.Mul }];
      const edges: GraphEdge[] = [];
      let tokenSide = 'src-0';
      if (legs > 1) {
        nodes.push({ id: 'median', op: Op.Median, arity: legs });
        for (let index = 0; index < legs; index += 1) {
          edges.push({ from: `src-${index}`, to: 'median', port: index });
        }
        tokenSide = 'median';
      }
      edges.push({ from: tokenSide, to: 'mul', port: 0 });
      edges.push({ from: `src-${legs}`, to: 'mul', port: 1 });
      return { nodes, edges };
    },
  },
  {
    id: 'spread',
    name: 'Spread across the sources',
    description:
      'The widest disagreement among every source: the highest reading minus ' +
      'the lowest. A market on whether the venues stay in line with each other.',
    reading: 'Yes wins if the widest gap between any two sources exceeds the strike.',
    build: (sourceCount) => {
      const base = inputs(sourceCount);
      if (sourceCount < 2) return { nodes: base.nodes, edges: [] };
      const highest = fold(Op.Max, 'max', sourceCount);
      const lowest = fold(Op.Min, 'min', sourceCount);
      return {
        nodes: [...base.nodes, ...highest.nodes, ...lowest.nodes, { id: 'gap', op: Op.Sub }],
        edges: [
          ...highest.edges,
          ...lowest.edges,
          { from: highest.root, to: 'gap', port: 0 },
          { from: lowest.root, to: 'gap', port: 1 },
        ],
      };
    },
  },
];
