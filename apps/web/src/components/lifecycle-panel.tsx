import {
  phaseOf,
  scheduleOf,
  settlementStep,
  type Market,
} from '@prediction-market/sdk';
import type { PublicKey } from '@solana/web3.js';
import type { ReactNode } from 'react';

import { ClaimPanel } from '@/components/claim-panel';
import { ErrorNotice } from '@/components/error-notice';
import { StakePanel } from '@/components/stake-panel';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { InfoHint } from '@/components/ui/info-hint';
import type { WindowCoverage } from '@/hooks/use-coverage';
import { useMarketActions } from '@/hooks/use-actions';
import { useNudge } from '@/hooks/use-nudge';
import type { MarketSources } from '@/hooks/use-sources';
import { IS_MAINNET } from '@/lib/cluster';
import { formatInstant } from '@/lib/format';

/** Wrapped SOL committed per swap when the window needs its closing trade. */
const SWAP_SIZE = 0.05;

/**
 * A market's whole life asks for exactly one thing at a time: a bet, then a
 * trade, then a snapshot, then a settle, then a claim. This panel is the one
 * place all of them live — the same card in the same spot, changing its face
 * as the market moves — so the reader never hunts the page for the button
 * that matters now.
 */
type Stage =
  | 'loading'
  | 'created'
  | 'open'
  | 'waiting'
  | 'nudge'
  | 'snapshot'
  | 'resolve'
  | 'void'
  | 'claim';

const stageOf = (market: Market, now: number | null, coverage: WindowCoverage): Stage => {
  // The stored status answers for a market past its snapshot without waiting
  // on the clock; everything earlier needs the chain time to be read first.
  if (market.status === 'resolved' || market.status === 'void') return 'claim';
  if (market.status === 'snapshotted') return 'resolve';
  if (now === null) return 'loading';

  const step = settlementStep(market, now);
  if (step === 'void') return 'void';
  if (step === 'resolve') return 'resolve';
  if (step === 'snapshot') {
    // On a test network an unreadable window means nobody has traded, and the
    // fix is a trade; on mainnet it means the sources are still catching up,
    // and the fix is patience.
    return IS_MAINNET || coverage.ready ? 'snapshot' : 'nudge';
  }

  const phase = phaseOf(market, now);
  if (phase === 'created') return 'created';
  if (phase === 'open') return 'open';
  return 'waiting';
};

