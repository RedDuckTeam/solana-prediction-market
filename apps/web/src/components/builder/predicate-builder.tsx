import {
  Background,
  Controls,
  ReactFlow,
  ReactFlowProvider,
  addEdge,
  useEdgesState,
  useNodesState,
  useReactFlow,
  type Connection,
  type Edge,
  type Node,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { useCallback, useEffect, useMemo, useState } from 'react';

import { OpcodeNode, type OpcodeNodeData } from '@/components/builder/opcode-node';
import { Badge } from '@/components/ui/badge';
import { InfoHint } from '@/components/ui/info-hint';
import { Button } from '@/components/ui/button';
import { useVerifier, verify, type VerifyResult } from '@/hooks/use-verifier';
import { compileGraph, type GraphEdge, type GraphNode } from '@/lib/compile';
import { GROUP_LABELS, OPCODES, PORT_COLOUR, SPEC_BY_OP, type OpSpec } from '@/lib/opcodes';
import { TEMPLATES } from '@/lib/templates';
import { cn } from '@/lib/utils';

export interface BuiltPredicate {
  bytecode: Uint8Array;
  inputsUsed: number[];
  summary: VerifyResult;
}

const nodeTypes = { opcode: OpcodeNode };

let counter = 0;
const nextId = () => `n${(counter += 1)}`;

/**
 * Blocks are the instruction set, wires are operands. What comes out is the
 * bytecode the market carries, checked as it is drawn by the verifier the chain
 * runs — so "this will be accepted" is a fact here, not a hope.
 */
function Canvas({
  sources,
  onChange,
}: {
  sources: string[];
  onChange: (built: BuiltPredicate | null) => void;
}) {
  const loaded = useVerifier();
  const { screenToFlowPosition } = useReactFlow();
  const [nodes, setNodes, onNodesChange] = useNodesState<Node>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const [compileError, setCompileError] = useState<string | null>(null);
  const [summary, setSummary] = useState<VerifyResult | null>(null);

  const patch = useCallback(
    (id: string, values: Partial<OpcodeNodeData>) => {
      setNodes((current) =>
        current.map((node) =>
          node.id === id ? { ...node, data: { ...node.data, ...values } } : node,
        ),
      );
    },
    [setNodes],
  );

  const place = useCallback(
    (spec: OpSpec, at?: { x: number; y: number }) => {
      const id = nextId();
      setNodes((current) => [
        ...current,
        {
          id,
          type: 'opcode',
          position: at ?? { x: 120 + current.length * 40, y: 80 + current.length * 30 },
          data: {
            op: spec.op,
            sources,
            arity: spec.immediate === 'arity' ? spec.inputs.length : undefined,
            inputIndex: spec.immediate === 'input-index' ? 0 : undefined,
            value: spec.immediate === 'const' ? '' : undefined,
            onChange: (values: Partial<OpcodeNodeData>) => patch(id, values),
          } satisfies OpcodeNodeData,
        } as Node,
      ]);
    },
    [patch, setNodes, sources],
  );

  const loadTemplate = useCallback(
    (templateId: string) => {
      const template = TEMPLATES.find((entry) => entry.id === templateId);
      if (!template) return;
      const graph = template.build(Math.max(2, sources.length));

      const ids = new Map(graph.nodes.map((node) => [node.id, nextId()]));
      // Laid out by depth so a freshly loaded template is readable rather than
      // a pile: sources on the left, the result on the right.
      const depth = new Map<string, number>();
      const measure = (id: string): number => {
        if (depth.has(id)) return depth.get(id)!;
        const feeding = graph.edges.filter((edge) => edge.to === id);
        const value = feeding.length === 0 ? 0 : 1 + Math.max(...feeding.map((e) => measure(e.from)));
        depth.set(id, value);
        return value;
      };
      graph.nodes.forEach((node) => measure(node.id));
      const rows = new Map<number, number>();

      setNodes(
        graph.nodes.map((node) => {
          const id = ids.get(node.id)!;
          const column = depth.get(node.id) ?? 0;
          const row = rows.get(column) ?? 0;
          rows.set(column, row + 1);
          return {
            id,
            type: 'opcode',
            position: { x: 40 + column * 240, y: 40 + row * 150 },
            data: {
              op: node.op,
              sources,
              value: node.value,
              inputIndex: node.inputIndex,
              arity: node.arity,
              onChange: (values: Partial<OpcodeNodeData>) => patch(id, values),
            } satisfies OpcodeNodeData,
          } as Node;
        }),
      );
      setEdges(
        graph.edges.map((edge) => ({
          id: `${ids.get(edge.from)}-${ids.get(edge.to)}-${edge.port}`,
          source: ids.get(edge.from)!,
          target: ids.get(edge.to)!,
          targetHandle: `in-${edge.port}`,
          sourceHandle: 'out',
        })),
      );
    },
    [patch, setEdges, setNodes, sources],
  );

  /** Refuses a wire whose types do not meet, before it can be drawn. */
  const isValidConnection = useCallback(
    (connection: Connection | Edge) => {
      const from = nodes.find((node) => node.id === connection.source);
      const to = nodes.find((node) => node.id === connection.target);
      if (!from || !to) return false;
      const fromSpec = SPEC_BY_OP.get((from.data as OpcodeNodeData).op);
      const toSpec = SPEC_BY_OP.get((to.data as OpcodeNodeData).op);
      if (!fromSpec || !toSpec) return false;
      const port = Number((connection.targetHandle ?? 'in-0').replace('in-', ''));
      const expected =
        toSpec.immediate === 'arity' ? toSpec.inputs[0] : toSpec.inputs[port];
      return expected === fromSpec.output;
    },
    [nodes],
  );

  const onConnect = useCallback(
    (connection: Connection) => {
      setEdges((current) => {
        // One value per port: a second wire replaces the first rather than
        // silently sitting behind it.
        const cleared = current.filter(
          (edge) =>
            !(edge.target === connection.target && edge.targetHandle === connection.targetHandle),
        );
        return addEdge(connection, cleared);
      });
    },
    [setEdges],
  );

  const graph = useMemo(() => {
    const graphNodes: GraphNode[] = nodes.map((node) => {
      const data = node.data as OpcodeNodeData;
      return {
        id: node.id,
        op: data.op,
        value: data.value,
        inputIndex: data.inputIndex,
        arity: data.arity,
      };
    });
    const graphEdges: GraphEdge[] = edges.map((edge) => ({
      from: edge.source,
      to: edge.target,
      port: Number((edge.targetHandle ?? 'in-0').replace('in-', '')),
    }));
    return { graphNodes, graphEdges };
  }, [nodes, edges]);

  useEffect(() => {
    if (!loaded) return;
    if (graph.graphNodes.length === 0) {
      setCompileError(null);
      setSummary(null);
      onChange(null);
      return;
    }

    const compiled = compileGraph(graph.graphNodes, graph.graphEdges);
    if (!compiled.ok) {
      setCompileError(compiled.error);
      setSummary(null);
      onChange(null);
      return;
    }

    setCompileError(null);
    // The market declares as many inputs as the highest source the graph reads,
    // and the verifier insists every one of them is read.
    const declared = compiled.inputsUsed.length;
    const result = verify(compiled.bytecode, declared);
    setSummary(result);
    onChange(
      result.ok
        ? { bytecode: compiled.bytecode, inputsUsed: compiled.inputsUsed, summary: result }
        : null,
    );
  }, [graph, loaded, onChange]);

  const grouped = useMemo(() => {
    const map = new Map<OpSpec['group'], OpSpec[]>();
    for (const spec of OPCODES) {
      map.set(spec.group, [...(map.get(spec.group) ?? []), spec]);
    }
    return [...map.entries()];
  }, []);

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-1.5 rounded-md border-l-2 border-foreground/30 bg-muted/40 px-3 py-2 text-xs">
        <p className="font-medium">Build a measurement, not a condition.</p>
        <InfoHint>
          The graph works out a number — a price, a median, a difference.
          Comparing it against the strike is the protocol&rsquo;s job, over a
          band rather than at a point, which is what stops a nudge across the
          line from being worth the whole pot. The &ldquo;above 100?&rdquo;
          part is the Strike field below.
        </InfoHint>
      </div>

    <div className="grid gap-4 lg:grid-cols-[13rem_1fr]">
      <aside className="space-y-4">
        <div>
          <p className="mb-2 text-xs font-medium uppercase tracking-wide text-muted-foreground">
            Start from
          </p>
          <p className="mb-2 text-[11px] leading-snug text-muted-foreground">
            A ready market you can then change.
          </p>
          <div className="space-y-1">
            {TEMPLATES.map((template) => (
              <button
                key={template.id}
                type="button"
                onClick={() => loadTemplate(template.id)}
                className="w-full rounded-md border px-2 py-1.5 text-left text-xs hover:border-foreground/30 hover:bg-accent"
                title={template.description}
              >
                {template.name}
              </button>
            ))}
          </div>
        </div>

        {grouped.map(([group, specs]) => (
          <div key={group}>
            <p className="mb-2 text-xs font-medium uppercase tracking-wide text-muted-foreground">
              {GROUP_LABELS[group]}
            </p>
            <div className="flex flex-wrap gap-1">
              {specs.map((spec) => (
                <button
                  key={spec.op}
                  type="button"
                  draggable
                  onDragStart={(event) => {
                    event.dataTransfer.setData('application/predikt-op', String(spec.op));
                    event.dataTransfer.effectAllowed = 'copy';
                  }}
                  onClick={() => place(spec)}
                  title={`${spec.hint}\n\nDrag onto the canvas, or click to drop it in.`}
                  className="cursor-grab rounded-md border px-2 py-1 text-xs hover:border-foreground/30 hover:bg-accent active:cursor-grabbing"
                >
                  {spec.label}
                </button>
              ))}
            </div>
          </div>
        ))}
      </aside>

      <div className="space-y-3">
        <div
          className="h-[26rem] overflow-hidden rounded-lg border"
          onDragOver={(event) => {
            event.preventDefault();
            event.dataTransfer.dropEffect = 'copy';
          }}
          onDrop={(event) => {
            event.preventDefault();
            const raw = event.dataTransfer.getData('application/predikt-op');
            if (!raw) return;
            const spec = SPEC_BY_OP.get(Number(raw));
            if (!spec) return;
            place(
              spec,
              screenToFlowPosition({ x: event.clientX, y: event.clientY }),
            );
          }}
        >
          <ReactFlow
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            onConnect={onConnect}
            isValidConnection={isValidConnection}
            fitView
            proOptions={{ hideAttribution: true }}
          >
            <Background />
            <Controls showInteractive={false} />
          </ReactFlow>
        </div>

        <div className="flex flex-wrap items-center gap-3 text-xs">
          <span className="text-muted-foreground">A socket carries:</span>
          {Object.entries(PORT_COLOUR).map(([type, colour]) => (
            <span key={type} className="flex items-center gap-1.5 text-muted-foreground">
              <span
                className="size-3 rounded-full"
                style={{ background: colour, boxShadow: '0 0 0 1px var(--color-border)' }}
              />
              {type === 'num' ? 'a number' : type === 'bool' ? 'yes or no' : 'bytes'}
            </span>
          ))}
          <span className="ml-auto text-muted-foreground">
            {loaded ? 'checked by the on-chain verifier' : 'loading the verifier…'}
          </span>
        </div>

        <Status compileError={compileError} summary={summary} empty={nodes.length === 0} />

        {nodes.length > 0 && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              setNodes([]);
              setEdges([]);
            }}
          >
            Clear
          </Button>
        )}
      </div>
    </div>
    </div>
  );
}

