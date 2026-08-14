import { PublicKey as PublicKeyCtor, type Connection, type PublicKey } from '@solana/web3.js';

import { collateralPda, configPda } from './pda.ts';
import { readOnlyClient } from './program.ts';
import type { FeedKind, Market, MarketParams, MarketStatus, VoidCause } from './types.ts';

/**
 * Reading accounts. Anchor decodes an IDL it knows only at runtime, so its
 * namespace is untyped and its numbers arrive as `BN`; both are dealt with
 * here, once, and nothing downstream sees the wire's shape.
 */

type Decoded = Record<string, unknown>;

/** Anchor renders a Rust enum as `{ variantName: {} }`. */
const variant = <T extends string>(value: unknown, fallback: T): T => {
  if (value && typeof value === 'object') {
    const [name] = Object.keys(value as object);
    if (name) return name as T;
  }
  return fallback;
};

/** `BN`, `bigint` and `number` all reach us; all leave as `bigint`. */
const big = (value: unknown): bigint => {
  if (typeof value === 'bigint') return value;
  if (typeof value === 'number') return BigInt(value);
  if (value && typeof (value as { toString: () => string }).toString === 'function') {
    return BigInt((value as { toString: () => string }).toString());
  }
  return 0n;
};

const number = (value: unknown): number => Number(big(value));

const toParams = (raw: Decoded): MarketParams => ({
  feeBps: number(raw['feeBps']),
  feedCapBps: number(raw['feedCapBps']),
  minRampBps: number(raw['minRampBps']),
  twapWindow: number(raw['twapWindow']),
  grace: number(raw['grace']),
  skew: number(raw['skew']),
  maxSegment: number(raw['maxSegment']),
  minObservations: number(raw['minObservations']),
  creationCooldown: number(raw['creationCooldown']),
  claimWindow: number(raw['claimWindow']),
  pythWindowTolerance: number(raw['pythWindowTolerance']),
  maxConfidenceBps: number(raw['maxConfidenceBps']),
  maxDownSlotsRatio: number(raw['maxDownSlotsRatio']),
  keeperReward: big(raw['keeperReward']),
  creationFee: big(raw['creationFee']),
});

const toMarket = (raw: Decoded): Market => ({
  status: variant<MarketStatus>(raw['status'], 'created'),
  marketId: Uint8Array.from((raw['marketId'] as number[]) ?? []),
  creator: raw['creator'] as PublicKey,
  collateralMint: raw['collateralMint'] as PublicKey,
  yesMint: raw['yesMint'] as PublicKey,
  noMint: raw['noMint'] as PublicKey,
  vault: raw['vault'] as PublicKey,
  createdAt: big(raw['createdAt']),
  openAt: big(raw['openAt']),
  settleAt: big(raw['settleAt']),
  params: toParams((raw['params'] ?? {}) as Decoded),
  capPerSide: big(raw['capPerSide']),
  strike: big(raw['strike']),
  rampBps: number(raw['rampBps']),
  stakedYes: big(raw['stakedYes']),
  stakedNo: big(raw['stakedNo']),
  statusReason: variant<VoidCause>(raw['statusReason'], 'none'),
  share: big(raw['share']),
  poolYes: big(raw['poolYes']),
  poolNo: big(raw['poolNo']),
  feeTotal: big(raw['feeTotal']),
  resolvedAt: big(raw['resolvedAt']),
});

export interface MarketRecord {
  address: PublicKey;
  account: Market;
}

/**
 * Eight bytes of discriminator, then `bump`, then `status`. Verified against
 * live accounts: a wrong offset filters on the wrong field and returns nothing.
 */
const STATUS_OFFSET = 9;

/** The order the on-chain enum declares, which is the byte it serialises to. */
const STATUS_BYTE: Record<MarketStatus, number> = {
  created: 0,
  open: 1,
  locked: 2,
  snapshotted: 3,
  resolved: 4,
  void: 5,
};

const BASE58 = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';

/** Base58 of one byte. Every status fits well inside the alphabet. */
const base58Byte = (value: number): string => BASE58[value]!;

/**
 * Markets, optionally narrowed by status. Unnarrowed this is one
 * `getProgramAccounts`: fine for tens, ruinous for tens of thousands, and there
 * is no pagination to soften it — a deployment that outgrows it wants an
 * indexer. Narrowing costs a request per status and lets the node discard the rest.
 */
export const fetchMarkets = async (
  connection: Connection,
  statuses?: MarketStatus[],
): Promise<MarketRecord[]> => {
  const program = readOnlyClient(connection);
  const namespace = (program.account as Decoded)['market'] as {
    all: (
      filters?: Array<{ memcmp: { offset: number; bytes: string } }>,
    ) => Promise<Array<{ publicKey: PublicKey; account: Decoded }>>;
  };

  const decode = (entries: Array<{ publicKey: PublicKey; account: Decoded }>) =>
    entries.map((entry) => ({
      address: entry.publicKey,
      account: toMarket(entry.account),
    }));

  if (!statuses || statuses.length === 0) return decode(await namespace.all());

  const batches = await Promise.all(
    statuses.map((status) =>
      namespace.all([
        { memcmp: { offset: STATUS_OFFSET, bytes: base58Byte(STATUS_BYTE[status]) } },
      ]),
    ),
  );
  return decode(batches.flat());
};

