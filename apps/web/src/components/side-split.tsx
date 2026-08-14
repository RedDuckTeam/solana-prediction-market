import type { Market } from '@prediction-market/sdk';

import { cn } from '@/lib/utils';

/**
 * A share of the pot, never a probability: before betting closes this is a
 * ratio that moves sharply, and "73% chance" would assert what nobody knows.
 */
export function SideSplit({ market }: { market: Market }) {
  const yes = market.stakedYes;
  const no = market.stakedNo;
  const total = yes + no;
  const yesPercent = total === 0n ? 50 : Number((yes * 1000n) / total) / 10;

  return (
    <div className="space-y-1.5">
      <div className="flex h-2 overflow-hidden rounded-full bg-muted">
        <div
          className={cn('bg-yes transition-[width]')}
          style={{ width: `${total === 0n ? 0 : yesPercent}%` }}
        />
        <div
          className={cn('bg-no transition-[width]')}
          style={{ width: `${total === 0n ? 0 : 100 - yesPercent}%` }}
        />
      </div>
      <div className="flex justify-between text-xs text-muted-foreground">
        <span>
          <span className="font-medium text-yes">Yes</span>{' '}
          {total === 0n ? 'no stake yet' : `${yesPercent.toFixed(1)}% of the pot`}
        </span>
        <span>
          <span className="font-medium text-no">No</span>{' '}
          {total === 0n ? '' : `${(100 - yesPercent).toFixed(1)}%`}
        </span>
      </div>
    </div>
  );
}
