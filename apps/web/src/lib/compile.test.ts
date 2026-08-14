import { MIN_MARKET_FEEDS, Op } from '@prediction-market/sdk';
import { evaluatePredicate, initSync, verifyPredicate } from '@prediction-market/predicate-wasm';
import { readFileSync } from 'node:fs';
import { fileURLToPath, URL } from 'node:url';
import { beforeAll, describe, expect, it } from 'vitest';

import { compileGraph, type GraphEdge, type GraphNode } from '@/lib/compile';
import { OPCODES } from '@/lib/opcodes';
import { TEMPLATES } from '@/lib/templates';

/**
 * The editor's contract: what it draws, the chain accepts.
 *
 * These run the real verifier — the same crate `create_market` calls, compiled
 * to WebAssembly — against bytecode the editor produced. A market that fails
 * verification cannot be created, so the alternative to this test is a user
 * paying for a transaction to discover a wiring bug.
 */
beforeAll(() => {
  // In a browser the module is fetched; under Node it is handed over directly.
  const wasm = readFileSync(
    fileURLToPath(
      new URL('../../../../packages/predicate-wasm/pkg/market_wasm_bg.wasm', import.meta.url),
    ),
  );
  initSync({ module: wasm });
});

const verify = (bytecode: Uint8Array, inputs: number) => {
  const result = verifyPredicate(bytecode, inputs);
  const value = {
    ops: result.ops,
    inputCount: result.inputCount,
    maxStackDepth: result.maxStackDepth,
  };
  result.free();
  return value;
};

/** A decimal as a raw Q64.64 string, exact for dyadic fractions. */
const q = (value: number): string => (BigInt(value * 4) * (1n << 62n)).toString();

/** Runs a template's bytecode over prices, through the real interpreter. */
const settle = (templateId: string, sources: number, prices: number[]): bigint => {
  const template = TEMPLATES.find((entry) => entry.id === templateId)!;
  const graph = template.build(sources);
  const compiled = compileGraph(graph.nodes, graph.edges);
  if (!compiled.ok) throw new Error(compiled.error);
  return BigInt(evaluatePredicate(compiled.bytecode, prices.map(q), 0n));
};

describe('templates', () => {
  // The chain refuses a market with fewer than MIN_MARKET_FEEDS sources, and
  // refuses a predicate that does not read every one of them. A template that
  // verifies but cannot satisfy both is a template nobody can use — which is
  // exactly what the fixed two-input `converted` and `spread` once were.
  for (const template of TEMPLATES) {
    for (const sources of [MIN_MARKET_FEEDS, 5, 8]) {
      it(`${template.id} is usable with ${sources} sources`, () => {
        const graph = template.build(sources);
        const compiled = compileGraph(graph.nodes, graph.edges);
        expect(compiled.ok, 'ok' in compiled && !compiled.ok ? compiled.error : '').toBe(true);
        if (!compiled.ok) return;

        expect(
          compiled.inputsUsed.length,
          `${template.id} must read all ${sources} declared sources`,
        ).toBe(sources);
        const contiguous = compiled.inputsUsed.every((value, index) => value === index);
        expect(contiguous, `${template.id} skips a source index`).toBe(true);

        const summary = verify(compiled.bytecode, compiled.inputsUsed.length);
        expect(summary.inputCount).toBe(sources);
        expect(summary.ops).toBeGreaterThan(0);
      });
    }
  }
});

describe('templates mean what their descriptions say', () => {
  // Run through the same interpreter `resolve` uses — a template that verifies
  // but settles differently than its description promises is a mis-sold
  // market. The prices are dyadic fractions, so every value is exact in Q64.64
  // and the assertions can demand equality rather than tolerance.
  it('above: the median of every source', () => {
    expect(settle('above', 3, [100, 300, 200])).toBe(BigInt(q(200)));
    expect(settle('above', 5, [1, 2, 3.5, 4, 5])).toBe(BigInt(q(3.5)));
  });

  it('odd-even: exactly 1 for an odd whole unit, 0 for an even one', () => {
    // 201.25, 201.5, 201.75 — median 201.5, floor 201, odd.
    expect(settle('odd-even', 3, [201.25, 201.5, 201.75])).toBe(BigInt(q(1)));
    // Median 200.75: fractional prices must not leak into the parity.
    expect(settle('odd-even', 3, [200.25, 200.75, 200.75])).toBe(BigInt(q(0)));
  });

  it('converted: the median of the token legs times the quote leg', () => {
    // median(0.5, 0.75) = 0.625, times 100 SOL/USD = 62.5.
    expect(settle('converted', 3, [0.5, 0.75, 100])).toBe(BigInt(q(62.5)));
  });

  it('spread: the highest reading minus the lowest', () => {
    expect(settle('spread', 3, [100, 250, 130])).toBe(BigInt(q(150)));
    expect(settle('spread', 4, [7, 7, 7, 7])).toBe(BigInt(q(0)));
  });
});

