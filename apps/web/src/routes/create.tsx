import {
  fetchCollaterals,
  fetchConfig,
  fetchFeeds,
  fromDecimalString,
  readClmmPools,
  MIN_MARKET_FEEDS,
  type ClmmPool,
  type CollateralRecord,
  type Config,
  type FeedRecord,
} from '@prediction-market/sdk';
import { useConnection } from '@solana/wallet-adapter-react';
import { PublicKey } from '@solana/web3.js';
import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router';

import { PredicateBuilderPanel, type BuiltPredicate } from '@/components/builder/predicate-builder';
import { ErrorNotice } from '@/components/error-notice';
import { PriceControls } from '@/components/price-controls';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { InfoHint } from '@/components/ui/info-hint';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useCreateMarket } from '@/hooks/use-create-market';
import { formatInstant, shortAddress } from '@/lib/format';
import { cn } from '@/lib/utils';

/** The program's own floor, read from the IDL rather than typed here twice. */
const MIN_SOURCES = MIN_MARKET_FEEDS;

/**
 * Derived, not guessed: betting closes a window plus the skew before
 * settlement and cannot close before it opens, so the floor is their sum.
 */
const earliestSettlement = (config: Config | null, now: number): number =>
  config === null
    ? now
    : now + config.params.creationCooldown + config.params.twapWindow + config.params.skew + 60;
/** Names for the mints a reader would otherwise see as base58. */
const MINT_NAMES: Record<string, string> = {
  So11111111111111111111111111111111111111112: 'SOL',
  EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v: 'USDC',
};

const RULES_URI = 'https://github.com/RedDuckTeam/solana-prediction-market/blob/main/README.md';

/**
 * Sources come from the registry and cannot be typed in — whether a price is
 * worth settling money against is decided ahead of time, not by the creator.
 * What is measured, and against what, is theirs.
 */
