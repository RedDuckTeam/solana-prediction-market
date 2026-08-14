import { ExternalLink } from 'lucide-react';

import { InfoHint } from '@/components/ui/info-hint';
import { CLUSTER, FAUCET_URL, IS_MAINNET } from '@/lib/cluster';

/**
 * A market that looks the same on a test network and a real one is a trap, so
 * the difference is stated outright rather than left to a badge. One line —
 * where the prices come from lives behind the hint.
 */
export function DemoBanner() {
  if (IS_MAINNET) return null;

  return (
    <div className="flex flex-wrap items-center gap-1.5 rounded-lg border border-dashed bg-muted/40 px-4 py-3 text-sm">
      <span className="font-medium">Demo on {CLUSTER} — nothing here is money.</span>
      <span className="text-muted-foreground">
        Any Solana wallet works
        {FAUCET_URL && (
          <>
            {'; test SOL from the '}
            <a
              className="inline-flex items-center gap-1 underline underline-offset-4 hover:text-foreground"
              href={FAUCET_URL}
              target="_blank"
              rel="noreferrer"
            >
              faucet <ExternalLink className="size-3" />
            </a>
          </>
        )}
        .
      </span>
      <InfoHint>
        The prices are real: genuine Raydium pools, which record a price
        whenever somebody trades through them. Nothing here writes a price —
        what {CLUSTER} lacks is traders, so a market page hands you the trades
        to make yourself, and the pool records them exactly as it would anyone
        else&rsquo;s.
      </InfoHint>
    </div>
  );
}
