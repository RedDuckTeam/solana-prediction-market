import { Handle, Position, type NodeProps } from '@xyflow/react';

import { Input } from '@/components/ui/input';
import { PORT_COLOUR, PORT_LABEL, SPEC_BY_OP, type PortType } from '@/lib/opcodes';
import { cn } from '@/lib/utils';

export interface OpcodeNodeData extends Record<string, unknown> {
  op: number;
  value?: string;
  inputIndex?: number;
  arity?: number;
  /** Labels of the market's declared price sources, for the source block. */
  sources: string[];
  onChange: (patch: { value?: string; inputIndex?: number; arity?: number }) => void;
}

/**
 * One instruction. Sockets get their own labelled row: an unlabelled circle
 * says nothing about which operand it is, and for `a − b` that is two markets.
 */
const socket = (type: PortType) => ({
  width: 12,
  height: 12,
  background: PORT_COLOUR[type],
  border: '2px solid var(--color-card)',
  boxShadow: '0 0 0 1px var(--color-border)',
});

export function OpcodeNode({ data, selected }: NodeProps & { data: OpcodeNodeData }) {
  const spec = SPEC_BY_OP.get(data.op);
  if (!spec) return null;

  const arity = spec.immediate === 'arity' ? (data.arity ?? spec.inputs.length) : spec.inputs.length;
  const inputs: PortType[] =
    spec.immediate === 'arity'
      ? Array.from({ length: arity }, () => spec.inputs[0] ?? 'num')
      : spec.inputs;

  const nameOf = (index: number) =>
    spec.immediate === 'arity'
      ? `#${index + 1}`
      : (spec.portNames?.[index] ?? PORT_LABEL[inputs[index] ?? 'num']);

  return (
    <div
      className={cn(
        'w-52 rounded-lg border-2 bg-card shadow-sm transition-colors',
        selected ? 'border-foreground/60' : 'border-border',
      )}
    >
      <div className="border-b px-3 py-2">
        <p className="text-sm font-semibold leading-none">{spec.label}</p>
        <p className="mt-1 text-[11px] leading-snug text-muted-foreground">{spec.hint}</p>
      </div>

      {inputs.length > 0 && (
        <div className="py-1">
          {inputs.map((type, index) => (
            <div key={index} className="relative flex h-7 items-center pl-3 pr-2">
              <Handle
                id={`in-${index}`}
                type="target"
                position={Position.Left}
                style={{ ...socket(type), left: -7 }}
              />
              <span className="text-[11px] text-muted-foreground">{nameOf(index)}</span>
              <span className="ml-auto text-[10px] uppercase tracking-wide text-muted-foreground/60">
                {PORT_LABEL[type]}
              </span>
            </div>
          ))}
        </div>
      )}

      {spec.immediate === 'const' && (
        <div className="border-t px-3 py-2">
          <Input
            className="h-7 text-xs"
            inputMode="decimal"
            placeholder="0.00"
            value={data.value ?? ''}
            onChange={(event) =>
              data.onChange({ value: event.target.value.replace(/[^\d.-]/g, '') })
            }
          />
        </div>
      )}

      {spec.immediate === 'input-index' && (
        <div className="border-t px-3 py-2">
          <select
            className="h-7 w-full rounded-md border bg-background px-2 text-xs"
            value={data.inputIndex ?? 0}
            onChange={(event) => data.onChange({ inputIndex: Number(event.target.value) })}
          >
            {data.sources.map((label, index) => (
              <option key={index} value={index}>
                {label}
              </option>
            ))}
          </select>
        </div>
      )}

      {spec.immediate === 'arity' && (
        <div className="flex items-center justify-between border-t px-3 py-2 text-xs">
          <span className="text-muted-foreground">how many</span>
          <div className="flex items-center gap-1">
            <button
              type="button"
              className="size-5 rounded border leading-none hover:bg-accent"
              onClick={() => data.onChange({ arity: Math.max(1, arity - 1) })}
            >
              −
            </button>
            <span className="w-4 text-center tabular-nums">{arity}</span>
            <button
              type="button"
              className="size-5 rounded border leading-none hover:bg-accent"
              onClick={() => data.onChange({ arity: Math.min(8, arity + 1) })}
            >
              +
            </button>
          </div>
        </div>
      )}

      <div className="relative flex h-7 items-center border-t px-3">
        <span className="text-[11px] font-medium">gives</span>
        {spec.output !== 'num' && (
          <span
            className="ml-1.5 text-[10px] text-muted-foreground"
            title="A market measures a number. This can feed another block, but cannot be the final answer."
          >
            (not a result)
          </span>
        )}
        <span className="ml-auto text-[10px] uppercase tracking-wide text-muted-foreground/60">
          {PORT_LABEL[spec.output]}
        </span>
        <Handle
          id="out"
          type="source"
          position={Position.Right}
          style={{ ...socket(spec.output), right: -7 }}
        />
      </div>
    </div>
  );
}
