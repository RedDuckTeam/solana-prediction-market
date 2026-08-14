import { phaseOf, scheduleOf, type Market } from '@prediction-market/sdk';

import { Badge } from '@/components/ui/badge';
import { useChainTime } from '@/hooks/use-markets';
import { formatCountdown } from '@/lib/format';

const LABEL: Record<string, string> = {
  created: 'Opening soon',
  open: 'Open',
  locked: 'Betting closed',
  snapshotted: 'Settling',
  resolved: 'Resolved',
  void: 'Voided',
};

export function MarketStatusBadge({ market, now }: { market: Market; now: number | null }) {
  const phase = now === null ? market.status : phaseOf(market, now);
  const variant =
    phase === 'open' ? 'default' : phase === 'void' ? 'destructive' : 'secondary';
  return <Badge variant={variant}>{LABEL[phase] ?? phase}</Badge>;
}

/**
 * The badge and the countdown, on their own second hand.
 *
 * These two are the only things on a market page that genuinely change every
 * second. Giving them their own clock confines the per-second re-render to
 * this fragment; the page around them holds a coarser clock and stays still
 * under the user's typing.
 */
export function LiveStatus({ market }: { market: Market }) {
  const now = useChainTime();
  return (
    <div className="flex items-center gap-2">
      <MarketStatusBadge market={market} now={now} />
      <NextDeadline market={market} now={now} />
    </div>
  );
}

/** What happens next, and when. */
export function NextDeadline({ market, now }: { market: Market; now: number | null }) {
  if (now === null) return null;
  const schedule = scheduleOf(market);
  const phase = phaseOf(market, now);

  const upcoming =
    phase === 'created'
      ? { label: 'opens in', at: schedule.openAt }
      : phase === 'open'
        ? { label: 'betting closes in', at: schedule.lockAt }
        : phase === 'locked'
          ? { label: 'settles in', at: schedule.settleAt }
          : null;

  if (!upcoming) return null;
  return (
    <span className="text-sm text-muted-foreground">
      {upcoming.label}{' '}
      <span className="font-medium text-foreground">
        {formatCountdown(upcoming.at - now)}
      </span>
    </span>
  );
}
