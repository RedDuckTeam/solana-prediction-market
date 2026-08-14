import { scheduleOf, type Market } from '@prediction-market/sdk';
import { windowCoverage } from '@prediction-market/predicate-wasm';

import { useMemo } from 'react';

import { useVerifier } from '@/hooks/use-verifier';
import type { MarketSource } from '@/hooks/use-sources';

export interface SourceCoverage {
  label: string;
  ok: boolean;
  reason: string;
  observationsInside: number;
}

export interface WindowCoverage {
  sources: SourceCoverage[];
  /** Every source has to be readable; a market settles on all of them or none. */
  ready: boolean;
}

/**
 * Whether a window can be settled from yet, per source, from the same reader
 * `snapshot` uses, compiled to WebAssembly. A second opinion in TypeScript
 * could disagree with the chain, and the chain's is the one that matters.
 */
export function useWindowCoverage(market: Market | null, sources: MarketSource[]): WindowCoverage {
  const verifierReady = useVerifier();

  // Memoised on the accounts themselves: the page re-renders every second as the
  // countdown advances, and re-reading three rings through WebAssembly each time
  // is work whose answer cannot have changed.
  return useMemo((): WindowCoverage => {
    if (!market || !verifierReady || sources.length === 0) {
      return { sources: [], ready: false };
    }

    const schedule = scheduleOf(market);
    const found = sources.map(({ feed, pool, ring }): SourceCoverage => {
      const label = feed.label || 'source';
      if (!ring) {
        return {
          label,
          ok: false,
          reason: 'the price account does not exist',
          observationsInside: 0,
        };
      }
      const verdict = windowCoverage(
        ring.data,
        pool.address.toBytes(),
        BigInt(schedule.windowStart),
        BigInt(Number(market.settleAt)),
        market.params.maxSegment,
        market.params.minObservations,
      );
      const read = {
        label,
        ok: verdict.ok,
        reason: verdict.reason,
        observationsInside: verdict.observationsInside,
      };
      verdict.free();
      return read;
    });

    return { sources: found, ready: found.length > 0 && found.every((source) => source.ok) };
  }, [market, verifierReady, sources]);
}
