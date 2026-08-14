import { phaseOf, scheduleOf, toSignificantString } from "@prediction-market/sdk";
import { ExternalLink } from "lucide-react";
import { useParams } from "react-router";

import { LifecyclePanel } from "@/components/lifecycle-panel";
import { LiveStatus } from "@/components/market-status";
import { PriceControls } from "@/components/price-controls";
import { SideSplit } from "@/components/side-split";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { InfoHint } from "@/components/ui/info-hint";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import { useChainTime, useMarket } from "@/hooks/use-markets";
import { useWindowCoverage } from "@/hooks/use-coverage";
import { useMarketSources } from "@/hooks/use-sources";
import { spotPrice } from "@/lib/spot-price";
import { useMintDecimals } from "@/hooks/use-mint";
import { explorerUrl } from "@/lib/cluster";
import {
  formatAmount,
  formatInstant,
  formatShare,
  shortAddress,
} from "@/lib/format";

export function MarketRoute() {
  const { market: addressParam } = useParams();
  const { data, loading, error, reload } = useMarket(addressParam ?? null);
  // Coarse on purpose. This clock only gates phases — which panel shows,
  // which buttons are live — and those answers change a handful of times in a
  // market's life. At one second the whole page would re-render under the
  // user's typing; the second-accurate countdown lives in `LiveStatus`,
  // which keeps its own clock.
  const now = useChainTime(15);
  const decimals = useMintDecimals(data?.account.collateralMint ?? null);
  const priceSources = useMarketSources(data?.address ?? null);
  const spot = spotPrice(priceSources.sources);
  const coverage = useWindowCoverage(data?.account ?? null, priceSources.sources);

  // Placeholders only while there is nothing to show. A background poll never
  // reaches this branch: swapping a live page for a skeleton unmounts every
  // panel, and with them whatever was being typed.
  if (!data) {
    if (loading) return <Skeleton className="h-96 w-full rounded-xl" />;
    return (
      <p className="rounded-md border border-destructive/40 bg-destructive/5 p-4 text-sm">
        {error ?? "Market not found."}
      </p>
    );
  }

  const { account: market, address } = data;
  const schedule = scheduleOf(market);
  const phase = now === null ? market.status : phaseOf(market, now);
  const settled = phase === "resolved" || phase === "void";
  const band = (market.rampBps / 100).toFixed(2);

  return (
    <div className="space-y-6">
      <div className="space-y-2">
        <LiveStatus market={market} />
        <h1 className="text-2xl font-semibold tracking-tight">
          Above {toSignificantString(market.strike)} at{" "}
          {formatInstant(schedule.settleAt)}
        </h1>
        <p className="text-sm text-muted-foreground">
          Settles from a {market.params.twapWindow / 60}-minute time-weighted
          price — no oracle, no reporter.
        </p>
      </div>

      <div className="grid gap-6 lg:grid-cols-[1fr_20rem]">
        <div className="space-y-6">
          <Card>
            <CardHeader>
              <CardTitle className="inline-flex items-center gap-1.5 text-sm font-medium">
                Stake
                <InfoHint>
                  How the stake currently divides — not a probability. The odds
                  are fixed only when betting closes, and can move sharply in
                  the last minutes.
                </InfoHint>
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              <SideSplit market={market} />
              <div className="grid grid-cols-2 gap-4 text-sm">
                <div>
                  <p className="text-muted-foreground">Yes</p>
                  <p className="font-medium">
                    {decimals === null
                      ? "—"
                      : formatAmount(market.stakedYes, decimals)}
                  </p>
                </div>
                <div>
                  <p className="text-muted-foreground">No</p>
                  <p className="font-medium">
                    {decimals === null
                      ? "—"
                      : formatAmount(market.stakedNo, decimals)}
                  </p>
                </div>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">
                How it settles
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-3 text-sm">
              <Row
                label="Betting closes"
                value={formatInstant(schedule.lockAt)}
              />
              <Row
                label="Measured window"
                value={`${formatInstant(schedule.windowStart)} → ${formatInstant(schedule.settleAt)}`}
                hint="Betting stops before this window opens, so nobody bets while watching the prices that settle their bet."
              />
              <Separator />
              <Row label="Strike" value={toSignificantString(market.strike)} />
              <Row
                label="Sources say now"
                value={spot === null ? "—" : toSignificantString(spot)}
                hint="The median of the pools this market reads, as they stand. Settlement uses an average over the window, not this."
              />
              <Row
                label="Settlement band"
                value={`±${band}% around the strike`}
                hint="Inside the band the pot is divided continuously instead of all-or-nothing, so nudging the price across the strike buys proportionally little."
              />
              <Row
                label="Cap per side"
                value={
                  decimals === null
                    ? "—"
                    : formatAmount(market.capPerSide, decimals)
                }
                hint="Each side is capped separately, against how expensive the thinnest price source is to move."
              />
              <Row
                label="Fee"
                value={`${market.params.feeBps / 100}% of what changes hands`}
              />
              {settled && (
                <>
                  <Separator />
                  <Row
                    label="Outcome"
                    value={
                      phase === "void"
                        ? `Voided — ${market.statusReason}, everyone refunded at par`
                        : `${formatShare(market.share)} of the pot to Yes`
                    }
                  />
                </>
              )}
            </CardContent>
          </Card>

          <a
            className="inline-flex items-center gap-1 font-mono text-xs text-muted-foreground hover:text-foreground"
            href={explorerUrl(address.toBase58())}
            target="_blank"
            rel="noreferrer"
          >
            {shortAddress(address.toBase58())}{" "}
            <ExternalLink className="size-3" />
          </a>
        </div>

        <aside className="order-first space-y-4 lg:order-none">
          <LifecyclePanel
            address={address}
            market={market}
            now={now}
            coverage={coverage}
            sources={priceSources}
            onDone={reload}
          />
          <PriceControls
            pools={priceSources.sources.map((source) => source.pool)}
            onMoved={priceSources.refresh}
          />
        </aside>
      </div>
    </div>
  );
}

function Row({
  label,
  value,
  hint,
}: {
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <div className="flex items-baseline justify-between gap-4">
      <div className="flex items-center gap-1.5 text-muted-foreground">
        {label}
        {hint && <InfoHint>{hint}</InfoHint>}
      </div>
      <span className="text-right font-medium">{value}</span>
    </div>
  );
}