describe('malformed graphs are refused before anyone signs', () => {
  it('an unwired input', () => {
    const nodes: GraphNode[] = [
      { id: 'a', op: Op.PushInput, inputIndex: 0 },
      { id: 'add', op: Op.Add },
    ];
    const edges: GraphEdge[] = [{ from: 'a', to: 'add', port: 0 }];
    const result = compileGraph(nodes, edges);
    expect(result.ok).toBe(false);
  });

  it('two results left over', () => {
    const nodes: GraphNode[] = [
      { id: 'a', op: Op.PushInput, inputIndex: 0 },
      { id: 'b', op: Op.PushInput, inputIndex: 1 },
    ];
    const result = compileGraph(nodes, []);
    expect(result.ok).toBe(false);
  });

  it('a cycle', () => {
    const nodes: GraphNode[] = [
      { id: 'x', op: Op.Abs },
      { id: 'y', op: Op.Negate },
    ];
    const edges: GraphEdge[] = [
      { from: 'x', to: 'y', port: 0 },
      { from: 'y', to: 'x', port: 0 },
    ];
    const result = compileGraph(nodes, edges);
    expect(result.ok).toBe(false);
  });

  it('an empty number block', () => {
    const nodes: GraphNode[] = [{ id: 'c', op: Op.PushConst, value: '' }];
    const result = compileGraph(nodes, []);
    expect(result.ok).toBe(false);
  });

  it('a bare verdict, which the chain rejects as a score', () => {
    // `input > 0` leaves a Bool. Every ordinary strike sits far above both 0
    // and 1, so such a market would always pay the same side.
    const nodes: GraphNode[] = [
      { id: 'a', op: Op.PushInput, inputIndex: 0 },
      { id: 'zero', op: Op.PushConst, value: '0' },
      { id: 'gt', op: Op.GreaterThan },
    ];
    const edges: GraphEdge[] = [
      { from: 'a', to: 'gt', port: 0 },
      { from: 'zero', to: 'gt', port: 1 },
    ];
    const compiled = compileGraph(nodes, edges);
    expect(compiled.ok).toBe(true);
    if (!compiled.ok) return;
    expect(() => verify(compiled.bytecode, 1)).toThrow(/number/i);
  });
});

describe('the block catalogue matches the verifier', () => {
  it('every block can be placed and verified in isolation', () => {
    // Each operator is wired to sources of the types it declares. If the
    // catalogue's arity or types disagreed with `verify.rs`, this would fail
    // for that block rather than silently at market creation.
    const producers: Record<string, GraphNode[]> = {
      num: [{ id: 'p', op: Op.PushInput, inputIndex: 0 }],
      bool: [
        { id: 'p0', op: Op.PushInput, inputIndex: 0 },
        { id: 'p1', op: Op.PushConst, value: '1' },
        { id: 'p', op: Op.GreaterThan },
      ],
      bytes: [
        { id: 'p0', op: Op.PushInput, inputIndex: 0 },
        { id: 'p', op: Op.NumToBytes },
      ],
    };
    const producerEdges: Record<string, GraphEdge[]> = {
      num: [],
      bool: [
        { from: 'p0', to: 'p', port: 0 },
        { from: 'p1', to: 'p', port: 1 },
      ],
      bytes: [{ from: 'p0', to: 'p', port: 0 }],
    };

    for (const spec of OPCODES) {
      if (spec.inputs.length === 0) continue;

      const nodes: GraphNode[] = [{ id: 'root', op: spec.op, arity: spec.inputs.length }];
      const edges: GraphEdge[] = [];
      spec.inputs.forEach((type, port) => {
        const suffix = `-${port}`;
        for (const node of producers[type]!) {
          nodes.push({ ...node, id: node.id + suffix });
        }
        for (const edge of producerEdges[type]!) {
          edges.push({ from: edge.from + suffix, to: edge.to + suffix, port: edge.port });
        }
        edges.push({ from: `p${suffix}`, to: 'root', port });
      });

      const compiled = compileGraph(nodes, edges);
      expect(compiled.ok, `${spec.label} failed to compile`).toBe(true);
      if (!compiled.ok) continue;

      // A Bool or Bytes result is legal bytecode but not a legal *market*, so
      // only the type check is asserted here.
      try {
        verify(compiled.bytecode, compiled.inputsUsed.length);
      } catch (error) {
        const message = (error as Error).message;
        expect(
          message,
          `${spec.label}: ${message}`,
        ).toMatch(/number|yes\/no|bytes/i);
      }
    }
  });
});
