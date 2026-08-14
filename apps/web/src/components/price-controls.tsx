import type { ClmmPool } from '@prediction-market/sdk';
import { NATIVE_MINT } from '@solana/spl-token';

import { Button } from '@/components/ui/button';
import { ErrorNotice } from '@/components/error-notice';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { InfoHint } from '@/components/ui/info-hint';
import { useNudge } from '@/hooks/use-nudge';
import { IS_MAINNET } from '@/lib/cluster';

/** Wrapped SOL committed per swap. Enough to move a price visibly. */
const SWAP_SIZE = 0.05;

/** What the token costs in SOL, whichever way Raydium happens to quote the pair. */
const tokenPrice = (pool: ClmmPool): number =>
  pool.mint0.equals(NATIVE_MINT) ? 1 / pool.price : pool.price;

/**
 * Moving the pools, for its own sake.
 *
 * Separate from the trade a market needs before it can settle: this one is for
 * putting a price where somebody wants it — to pick a strike against, or to
 * decide where a market they have bet on lands. Test networks only, and real
 * swaps either way.
 */
export function PriceControls({
  pools,
  onMoved,
}: {
  pools: ClmmPool[];
  onMoved: () => void;
}) {
  const { nudge, pending, error, connected } = useNudge(pools);
  if (IS_MAINNET || pools.length === 0) return null;

  const prices = pools.map(tokenPrice).sort((a, b) => a - b);
  const median = prices[Math.floor(prices.length / 2)] ?? 0;

  const send = (direction: 'up' | 'down') => {
    void nudge(direction, SWAP_SIZE)
      .then(onMoved)
      .catch(() => {});
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle className="inline-flex items-center gap-1.5 text-sm font-medium">
          Move the price
          <InfoHint>
            Test network only: nobody trades here, so the pools sit where they
            were left. Each press swaps through all {pools.length} of them at
            once — real trades, so the price moves and you pay the fee.
          </InfoHint>
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <p className="text-sm">
          <span className="text-muted-foreground">Token now </span>
          <span className="font-medium">{median.toPrecision(5)} SOL</span>
        </p>

        <div className="grid grid-cols-2 gap-2">
          <Button
            variant="secondary"
            size="sm"
            disabled={!connected || pending}
            onClick={() => send('up')}
          >
            {pending ? 'Trading…' : 'Push up'}
          </Button>
          <Button
            variant="secondary"
            size="sm"
            disabled={!connected || pending}
            onClick={() => send('down')}
          >
            {pending ? 'Trading…' : 'Push down'}
          </Button>
        </div>

        {!connected && (
          <p className="text-xs text-muted-foreground">Connect a wallet to trade.</p>
        )}
        <ErrorNotice failure={error} />
      </CardContent>
    </Card>
  );
}
