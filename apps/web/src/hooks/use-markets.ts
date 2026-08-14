import { fetchMarket, fetchMarkets, type Market, type MarketRecord } from '@prediction-market/sdk';
import { useConnection } from '@solana/wallet-adapter-react';
import { PublicKey, type Connection } from '@solana/web3.js';
import { useCallback, useEffect, useRef, useState } from 'react';

interface Loadable<T> {
  data: T | null;
  loading: boolean;
  error: string | null;
  reload: () => void;
}

/**
 * Whether two decoded accounts hold the same values. Background polls mostly
 * read back what is already on screen, and handing React a fresh object for
 * unchanged data re-renders every panel and recomputes every memo for nothing
 * — so an unchanged read keeps the object it already had.
 *
 * Bigints and public keys both stringify predictably, which makes equality
 * one string compare.
 */
const encoded = (value: unknown): string =>
  JSON.stringify(value, (_key, entry: unknown) =>
    typeof entry === 'bigint'
      ? entry.toString()
      : entry instanceof PublicKey
        ? entry.toBase58()
        : entry,
  );

/**
 * `getProgramAccounts` walks every account the program owns and is the first
 * call a public endpoint refuses, so opening a market and pressing back must
 * not repeat it. A market's own page reads that market directly regardless.
 */
const LIST_TTL_MS = 20_000;

let cached: { endpoint: string; at: number; markets: MarketRecord[] } | null = null;

/** A reload asked for by hand always reaches the chain. */
let lastFetchedNonce = -1;

/**
 * Unfiltered and unpaginated on purpose: a deployment that outgrows one fetch
 * wants an indexer, and hiding that here would only hide it from a fork.
 */
export function useMarkets(): Loadable<MarketRecord[]> {
  const { connection } = useConnection();
  const [data, setData] = useState<MarketRecord[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [nonce, setNonce] = useState(0);
  const shown = useRef(false);

  useEffect(() => {
    let cancelled = false;
    // The skeleton appears once. A reload refreshes behind the list that is
    // already showing, rather than tearing it down to show placeholders.
    if (!shown.current) setLoading(true);
    (async () => {
      try {
        const fresh =
          cached &&
          cached.endpoint === connection.rpcEndpoint &&
          Date.now() - cached.at < LIST_TTL_MS &&
          nonce === lastFetchedNonce;
        const found = fresh
          ? cached!.markets
          : await fetchMarkets(connection);
        if (cancelled) return;
        cached = { endpoint: connection.rpcEndpoint, at: Date.now(), markets: found };
        lastFetchedNonce = nonce;
        shown.current = true;
        setData((current) => (current && encoded(current) === encoded(found) ? current : found));
        setError(null);
      } catch (cause) {
        if (!cancelled) setError(cause instanceof Error ? cause.message : String(cause));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [connection, nonce]);

  return { data, loading, error, reload: useCallback(() => setNonce((n) => n + 1), []) };
}

/**
 * How often a market that is still moving is read again.
 *
 * Settlement is permissionless, so a market changes underneath whoever is
 * looking at it: somebody else records the prices, and a page that has not
 * noticed goes on offering a button whose transaction can only fail. Markets
 * that have resolved or voided never change again and are left alone.
 */
const MARKET_POLL_MS = 20_000;

export function useMarket(address: string | null): Loadable<MarketRecord> {
  const { connection } = useConnection();
  const [data, setData] = useState<MarketRecord | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [nonce, setNonce] = useState(0);
  const shownFor = useRef<string | null>(null);

  useEffect(() => {
    if (!address) return;
    let cancelled = false;
    // Only a navigation earns the skeleton. A background poll must never
    // flip this page back to placeholders: doing so unmounts every panel,
    // and with them whatever the user was in the middle of typing.
    if (shownFor.current !== address) {
      shownFor.current = address;
      setData(null);
      setLoading(true);
    }
    (async () => {
      try {
        const key = new PublicKey(address);
        const account = await fetchMarket(connection, key);
        if (cancelled) return;
        setData((current) =>
          current && current.address.equals(key) && encoded(current.account) === encoded(account)
            ? current
            : { address: key, account },
        );
        setError(null);
      } catch (cause) {
        if (!cancelled) setError(cause instanceof Error ? cause.message : String(cause));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [address, connection, nonce]);

  const final = data?.account.status === 'resolved' || data?.account.status === 'void';
  useEffect(() => {
    if (!address || final) return;
    const timer = setInterval(() => setNonce((n) => n + 1), MARKET_POLL_MS);
    return () => clearInterval(timer);
  }, [address, final]);

  return { data, loading, error, reload: useCallback(() => setNonce((n) => n + 1), []) };
}

/**
 * The chain clock, shared by every subscriber on the page. One sync a minute
 * corrects drift; between syncs the clock advances locally, so a countdown
 * does not stutter and the chain is not asked sixty times for it.
 */
const CLOCK_SYNC_MS = 60_000;

let clock: { endpoint: string; base: number; readAt: number } | null = null;
let clockSync: Promise<void> | null = null;

const syncClock = (connection: Connection): Promise<void> => {
  clockSync ??= (async () => {
    try {
      const slot = await connection.getSlot('confirmed');
      const chainTime = await connection.getBlockTime(slot);
      if (chainTime) {
        clock = { endpoint: connection.rpcEndpoint, base: chainTime, readAt: Date.now() };
      }
    } catch {
      /* a missed sync is not worth surfacing; the next one corrects it */
    } finally {
      clockSync = null;
    }
  })();
  return clockSync;
};

const clockTime = (endpoint: string): number | null =>
  clock && clock.endpoint === endpoint
    ? clock.base + Math.round((Date.now() - clock.readAt) / 1000)
    : null;

/**
 * The chain's clock, not the browser's: every deadline is read from the former,
 * and a countdown off the latter says "open" seconds after it is not.
 *
 * `stepSeconds` is how often the caller is willing to be re-rendered. A
 * countdown needs every second; a phase gate changes its answer a handful of
 * times in a market's whole life, and a page that re-renders every second is
 * redrawing under whatever the user is typing. The clock itself stays exact —
 * only what this hook *returns* is quantised, so two subscribers at different
 * steps never disagree about what time it is.
 */
const quantise = (time: number | null, stepSeconds: number): number | null =>
  time === null ? null : time - (time % stepSeconds);

export function useChainTime(stepSeconds = 1): number | null {
  const { connection } = useConnection();
  const [visible, setVisible] = useState<number | null>(() =>
    quantise(clockTime(connection.rpcEndpoint), stepSeconds),
  );

  useEffect(() => {
    const show = () => {
      const time = quantise(clockTime(connection.rpcEndpoint), stepSeconds);
      if (time === null) return;
      // Same value, no state change, no re-render: this is the whole point.
      setVisible((current) => (current === time ? current : time));
    };
    const tick = () => {
      const stale =
        !clock ||
        clock.endpoint !== connection.rpcEndpoint ||
        Date.now() - clock.readAt >= CLOCK_SYNC_MS;
      if (stale) void syncClock(connection).then(show);
      else show();
    };
    tick();
    const timer = setInterval(tick, 1_000);
    return () => clearInterval(timer);
  }, [connection, stepSeconds]);

  return visible;
}

export type { Market, MarketRecord };
