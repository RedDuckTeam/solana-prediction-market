import { phaseOf, settlementStep, type Market } from '@prediction-market/sdk';
import { useState } from 'react';

import { MarketCard } from '@/components/market-card';
import { DemoBanner } from '@/components/demo-banner';
import { HowItWorks } from '@/components/how-it-works';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { useChainTime, useMarkets } from '@/hooks/use-markets';

/**
 * The three things somebody comes here to do. Filtered over what was already
 * fetched: narrowing on the node costs one `getProgramAccounts` per status.
 */
const FILTERS = {
  all: { label: 'All', matches: () => true },
  open: {
    label: 'Open to bets',
    matches: (market: Market, now: number) => phaseOf(market, now) === 'open',
  },
  waiting: {
    label: 'Needs settling',
    matches: (market: Market, now: number) => settlementStep(market, now) !== null,
  },
  done: {
    label: 'Settled',
    matches: (market: Market) => market.status === 'resolved' || market.status === 'void',
  },
} as const;

type FilterKey = keyof typeof FILTERS;

export function MarketsRoute() {
  const { data, loading, error } = useMarkets();
  const now = useChainTime();
  const [active, setActive] = useState<FilterKey>('all');

  const markets = data ?? [];
  const counts = Object.fromEntries(
    (Object.keys(FILTERS) as FilterKey[]).map((key) => [
      key,
      now === null ? markets.length : markets.filter((e) => FILTERS[key].matches(e.account, now)).length,
    ]),
  ) as Record<FilterKey, number>;
  const shown = now === null ? markets : markets.filter((e) => FILTERS[active].matches(e.account, now));

  return (
    <div className="space-y-6">
      <DemoBanner />

      <div className="space-y-1">
        <h1 className="text-2xl font-semibold tracking-tight">Markets</h1>
        <p className="text-sm text-muted-foreground">
          Each one settles from a time-weighted price that anyone can re-derive from
          chain state.
        </p>
      </div>

      {error && (
        <p className="rounded-md border border-destructive/40 bg-destructive/5 p-4 text-sm">
          Could not reach the program on this cluster. It may not be deployed here yet.
          <span className="mt-1 block font-mono text-xs opacity-70">{error}</span>
        </p>
      )}

      {loading && (
        <div className="grid gap-4 sm:grid-cols-2">
          {[0, 1].map((key) => (
            <Skeleton key={key} className="h-40 w-full rounded-xl" />
          ))}
        </div>
      )}

      {!loading && !error && markets.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {(Object.keys(FILTERS) as FilterKey[]).map((key) => (
            <Button
              key={key}
              size="sm"
              variant={active === key ? 'secondary' : 'ghost'}
              onClick={() => setActive(key)}
            >
              {FILTERS[key].label}
              <span className="ml-1.5 text-xs text-muted-foreground">{counts[key]}</span>
            </Button>
          ))}
        </div>
      )}

      {!loading && !error && markets.length === 0 && (
        <p className="rounded-md border border-dashed p-8 text-center text-sm text-muted-foreground">
          No markets have been created on this cluster yet.
        </p>
      )}

      {markets.length > 0 && shown.length === 0 && (
        <p className="rounded-md border border-dashed p-8 text-center text-sm text-muted-foreground">
          Nothing here right now. Every market is somewhere else in its life.
        </p>
      )}

      {shown.length > 0 && (
        <div className="grid gap-4 sm:grid-cols-2">
          {shown.map((entry) => (
            <MarketCard
              key={entry.address.toBase58()}
              address={entry.address}
              market={entry.account}
              now={now}
            />
          ))}
        </div>
      )}

      <HowItWorks />
    </div>
  );
}
