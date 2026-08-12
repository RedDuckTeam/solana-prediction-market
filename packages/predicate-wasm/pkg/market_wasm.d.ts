/* tslint:disable */
/* eslint-disable */
/**
 * Reads a Raydium observation ring exactly as `snapshot` will.
 */
export function windowCoverage(ring: Uint8Array, pool: Uint8Array, from: bigint, to: bigint, max_segment: number, min_observations: number): Coverage;
/**
 * Runs a predicate over hypothetical prices, exactly as `resolve` will.
 *
 * This is what lets the editor — and the template tests behind it — show the
 * score a draft would settle to, instead of promising that a graph which
 * merely *verifies* also *means* what its description says. Inputs and the
 * result are raw Q64.64 values carried as strings, because a JavaScript
 * number cannot hold an i128.
 */
export function evaluatePredicate(code: Uint8Array, inputs_raw: string[], settle_at: bigint): string;
/**
 * Verifies bytecode exactly as `create_market` will.
 *
 * The error is a sentence rather than a code, because it is shown to whoever
 * is drawing the graph and they cannot look up a discriminant.
 */
export function verifyPredicate(code: Uint8Array, input_count: number): Verified;
/**
 * The limits the editor should enforce before it even asks.
 */
export function predicateLimits(): any;
/**
 * What a settlement would see in a pool's ring right now.
 *
 * The editor shows whether a window is answerable yet, and the only honest way
 * to say so is to ask the reader that will be asked at settlement. Anything
 * else is a second opinion that can differ from the one that decides money.
 */
export class Coverage {
  private constructor();
  free(): void;
  [Symbol.dispose](): void;
  readonly observationsInside: number;
  readonly ok: boolean;
  readonly reason: string;
}
/**
 * What a verified program costs, so the editor can show it before anyone signs.
 */
export class Verified {
  private constructor();
  free(): void;
  [Symbol.dispose](): void;
  readonly inputCount: number;
  readonly maxStackDepth: number;
  readonly ops: number;
  readonly bytes: number;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly __wbg_coverage_free: (a: number, b: number) => void;
  readonly __wbg_verified_free: (a: number, b: number) => void;
  readonly coverage_observationsInside: (a: number) => number;
  readonly coverage_ok: (a: number) => number;
  readonly coverage_reason: (a: number) => [number, number];
  readonly evaluatePredicate: (a: number, b: number, c: number, d: number, e: bigint) => [number, number, number, number];
  readonly verified_bytes: (a: number) => number;
  readonly verified_inputCount: (a: number) => number;
  readonly verified_maxStackDepth: (a: number) => number;
  readonly verified_ops: (a: number) => number;
  readonly verifyPredicate: (a: number, b: number, c: number) => [number, number, number];
  readonly windowCoverage: (a: number, b: number, c: number, d: number, e: bigint, f: bigint, g: number, h: number) => number;
  readonly predicateLimits: () => any;
  readonly __wbindgen_malloc: (a: number, b: number) => number;
  readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_export_2: WebAssembly.Table;
  readonly __externref_table_alloc: () => number;
  readonly __externref_table_dealloc: (a: number) => void;
  readonly __wbindgen_free: (a: number, b: number, c: number) => void;
  readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;
/**
* Instantiates the given `module`, which can either be bytes or
* a precompiled `WebAssembly.Module`.
*
* @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
*
* @returns {InitOutput}
*/
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
* If `module_or_path` is {RequestInfo} or {URL}, makes a request and
* for everything else, calls `WebAssembly.instantiate` directly.
*
* @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
*
* @returns {Promise<InitOutput>}
*/
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
