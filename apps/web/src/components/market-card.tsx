import { scheduleOf, toSignificantString, type Market } from '@prediction-market/sdk';
import type { PublicKey } from '@solana/web3.js';
import { Link } from 'react-router';

import { MarketStatusBadge, NextDeadline } from '@/components/market-status';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import { formatUtc } from '@/lib/format';

import { SideSplit } from './side-split';

export function MarketCard({
  address,
  market,
  now,
}: {
  address: PublicKey;
  market: Market;
  now: number | null;
}) {
  const schedule = scheduleOf(market);
  return (
    <Link to={`/markets/${address.toBase58()}`} className="block">
      <Card className="transition-colors hover:border-foreground/20">
        <CardHeader className="gap-2">
          <div className="flex items-center gap-2">
            <MarketStatusBadge market={market} now={now} />
            <NextDeadline market={market} now={now} />
          </div>
          <h2 className="text-base font-medium">
            Above {toSignificantString(market.strike)} at {formatUtc(schedule.settleAt)}
          </h2>
        </CardHeader>
        <CardContent>
          <SideSplit market={market} />
        </CardContent>
      </Card>
    </Link>
  );
}
