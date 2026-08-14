import type { ClmmPool } from '@prediction-market/sdk';
import { NATIVE_MINT } from '@solana/spl-token';
import { useWallet } from '@solana/wallet-adapter-react';
import { useCallback, useEffect, useState } from 'react';

import { useAnchorProvider } from '@/components/solana-providers';
import { IS_MAINNET, RAYDIUM_CLMM } from '@/lib/cluster';
import { describeFailure, ExplainedError, type ActionFailure } from '@/lib/errors';

/**
 * Trading through the pools a market reads. The swaps are real; nothing here
 * writes a price anywhere.
 *
 * `up` means the token's price in SOL rises, which is how a person reads it.
 * Whether that runs with or against a pool's own quoting depends on which mint
 * Raydium put first, so it is derived rather than assumed — and it needs no
 * market, which is what lets the same control serve a page with no market yet.
 */
export function useNudge(pools: ClmmPool[]) {
  const provider = useAnchorProvider();
  const { publicKey } = useWallet();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<ActionFailure | null>(null);

  // Fetched while the page is idle rather than on the click. It is a third of a
  // megabyte, and downloading it between the press and the wallet is time the
  // reader spends looking at a button that has visibly done nothing.
  useEffect(() => {
    if (IS_MAINNET) return;
    void import('@prediction-market/sdk/swap');
  }, []);

  const nudge = useCallback(
    async (direction: 'up' | 'down', sol: number) => {
      if (IS_MAINNET) throw new ExplainedError('Price nudging exists on test networks only.');
      if (!provider || !publicKey) throw new ExplainedError('Connect a wallet first.');
      if (!provider.sendAll) throw new ExplainedError('This wallet cannot sign a batch of transactions.');
      if (pools.length === 0) throw new ExplainedError('The price sources have not loaded yet.');

      setPending(true);
      setError(null);
      try {
        const { connection } = provider;

        // Loaded here, not imported: Raydium's client is a third of a megabyte
        // and this is a test-network button. Nobody who leaves it alone pays for it.
        const { buildNudge, mintToSpend } = await import('@prediction-market/sdk/swap');

        // One transaction per pool. Three swaps do not fit in one -- fifteen
        // accounts each against a 1232-byte message -- so they are signed
        // together instead, which the wallet shows as a single approval.
        //
        // Built at once rather than in turn: each needs a round trip, and they
        // do not depend on one another, so waiting for them in sequence only
        // adds delay before the wallet is asked.
        const batch = await Promise.all(
          pools.map(async (pool) => ({
            tx: await buildNudge({
              connection,
              programId: RAYDIUM_CLMM,
              pool,
              payer: publicKey,
              inputMint: mintToSpend(
                pool,
                (direction === 'up') !== pool.mint0.equals(NATIVE_MINT),
              ),
              sol,
            }),
          })),
        );

        return await provider.sendAll(batch);
      } catch (cause) {
        setError(describeFailure(cause));
        throw cause;
      } finally {
        setPending(false);
      }
    },
    [provider, publicKey, pools],
  );

  return { nudge, pending, error, connected: Boolean(publicKey) };
}
