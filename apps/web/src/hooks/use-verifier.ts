import init, { verifyPredicate } from '@prediction-market/predicate-wasm';
import { useEffect, useState } from 'react';

let ready: Promise<void> | null = null;

const load = (): Promise<void> => {
  ready ??= init().then(() => undefined);
  return ready;
};

export interface VerifyOk {
  ok: true;
  ops: number;
  maxStackDepth: number;
  inputCount: number;
  bytes: number;
}

export type VerifyResult = VerifyOk | { ok: false; error: string };

/**
 * The crate `create_market` calls, compiled to WebAssembly, so the editor's
 * verdict and the chain's cannot drift. A reimplementation would disagree, and
 * the disagreement would surface as a transaction that failed after paying.
 */
export const verify = (bytecode: Uint8Array, inputCount: number): VerifyResult => {
  try {
    const result = verifyPredicate(bytecode, inputCount);
    const value: VerifyOk = {
      ok: true,
      ops: result.ops,
      maxStackDepth: result.maxStackDepth,
      inputCount: result.inputCount,
      bytes: result.bytes,
    };
    result.free();
    return value;
  } catch (cause) {
    return { ok: false, error: cause instanceof Error ? cause.message : String(cause) };
  }
};

/** `false` until the module is in memory; verification cannot run before then. */
export function useVerifier(): boolean {
  const [loaded, setLoaded] = useState(false);
  useEffect(() => {
    let cancelled = false;
    void load().then(() => !cancelled && setLoaded(true));
    return () => {
      cancelled = true;
    };
  }, []);
  return loaded;
}