export const fetchMarket = async (
  connection: Connection,
  address: PublicKey,
): Promise<Market> => {
  const program = readOnlyClient(connection);
  const namespace = (program.account as Decoded)['market'] as {
    fetch: (address: PublicKey) => Promise<Decoded>;
  };
  return toMarket(await namespace.fetch(address));
};

export interface Config {
  authority: PublicKey;
  treasury: PublicKey;
  paused: boolean;
  params: MarketParams;
  timelock: number;
  /** A queued parameter change, readable while it waits out the timelock. */
  pending: { params: MarketParams; effectiveAt: number } | null;
}

export const fetchConfig = async (connection: Connection): Promise<Config> => {
  const program = readOnlyClient(connection);
  const namespace = (program.account as Decoded)['config'] as {
    fetch: (address: PublicKey) => Promise<Decoded>;
  };
  const raw = await namespace.fetch(configPda());
  return {
    authority: raw['authority'] as PublicKey,
    treasury: raw['treasury'] as PublicKey,
    paused: Boolean(raw['paused']),
    params: toParams((raw['params'] ?? {}) as Decoded),
    timelock: number(raw['timelock']),
    pending: raw['hasPending']
      ? {
          params: toParams((raw['pendingParams'] ?? {}) as Decoded),
          effectiveAt: number(raw['pendingEffectiveAt']),
        }
      : null,
  };
};

export interface FeedRecord {
  address: PublicKey;
  kind: FeedKind;
  enabled: boolean;
  effectiveAt: bigint;
  depthQuote: bigint;
  label: string;
  /** The source id read as an address. For a pool feed this is its ring. */
  sourceAddress: PublicKey;
  /** The pool the ring belongs to. Zero for an oracle feed, which has none. */
  pool: PublicKey;
}

const toFeed = (address: PublicKey, raw: Decoded): FeedRecord => ({
  address,
  kind: variant<FeedKind>(raw['kind'], 'raydiumClmm'),
  enabled: Boolean(raw['enabled']),
  effectiveAt: big(raw['effectiveAt']),
  depthQuote: big(raw['depthQuote']),
  label: new TextDecoder()
    .decode(Uint8Array.from((raw['label'] as number[]) ?? []))
    .replace(/\0+$/, ''),
  sourceAddress: new PublicKeyCtor(Uint8Array.from((raw['sourceId'] as number[]) ?? [])),
  pool: raw['pool'] as PublicKey,
});

/** The price sources governance has admitted. */
export const fetchFeeds = async (connection: Connection): Promise<FeedRecord[]> => {
  const program = readOnlyClient(connection);
  const namespace = (program.account as Decoded)['feed'] as {
    all: () => Promise<Array<{ publicKey: PublicKey; account: Decoded }>>;
  };
  const entries = await namespace.all();
  return entries.map((entry) => toFeed(entry.publicKey, entry.account));
};

export const fetchFeed = async (
  connection: Connection,
  address: PublicKey,
): Promise<FeedRecord> => {
  const program = readOnlyClient(connection);
  const namespace = (program.account as Decoded)['feed'] as {
    fetch: (address: PublicKey) => Promise<Decoded>;
  };
  return toFeed(address, await namespace.fetch(address));
};

/** Several feeds in one request; one at a time is a round trip each. */
export const fetchFeedsByAddress = async (
  connection: Connection,
  addresses: PublicKey[],
): Promise<FeedRecord[]> => {
  if (addresses.length === 0) return [];
  const program = readOnlyClient(connection);
  const namespace = (program.account as Decoded)['feed'] as {
    fetchMultiple: (addresses: PublicKey[]) => Promise<Array<Decoded | null>>;
  };
  const raw = await namespace.fetchMultiple(addresses);
  return raw.flatMap((entry, index) =>
    entry ? [toFeed(addresses[index]!, entry)] : [],
  );
};

export interface CollateralRecord {
  address: PublicKey;
  mint: PublicKey;
  decimals: number;
  enabled: boolean;
  minStake: bigint;
}

/** One approved mint, by the mint it takes. */
export const fetchCollateral = async (
  connection: Connection,
  mint: PublicKey,
): Promise<CollateralRecord | null> => {
  const program = readOnlyClient(connection);
  const namespace = (program.account as Decoded)['collateral'] as {
    fetchNullable: (address: PublicKey) => Promise<Decoded | null>;
  };
  const address = collateralPda(mint);
  const raw = await namespace.fetchNullable(address);
  return raw && {
    address,
    mint: raw['mint'] as PublicKey,
    decimals: number(raw['decimals']),
    enabled: Boolean(raw['enabled']),
    minStake: big(raw['minStake']),
  };
};

/** The mints governance has approved as stake. */
export const fetchCollaterals = async (
  connection: Connection,
): Promise<CollateralRecord[]> => {
  const program = readOnlyClient(connection);
  const namespace = (program.account as Decoded)['collateral'] as {
    all: () => Promise<Array<{ publicKey: PublicKey; account: Decoded }>>;
  };
  const entries = await namespace.all();
  return entries.map((entry) => ({
    address: entry.publicKey,
    mint: entry.account['mint'] as PublicKey,
    decimals: number(entry.account['decimals']),
    enabled: Boolean(entry.account['enabled']),
    minStake: big(entry.account['minStake']),
  }));
};
