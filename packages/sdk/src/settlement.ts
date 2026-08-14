import type { Connection, PublicKey } from '@solana/web3.js';

import { fetchFeed } from './accounts.ts';
import { specPda } from './pda.ts';
import { readOnlyClient } from './program.ts';
import type { FeedKind } from './types.ts';

/** One price input of a market, as the spec declares it. */
export interface SpecFeed {
  feed: PublicKey;
  invert: boolean;
}

export interface MarketSpec {
  market: PublicKey;
  feeds: SpecFeed[];
  bytecode: Uint8Array;
  rulesUri: string;
}

export const fetchSpec = async (
  connection: Connection,
  market: PublicKey,
): Promise<MarketSpec> => {
  const program = readOnlyClient(connection);
  const namespace = (program.account as Record<string, unknown>)['marketSpec'] as {
    fetch: (address: PublicKey) => Promise<Record<string, unknown>>;
  };
  const raw = await namespace.fetch(specPda(market));
  return {
    market: raw['market'] as PublicKey,
    feeds: (raw['feeds'] as Array<{ feed: PublicKey; invert: boolean }>).map((entry) => ({
      feed: entry.feed,
      invert: entry.invert,
    })),
    bytecode: Uint8Array.from((raw['bytecode'] as number[]) ?? []),
    rulesUri: (raw['rulesUri'] as string) ?? '',
  };
};

/**
 * `snapshot` takes, per declared feed and in order, the `Feed` account followed
 * by the account its price is read from.
 */
export interface SettlementAccount {
  pubkey: PublicKey;
  isSigner: false;
  isWritable: false;
}

export interface SettlementAccounts {
  accounts: SettlementAccount[];
  /** Feeds whose price account has to be posted before this will succeed. */
  unposted: Array<{ feed: PublicKey; kind: FeedKind }>;
}

const readOnly = (pubkey: PublicKey): SettlementAccount => ({
  pubkey,
  isSigner: false,
  isWritable: false,
});

/**
 * A pool feed's ring sits at a fixed address, which is the feed's own source id,
 * so it derives offline. A Pyth feed's account does not exist until someone
 * posts an update for the window, so it can only be supplied by the caller.
 */
export const settlementAccounts = async (
  connection: Connection,
  spec: MarketSpec,
  posted: Map<string, PublicKey> = new Map(),
): Promise<SettlementAccounts> => {
  const accounts: SettlementAccount[] = [];
  const unposted: Array<{ feed: PublicKey; kind: FeedKind }> = [];

  for (const declared of spec.feeds) {
    const feed = await fetchFeed(connection, declared.feed);
    accounts.push(readOnly(declared.feed));

    if (feed.kind === 'raydiumClmm') {
      accounts.push(readOnly(feed.sourceAddress));
      continue;
    }
    const update = posted.get(declared.feed.toBase58());
    if (!update) {
      unposted.push({ feed: declared.feed, kind: feed.kind });
      continue;
    }
    accounts.push(readOnly(update));
  }

  return { accounts, unposted };
};
