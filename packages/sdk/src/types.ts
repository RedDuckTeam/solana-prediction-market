import type { PublicKey } from '@solana/web3.js';

/** Where a market is in its life. Mirrors the on-chain enum. */
export type MarketStatus =
  | 'created'
  | 'open'
  | 'locked'
  | 'snapshotted'
  | 'resolved'
  | 'void';

/** Why a market was abandoned. */
export type VoidCause =
  | 'none'
  | 'snapshotMissed'
  | 'emptySide'
  | 'predicateAborted';

export type FeedKind = 'raydiumClmm' | 'pythTwap';

export interface MarketParams {
  feeBps: number;
  feedCapBps: number;
  minRampBps: number;
  twapWindow: number;
  grace: number;
  skew: number;
  maxSegment: number;
  minObservations: number;
  creationCooldown: number;
  claimWindow: number;
  pythWindowTolerance: number;
  maxConfidenceBps: number;
  maxDownSlotsRatio: number;
  keeperReward: bigint;
  creationFee: bigint;
}

export interface Market {
  status: MarketStatus;
  marketId: Uint8Array;
  creator: PublicKey;
  collateralMint: PublicKey;
  yesMint: PublicKey;
  noMint: PublicKey;
  vault: PublicKey;
  createdAt: bigint;
  openAt: bigint;
  settleAt: bigint;
  params: MarketParams;
  capPerSide: bigint;
  /** The value the predicate's score is compared against, Q64.64. */
  strike: bigint;
  /** Half-width of the settlement band, in basis points of the strike. */
  rampBps: number;
  stakedYes: bigint;
  stakedNo: bigint;
  statusReason: VoidCause;
  /** Fraction of the pot owed to YES, Q64.64, fixed at resolution. */
  share: bigint;
  poolYes: bigint;
  poolNo: bigint;
  feeTotal: bigint;
  resolvedAt: bigint;
}

/** The timetable a market runs to, derived from its parameters. */
export interface Schedule {
  /** When betting opens. */
  openAt: number;
  /** When betting closes — before the measured window, not at settlement. */
  lockAt: number;
  /** Start of the averaging window. */
  windowStart: number;
  /** The instant the market settles at. */
  settleAt: number;
  /** Last moment a snapshot may be taken. */
  graceEnd: number;
}

export const scheduleOf = (market: Market): Schedule => {
  const settleAt = Number(market.settleAt);
  const windowStart = settleAt - market.params.twapWindow;
  return {
    openAt: Number(market.openAt),
    lockAt: windowStart - market.params.skew,
    windowStart,
    settleAt,
    graceEnd: settleAt + market.params.grace,
  };
};

/**
 * What a market is currently accepting, derived from the clock rather than the
 * stored status: the status only advances when someone sends a transaction, so
 * a market can be past its deadline and still say `open`.
 */
export const phaseOf = (market: Market, now: number): MarketStatus => {
  if (market.status === 'resolved' || market.status === 'void') return market.status;
  if (market.status === 'snapshotted') return 'snapshotted';
  const schedule = scheduleOf(market);
  if (now < schedule.openAt) return 'created';
  if (now < schedule.lockAt) return 'open';
  return 'locked';
};

export const acceptsStakes = (market: Market, now: number): boolean =>
  phaseOf(market, now) === 'open';

/**
 * The step a market is waiting for, if any. `void` outranks `snapshot`: an
 * empty side has no counterparty to settle between, whatever the clock says.
 */
export type SettlementStep = 'snapshot' | 'resolve' | 'void' | null;

export const settlementStep = (market: Market, now: number): SettlementStep => {
  const phase = phaseOf(market, now);
  if (phase === 'resolved' || phase === 'void') return null;
  if (phase === 'snapshotted') return 'resolve';

  const schedule = scheduleOf(market);
  if (now >= schedule.lockAt && (market.stakedYes === 0n || market.stakedNo === 0n)) {
    return 'void';
  }
  if (now > schedule.graceEnd) return 'void';
  if (now >= schedule.settleAt) return 'snapshot';
  return null;
};