export function LifecyclePanel({
  address,
  market,
  now,
  coverage,
  sources,
  onDone,
}: {
  address: PublicKey;
  market: Market;
  now: number | null;
  coverage: WindowCoverage;
  sources: MarketSources;
  onDone: () => void;
}) {
  const actions = useMarketActions(address, market);
  const trade = useNudge(sources.sources.map((source) => source.pool));

  const stage = stageOf(market, now, coverage);
  const schedule = scheduleOf(market);

  // Recording prices reads the sources, so offering the button before they can
  // be read is offering a transaction that pays a fee to fail.
  const blocked = stage === 'snapshot' && !coverage.ready;

  const deadline = (
    <p className="text-xs text-muted-foreground">
      Last chance {formatInstant(schedule.graceEnd)} — after that it refunds.
    </p>
  );

  const sendButton = (send: () => Promise<unknown>, label: string) => (
    <>
      <Button
        className="w-full"
        disabled={!actions.connected || actions.pending || blocked}
        // Re-read either way: a failure here usually means somebody else got
        // there first, and the page is a step behind rather than broken.
        onClick={() => void send().then(onDone).catch(onDone)}
      >
        {actions.pending ? 'Sending…' : label}
      </Button>
      {!actions.connected && (
        <p className="text-xs text-muted-foreground">Connect a wallet to send it.</p>
      )}
      <ErrorNotice failure={actions.error} />
    </>
  );

  const face = (): { title: ReactNode; body: ReactNode } => {
    switch (stage) {
      case 'loading':
        return {
          title: 'Place a bet',
          body: (
            <p className="rounded-md border border-dashed p-4 text-sm text-muted-foreground">
              Reading the chain clock…
            </p>
          ),
        };

      case 'created':
        return {
          title: 'Betting not open yet',
          body: (
            <p className="flex items-center gap-1.5 text-sm text-muted-foreground">
              Betting opens {formatInstant(schedule.openAt)}.
              <InfoHint>
                A market waits after creation so its rules can be read before
                any money is in it.
              </InfoHint>
            </p>
          ),
        };

      case 'open':
        return {
          title: 'Place a bet',
          body: <StakePanel address={address} market={market} onDone={onDone} />,
        };

      case 'waiting':
        return {
          title: 'Betting closed',
          body: (
            <div className="space-y-2 text-sm text-muted-foreground">
              <p className="flex items-center gap-1.5">
                Measuring the price until {formatInstant(schedule.settleAt)}.
                <InfoHint>
                  Betting closes before the measured window opens, so nobody
                  bets while watching the prices that settle their bet.
                </InfoHint>
              </p>
              <p className="text-xs">
                When the window ends, the next step appears right here.
              </p>
            </div>
          ),
        };

      case 'nudge':
        return {
          title: (
            <>
              This market needs one trade
              <InfoHint>
                Nobody trades on a test network, so the average has no reading
                at the end of its window. One real swap through every source
                records it — and it lands outside the averaged window, so it
                cannot move where this market settles.
              </InfoHint>
            </>
          ),
          body: (
            <>
              <p className="text-sm text-muted-foreground">
                The measured window ended without a closing trade; one swap
                records it.
              </p>
              {deadline}
              <Button
                className="w-full"
                disabled={!trade.connected || trade.pending}
                onClick={() => {
                  void trade
                    .nudge('up', SWAP_SIZE)
                    .then(() => sources.refresh())
                    .catch(() => {});
                }}
              >
                {trade.pending ? 'Trading…' : 'Trade through the sources'}
              </Button>
              {!trade.connected && (
                <p className="text-xs text-muted-foreground">
                  Connect a wallet to send it.
                </p>
              )}
              <ErrorNotice failure={trade.error} />
            </>
          ),
        };

      case 'snapshot':
        return {
          title: 'The prices are not recorded',
          body: (
            <>
              <p className="text-sm text-muted-foreground">
                Anyone can record them; until someone does, the market cannot
                settle.
              </p>
              {deadline}
              {blocked && (
                <div className="space-y-1 rounded-md border border-dashed p-2 text-xs text-muted-foreground">
                  <p className="font-medium text-foreground">Not yet readable.</p>
                  {coverage.sources
                    .filter((source) => !source.ok)
                    .map((source) => (
                      <p key={source.label}>
                        {source.label} — {source.reason}
                      </p>
                    ))}
                </div>
              )}
              {sendButton(actions.snapshot, 'Record the prices')}
            </>
          ),
        };

      case 'resolve':
        return {
          title: 'Ready to settle',
          body: (
            <>
              <p className="text-sm text-muted-foreground">
                Splits the pot over the recorded prices — same answer whoever
                sends it.
              </p>
              {sendButton(actions.resolve, 'Settle the market')}
            </>
          ),
        };

      case 'void':
        return {
          title: 'This market cannot settle',
          body: (
            <>
              <p className="text-sm text-muted-foreground">
                It refunds everyone at par instead.
              </p>
              {sendButton(actions.voidMarket, 'Void the market')}
            </>
          ),
        };

      case 'claim':
        return {
          title: 'Your position',
          body: <ClaimPanel address={address} market={market} onDone={onDone} />,
        };
    }
  };

  const { title, body } = face();

  return (
    <Card>
      <CardHeader>
        <CardTitle className="inline-flex items-center gap-1.5 text-sm font-medium">
          {title}
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">{body}</CardContent>
    </Card>
  );
}
