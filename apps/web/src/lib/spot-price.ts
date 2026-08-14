import { fromDecimalString } from '@prediction-market/sdk';

import type { MarketSource } from '@/hooks/use-sources';

const median = (values: number[]): number => {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0
    ? ((sorted[middle - 1] ?? 0) + (sorted[middle] ?? 0)) / 2
    : (sorted[middle] ?? 0);
};

/**
 * What the sources say now, combined the way settlement will combine them, in
 * the protocol's fixed point so the page renders it by the rule it renders the
 * strike. Not what settles anything: that is an average over a window, later.
 *
 * A market may read its pool upside down, and then the price a person means is
 * the reciprocal of the one the pool quotes.
 */
export function spotPrice(sources: MarketSource[]): bigint | null {
  if (sources.length === 0) return null;
  const prices = sources.map((source) =>
    source.declaredInvert ? 1 / source.pool.price : source.pool.price,
  );
  return fromDecimalString(median(prices).toFixed(9));
}
