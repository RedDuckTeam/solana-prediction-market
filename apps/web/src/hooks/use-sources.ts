import {
  fetchFeedsByAddress,
  fetchSpec,
  readClmmPools,
  type ClmmPool,
  type FeedRecord,
} from '@prediction-market/sdk';
import { useConnection } from '@solana/wallet-adapter-react';
import type { AccountInfo, PublicKey } from '@solana/web3.js';
import { useCallback, useEffect, useState } from 'react';

/** One declared source: the feed, the pool it names, and that pool's ring. */
export interface MarketSource {
  declaredInvert: boolean;
  feed: FeedRecord;
  pool: ClmmPool;
  /** The pool's observation ring, or null if it has never been written. */
  ring: AccountInfo<Buffer> | null;
}

export interface MarketSources {
  sources: MarketSource[];
  loading: boolean;
  refresh: () => void;
}

/**
 * A market's sources, read once and shared by the three things that ask: the
 * live price, the window's coverage, and a swap. Read separately they were the
 * same accounts three times over, one round trip each.
 */
export function useMarketSources(address: PublicKey | null): MarketSources {
  const { connection } = useConnection();
  const [sources, setSources] = useState<MarketSource[]>([]);
  const [loading, setLoading] = useState(false);
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    if (!address) return;
    let cancelled = false;
    setLoading(true);

    (async () => {
      try {
        const spec = await fetchSpec(connection, address);
        const declared = spec.feeds;
        const feeds = await fetchFeedsByAddress(
          connection,
          declared.map((entry) => entry.feed),
        );
        const [pools, rings] = await Promise.all([
          readClmmPools(connection, feeds.map((feed) => feed.pool)),
          connection.getMultipleAccountsInfo(feeds.map((feed) => feed.sourceAddress)),
        ]);

        // A source is only usable when every part of it read back; a partial one
        // would make the page answer a question it cannot actually answer.
        const found = feeds.flatMap((feed, index) => {
          const pool = pools[index];
          const entry = declared[index];
          return pool && entry
            ? [{ declaredInvert: entry.invert, feed, pool, ring: rings[index] ?? null }]
            : [];
        });
        if (!cancelled) setSources(found);
      } catch {
        // A read that failed says nothing; the next refresh will do.
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [connection, address?.toBase58(), nonce]);

  return {
    sources,
    loading,
    refresh: useCallback(() => setNonce((value) => value + 1), []),
  };
}
