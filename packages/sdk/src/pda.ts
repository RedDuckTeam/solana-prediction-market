import { PublicKey } from '@solana/web3.js';

import { PROGRAM_ID } from './program.ts';

/**
 * PDA seeds, mirroring `programs/prediction-market/src/constants.rs`. A wrong
 * seed derives a valid-looking address for an account that will never exist, so
 * these strings are written once. `TextEncoder`, not `Buffer`, which is Node's.
 */
const utf8 = (text: string): Uint8Array => new TextEncoder().encode(text);

export const SEEDS = {
  config: utf8('config'),
  collateral: utf8('collateral'),
  feed: utf8('feed'),
  market: utf8('market'),
  spec: utf8('spec'),
  snapshot: utf8('snapshot'),
  vault: utf8('vault'),
  yesMint: utf8('yes'),
  noMint: utf8('no'),
} as const;

const derive = (seeds: Uint8Array[]): PublicKey =>
  PublicKey.findProgramAddressSync(seeds, PROGRAM_ID)[0];

export const configPda = (): PublicKey => derive([SEEDS.config]);

export const collateralPda = (mint: PublicKey): PublicKey =>
  derive([SEEDS.collateral, mint.toBytes()]);

/**
 * A feed is seeded by its source identifier, whatever kind of source it is:
 * an observation ring's address for a pool, an instrument id for an oracle.
 */
export const feedPda = (sourceId: PublicKey | Uint8Array): PublicKey =>
  derive([SEEDS.feed, sourceId instanceof PublicKey ? sourceId.toBytes() : sourceId]);

export const marketPda = (marketId: Uint8Array): PublicKey =>
  derive([SEEDS.market, marketId]);

export const specPda = (market: PublicKey): PublicKey =>
  derive([SEEDS.spec, market.toBytes()]);

export const snapshotPda = (market: PublicKey): PublicKey =>
  derive([SEEDS.snapshot, market.toBytes()]);

export const vaultPda = (market: PublicKey): PublicKey =>
  derive([SEEDS.vault, market.toBytes()]);

export const yesMintPda = (market: PublicKey): PublicKey =>
  derive([SEEDS.yesMint, market.toBytes()]);

export const noMintPda = (market: PublicKey): PublicKey =>
  derive([SEEDS.noMint, market.toBytes()]);

/** Every address a market owns, derived from its identifier. */
export const marketAddresses = (marketId: Uint8Array) => {
  const market = marketPda(marketId);
  return {
    market,
    spec: specPda(market),
    snapshot: snapshotPda(market),
    vault: vaultPda(market),
    yesMint: yesMintPda(market),
    noMint: noMintPda(market),
  };
};
