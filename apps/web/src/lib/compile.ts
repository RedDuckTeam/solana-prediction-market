import { Op, PredicateBuilder, fromDecimalString } from '@prediction-market/sdk';

import { SPEC_BY_OP, type OpSpec } from '@/lib/opcodes';

/**
 * A drawn graph into bytecode. The machine is a stack, so a tree compiles by
 * walking depth-first and emitting each node after its children.
 */

export interface GraphNode {
  id: string;
  op: number;
  /** `PushConst` carries a decimal the user typed; `PushInput` a source index. */
  value?: string;
  inputIndex?: number;
  /** How many operands a `Median`/`Mean` takes. */
  arity?: number;
}

export interface GraphEdge {
  /** The node producing the value. */
  from: string;
  /** The node consuming it, and which of its ports. */
  to: string;
  port: number;
}

export type CompileResult =
  | { ok: true; bytecode: Uint8Array; inputsUsed: number[] }
  | { ok: false; error: string; nodeId?: string };

const arityOf = (node: GraphNode, spec: OpSpec): number =>
  spec.immediate === 'arity' ? (node.arity ?? spec.inputs.length) : spec.inputs.length;

/**
 * The market's score is the one node nothing consumes. Anything else left
 * unconsumed is unwired work, which the verifier rejects as a stray value.
 */
const findRoot = (nodes: GraphNode[], edges: GraphEdge[]): CompileResult | { root: GraphNode } => {
  const consumed = new Set(edges.map((edge) => edge.from));
  const roots = nodes.filter((node) => !consumed.has(node.id));

  if (nodes.length === 0) return { ok: false, error: 'Nothing has been placed yet.' };
  if (roots.length === 0) {
    return { ok: false, error: 'Every block feeds another, so nothing is the result.' };
  }
  if (roots.length > 1) {
    return {
      ok: false,
      error: `${roots.length} blocks are left over. Exactly one has to be the result.`,
      nodeId: roots[1]!.id,
    };
  }
  return { root: roots[0]! };
};

export const compileGraph = (nodes: GraphNode[], edges: GraphEdge[]): CompileResult => {
  const found = findRoot(nodes, edges);
  if ('ok' in found) return found;

  const byId = new Map(nodes.map((node) => [node.id, node]));
  const builder = new PredicateBuilder();
  const inputsUsed = new Set<number>();
  const visiting = new Set<string>();

  const emit = (id: string): CompileResult | null => {
    const node = byId.get(id);
    if (!node) return { ok: false, error: 'A connection points at a block that is gone.' };
    const spec = SPEC_BY_OP.get(node.op);
    if (!spec) return { ok: false, error: 'Unknown block.', nodeId: id };

    // A tree cannot contain itself. React Flow permits the edge, so it is
    // caught here rather than left to recurse forever.
    if (visiting.has(id)) {
      return { ok: false, error: 'These blocks feed each other in a loop.', nodeId: id };
    }
    visiting.add(id);

    const arity = arityOf(node, spec);
    for (let port = 0; port < arity; port += 1) {
      const edge = edges.find((candidate) => candidate.to === id && candidate.port === port);
      if (!edge) {
        return { ok: false, error: `“${spec.label}” has an input with nothing in it.`, nodeId: id };
      }
      const failure = emit(edge.from);
      if (failure) return failure;
    }
    visiting.delete(id);

    switch (spec.immediate) {
      case 'input-index': {
        const index = node.inputIndex ?? 0;
        inputsUsed.add(index);
        builder.pushInput(index);
        break;
      }
      case 'const': {
        const text = (node.value ?? '').trim();
        if (!/^-?\d+(\.\d+)?$/.test(text)) {
          return { ok: false, error: 'A number block is empty or not a number.', nodeId: id };
        }
        builder.pushConst(fromDecimalString(text));
        break;
      }
      case 'arity': {
        if (node.op === Op.Median) builder.median(arity);
        else builder.mean(arity);
        break;
      }
      default:
        builder.op(node.op as Op);
    }
    return null;
  };

  try {
    const failure = emit(found.root.id);
    if (failure) return failure;
  } catch (cause) {
    return { ok: false, error: cause instanceof Error ? cause.message : String(cause) };
  }

  return {
    ok: true,
    bytecode: builder.build(),
    inputsUsed: [...inputsUsed].sort((a, b) => a - b),
  };
};