function Status({
  compileError,
  summary,
  empty,
}: {
  compileError: string | null;
  summary: VerifyResult | null;
  empty: boolean;
}) {
  if (empty) {
    return (
      <div className="space-y-2 rounded-md border border-dashed p-3 text-xs text-muted-foreground">
        <p className="font-medium text-foreground">
          Start from a template on the left, or drag blocks onto the canvas.
        </p>
        <ol className="list-inside list-decimal space-y-1">
          <li>Drag from a block's right socket to another block's left socket to wire them.</li>
          <li>
            Sockets only join when their colours match, so a yes/no answer cannot be fed
            into arithmetic.
          </li>
          <li>
            One block must be left over. Its value is what the market measures — the
            protocol compares that against the strike, so the graph never does.
          </li>
        </ol>
      </div>
    );
  }
  if (compileError) {
    return <p className="rounded-md border border-dashed p-3 text-xs">{compileError}</p>;
  }
  if (!summary) return null;
  if (!summary.ok) {
    return (
      <p className="rounded-md border border-destructive/40 bg-destructive/5 p-3 text-xs">
        {summary.error}
      </p>
    );
  }
  return (
    <div className="space-y-2">
      <p className="rounded-md border border-dashed p-3 text-xs">
        <span className="font-medium">Reads as: </span>
        Yes wins above the strike, No below; the band splits between.
      </p>
      <div className="flex flex-wrap items-center gap-2">
      <Badge variant="secondary" className="text-xs">
        verified
      </Badge>
      <span className={cn('text-xs text-muted-foreground')}>
        {summary.ops} instructions · {summary.bytes} bytes · reads{' '}
        {summary.inputCount} source{summary.inputCount === 1 ? '' : 's'} · peak stack{' '}
        {summary.maxStackDepth}
      </span>
      </div>
    </div>
  );
}

export function PredicateBuilderPanel(props: {
  sources: string[];
  onChange: (built: BuiltPredicate | null) => void;
}) {
  return (
    <ReactFlowProvider>
      <Canvas {...props} />
    </ReactFlowProvider>
  );
}