export function CreateRoute() {
  const { connection } = useConnection();
  const navigate = useNavigate();
  const { create, pending, error, connected } = useCreateMarket();
  const [feeds, setFeeds] = useState<FeedRecord[] | null>(null);
  const [chosen, setChosen] = useState<string[]>([]);
  const [inverted, setInverted] = useState<Record<string, boolean>>({});
  const [strike, setStrike] = useState('');
  const [band, setBand] = useState('0.50');
  const [settleAt, setSettleAt] = useState('');
  const [built, setBuilt] = useState<BuiltPredicate | null>(null);
  const [collaterals, setCollaterals] = useState<CollateralRecord[]>([]);
  const [collateral, setCollateral] = useState<string | null>(null);
  const [config, setConfig] = useState<Config | null>(null);
  const [pools, setPools] = useState<ClmmPool[]>([]);
  const [moved, setMoved] = useState(0);

  useEffect(() => {
    let cancelled = false;
    fetchFeeds(connection)
      .then(async (found) => {
        if (cancelled) return;
        setFeeds(found);
        // The pools behind them, so a price can be put where the strike wants it.
        const usable = found.filter((feed) => feed.enabled);
        setPools(await readClmmPools(connection, usable.map((feed) => feed.pool)));
      })
      .catch(() => !cancelled && setFeeds([]));
    fetchCollaterals(connection)
      .then((found) => {
        if (cancelled) return;
        const usable = found.filter((entry) => entry.enabled);
        setCollaterals(usable);
        setCollateral((current) => current ?? usable[0]?.mint.toBase58() ?? null);
      })
      .catch(() => !cancelled && setCollaterals([]));
    fetchConfig(connection)
      .then((found) => !cancelled && setConfig(found))
      .catch(() => !cancelled && setConfig(null));
    return () => {
      cancelled = true;
    };
  }, [connection, moved]);

  const now = Math.floor(Date.now() / 1000);
  const active = (feeds ?? []).filter((feed) => feed.enabled && Number(feed.effectiveAt) <= now);
  const settleSeconds = settleAt ? Math.floor(new Date(settleAt).getTime() / 1000) : null;

  const chosenFeeds = chosen
    .map((id) => active.find((feed) => feed.address.toBase58() === id))
    .filter((feed): feed is FeedRecord => Boolean(feed));
  const sourceLabels = chosenFeeds.map(
    (feed, index) => feed.label || `Source ${index + 1}`,
  );

  const onPredicate = useCallback((next: BuiltPredicate | null) => setBuilt(next), []);

  const problems: string[] = [];
  if (chosen.length < MIN_SOURCES) {
    problems.push(`Choose at least ${MIN_SOURCES} price sources.`);
  }
  if (!built) problems.push('Finish the expression so it verifies.');
  else if (built.inputsUsed.length !== chosen.length) {
    problems.push(
      `The expression reads ${built.inputsUsed.length} of the ${chosen.length} sources chosen. Every declared source has to be read.`,
    );
  }
  if (!collateral) problems.push('Choose what this market is staked in.');
  if (!strike) problems.push('Set a strike.');
  else {
    // Refused here rather than by a failed transaction: the chain requires a
    // positive strike. A "below" market is not a negative strike -- it is the
    // NO side of this same question.
    let parsed: bigint | null = null;
    try {
      parsed = fromDecimalString(strike);
    } catch {
      problems.push('The strike is not a number.');
    }
    if (parsed !== null && parsed <= 0n) {
      problems.push(
        'The strike has to be above zero. To bet that the price ends below it, take the NO side.',
      );
    }
  }
  const earliest = earliestSettlement(config, now);
  if (!settleSeconds) problems.push('Set a settlement time.');
  else if (settleSeconds < earliest) {
    const minutes = Math.ceil((earliest - now) / 60);
    problems.push(
      `Settlement has to be at least ${minutes} minutes away: betting closes a full averaging window before it.`,
    );
  }

  return (
    <div className="space-y-6">
      <div className="space-y-1">
        <h1 className="text-2xl font-semibold tracking-tight">Create a market</h1>
        <p className="text-sm text-muted-foreground">
          A question, the sources that answer it, and the band it settles over.
        </p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm font-medium">Price sources</CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          {feeds === null && <p className="text-sm text-muted-foreground">Loading…</p>}
          {feeds !== null && active.length === 0 && (
            <p className="rounded-md border border-dashed p-4 text-sm text-muted-foreground">
              No sources are registered and effective on this cluster yet. Governance
              admits them, one timelock ahead of use.
            </p>
          )}
          {active.map((feed) => {
            const id = feed.address.toBase58();
            const picked = chosen.includes(id);
            return (
              <div
                key={id}
                className={cn(
                  'flex items-center gap-3 rounded-md border p-3 transition-colors',
                  picked ? 'border-foreground/40 bg-accent' : 'hover:border-foreground/20',
                )}
              >
                <button
                  type="button"
                  className="flex flex-1 items-center gap-3 text-left"
                  onClick={() =>
                    setChosen((current) =>
                      picked ? current.filter((entry) => entry !== id) : [...current, id],
                    )
                  }
                >
                  <Badge variant="secondary" className="font-mono text-xs">
                    {feed.kind === 'pythTwap' ? 'oracle' : 'pool'}
                  </Badge>
                  <span className="text-sm">{feed.label || shortAddress(id)}</span>
                  {picked && (
                    <span className="font-mono text-xs text-muted-foreground">
                      input {chosen.indexOf(id)}
                    </span>
                  )}
                </button>
                {picked && (
                  <button
                    type="button"
                    onClick={() =>
                      setInverted((current) => ({ ...current, [id]: !current[id] }))
                    }
                    title="Read the pair the other way round. Two inverted legs multiplied together price a token in dollars without a pool for that pair existing."
                    className={cn(
                      'rounded border px-2 py-1 font-mono text-[11px] transition-colors',
                      inverted[id]
                        ? 'border-foreground/40 bg-background'
                        : 'text-muted-foreground hover:border-foreground/20',
                    )}
                  >
                    {inverted[id] ? 'B/A' : 'A/B'}
                  </button>
                )}
              </div>
            );
          })}
          <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
            At least {MIN_SOURCES} sources.
            <InfoHint>
              Three, so a median survives one source being captured. A/B reads a
              pair as registered; B/A reads it inverted — how a token quoted in
              SOL becomes a dollar price when multiplied by a SOL/USDC leg.
            </InfoHint>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm font-medium">What this market measures</CardTitle>
        </CardHeader>
        <CardContent>
          <PredicateBuilderPanel
            sources={sourceLabels.length > 0 ? sourceLabels : ['(choose sources first)']}
            onChange={onPredicate}
          />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm font-medium">The question</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-2">
            <div className="flex items-center gap-1.5">
              <Label>Staked in</Label>
              <InfoHint>
                What people bet with and are paid in. Governance approves these;
                a mint carrying a transfer fee is refused, because a deposit
                arriving short would leave the vault owing more than it holds.
              </InfoHint>
            </div>
            {collaterals.length === 0 ? (
              <p className="text-xs text-muted-foreground">
                No mint has been approved as stake on this cluster yet.
              </p>
            ) : (
              <div className="flex flex-wrap gap-2">
                {collaterals.map((entry) => {
                  const mint = entry.mint.toBase58();
                  return (
                    <button
                      key={mint}
                      type="button"
                      onClick={() => setCollateral(mint)}
                      className={cn(
                        'rounded-md border px-3 py-1.5 text-sm transition-colors',
                        collateral === mint
                          ? 'border-foreground/40 bg-accent'
                          : 'hover:border-foreground/20',
                      )}
                    >
                      {MINT_NAMES[mint] ?? shortAddress(mint)}
                    </button>
                  );
                })}
              </div>
            )}
          </div>

          <PriceControls pools={pools} onMoved={() => setMoved((n) => n + 1)} />

          <div className="space-y-2">
            <div className="flex items-center gap-1.5">
              <Label htmlFor="strike">Strike</Label>
              <InfoHint>
                Yes wins when the measurement settles above this. The comparison
                belongs to the protocol, not to the expression — a band written
                in bytecode could not be checked on chain.
              </InfoHint>
            </div>
            <Input
              id="strike"
              inputMode="decimal"
              placeholder="100.00"
              value={strike}
              onChange={(event) => setStrike(event.target.value.replace(/[^\d.-]/g, ''))}
            />
          </div>

          <div className="space-y-2">
            <div className="flex items-center gap-1.5">
              <Label htmlFor="band">Settlement band (%)</Label>
              <InfoHint>
                Inside &plusmn;{band || '0'}% of the strike the pot is divided
                continuously; outside it the market is all-or-nothing. The band
                is what stops a price nudged across the strike from being worth
                the whole pot.
              </InfoHint>
            </div>
            <Input
              id="band"
              inputMode="decimal"
              value={band}
              onChange={(event) => setBand(event.target.value.replace(/[^\d.]/g, ''))}
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="settle">Settles at</Label>
            <Input
              id="settle"
              type="datetime-local"
              value={settleAt}
              onChange={(event) => setSettleAt(event.target.value)}
            />
            {settleSeconds !== null && config !== null && (
              <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                Betting closes{' '}
                {formatInstant(settleSeconds - config.params.twapWindow - config.params.skew)}.
                <InfoHint>
                  Before the measured window opens, so nobody bets while
                  watching the prices that settle their bet.
                </InfoHint>
              </div>
            )}
          </div>
        </CardContent>
      </Card>

      {problems.length > 0 && (
        <ul className="space-y-1 text-sm text-muted-foreground">
          {problems.map((problem) => (
            <li key={problem}>· {problem}</li>
          ))}
        </ul>
      )}

      <ErrorNotice failure={error} />

      <Button
        className="w-full"
        disabled={!connected || pending || problems.length > 0}
        onClick={() => {
          if (settleSeconds === null || !built || !collateral) return;
          void create({
            feeds: chosen.map((id) => ({
              feed: new PublicKey(id),
              invert: Boolean(inverted[id]),
            })),
            collateralMint: new PublicKey(collateral),
            strike,
            rampBps: Math.round(Number(band) * 100),
            settleAt: settleSeconds,
            bytecode: built.bytecode,
            rulesUri: RULES_URI,
          })
            .then((market) => navigate(`/markets/${market.toBase58()}`))
            .catch(() => {});
        }}
      >
        {pending ? 'Creating…' : 'Create market'}
      </Button>
      <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
        Costs a small fee plus a refundable crank bond.
        <InfoHint>
          The fee is non-refundable; the bond pays whoever runs the settlement
          cranks, and whatever they do not spend returns when the market is
          closed.
        </InfoHint>
        {!connected && ' Connect a wallet to send it.'}
      </div>
    </div>
  );
}
