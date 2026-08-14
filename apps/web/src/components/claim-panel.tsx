import type { Market } from '@prediction-market/sdk';
import type { PublicKey } from '@solana/web3.js';

import { Button } from '@/components/ui/button';
import { ErrorNotice } from '@/components/error-notice';
import { useMarketActions, useTokenBalance } from '@/hooks/use-actions';
import { useMintDecimals } from '@/hooks/use-mint';
import { formatAmount } from '@/lib/format';

/**
 * Both sides are shown even when one is worth nothing: claiming it still closes
 * the token account and returns the rent, real money against a small bet.
 */
export function ClaimPanel({
  address,
  market,
  onDone,
}: {
  address: PublicKey;
  market: Market;
  onDone: () => void;
}) {
  const actions = useMarketActions(address, market);
  const yesHeld = useTokenBalance(market.yesMint);
  const noHeld = useTokenBalance(market.noMint);
  const decimals = useMintDecimals(market.collateralMint);

  const owed = (held: bigint, pool: bigint, staked: bigint) =>
    staked === 0n ? 0n : (held * pool) / staked;

  const rows = [
    {
      side: 'yes' as const,
      held: yesHeld.balance ?? 0n,
      payout: owed(yesHeld.balance ?? 0n, market.poolYes, market.stakedYes),
      refresh: yesHeld.refresh,
    },
    {
      side: 'no' as const,
      held: noHeld.balance ?? 0n,
      payout: owed(noHeld.balance ?? 0n, market.poolNo, market.stakedNo),
      refresh: noHeld.refresh,
    },
  ].filter((row) => row.held > 0n);

  if (rows.length === 0) {
    return (
      <p className="rounded-md border border-dashed p-4 text-sm text-muted-foreground">
        You hold no position in this market.
      </p>
    );
  }

  return (
    <div className="space-y-3">
      {rows.map((row) => (
        <div
          key={row.side}
          className="flex items-center justify-between rounded-md border p-3"
        >
          <div className="space-y-0.5">
            <p className="text-sm font-medium capitalize">{row.side}</p>
            <p className="text-xs text-muted-foreground">
              {decimals === null ? '—' : formatAmount(row.held, decimals)} held ·{' '}
              {decimals === null ? '—' : formatAmount(row.payout, decimals)} owed
            </p>
          </div>
          <Button
            size="sm"
            variant={row.payout > 0n ? 'default' : 'secondary'}
            disabled={!actions.connected || actions.pending}
            onClick={() =>
              void actions
                .claim(row.side === 'yes')
                .then(() => {
                  row.refresh();
                  onDone();
                })
                // The failure is already in `actions.error`; without this the
                // rethrow surfaces as an unhandled rejection in the console.
                .catch(() => {})
            }
          >
            {row.payout > 0n ? 'Claim' : 'Close position'}
          </Button>
        </div>
      ))}
      <ErrorNotice failure={actions.error} />
    </div>
  );
}
