/**
 * Common to the devnet scripts. These hold the governance key, so nothing here
 * belongs near a real deployment.
 */
import { AnchorProvider, BN, Wallet } from '@anchor-lang/core';
import { client as marketClient } from '@prediction-market/sdk';
import { Connection, Keypair, PublicKey } from '@solana/web3.js';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '../../..');

/** Where `pools.ts` records what it created, for the later steps to read. */
export const poolsFile = resolve(repoRoot, '.keys/devnet-pools.json');

/**
 * The endpoint these scripts use. Named as it is everywhere else in the repo —
 * the worker's secret, the dev server's proxy — so there is one thing to set.
 * They are a separate client from the front end and reach the chain directly.
 */
export const RPC = process.env['UPSTREAM_RPC_URL'] ?? 'https://api.devnet.solana.com';

/**
 * Not mainnet's address, and not the only one on devnet — an older deployment
 * still holds thousands of pools. This is the one their current SDK targets.
 */
export const RAYDIUM_CLMM = new PublicKey('DRayAUgENGQBKVaX8owNhgzkEDyoHTGVEGHVJT1E9pfH');

/** Wrapped SOL: the one mint a devnet visitor can obtain from a faucet. */
export const WSOL = new PublicKey('So11111111111111111111111111111111111111112');

export interface PoolRecord {
  pool: string;
  observation: string;
  fee: string;
  spacing: number;
}

export const readPools = (): { token: string; quote: string; pools: PoolRecord[] } =>
  JSON.parse(readFileSync(poolsFile, 'utf8')) as {
    token: string;
    quote: string;
    pools: PoolRecord[];
  };

export const walletKeypair = (): Keypair => {
  const path = process.env['DEPLOYER_KEYPAIR'] ?? resolve(repoRoot, '.keys/deployer.json');
  const bytes = Uint8Array.from(JSON.parse(readFileSync(path, 'utf8')) as number[]);
  return Keypair.fromSecretKey(bytes);
};

export const connect = () => {
  const connection = new Connection(RPC, 'confirmed');
  const keypair = walletKeypair();
  const provider = new AnchorProvider(connection, new Wallet(keypair), {
    commitment: 'confirmed',
  });
  return { connection, keypair, provider };
};

export const client = (provider: AnchorProvider) => marketClient(provider);

/**
 * The governance parameters the demo runs under, shared by `seed` (which sets
 * them at initialisation) and `params` (which proposes them onto a deployment
 * that already exists).
 *
 * As much room as the protocol allows: `twap_window + grace` may not exceed
 * 1200 seconds, so ten minutes each. `max_segment` sits at the window and
 * `min_observations` at zero, which asks for no trade *inside* the window --
 * one trade after it closes is then enough to settle from. A live deployment
 * with traders would set both tighter and never notice.
 */
export const demoParams = () => ({
  feeBps: 100,
  feedCapBps: 500,
  minRampBps: 25,
  twapWindow: 600,
  grace: 600,
  skew: 60,
  maxSegment: 600,
  minObservations: 0,
  creationCooldown: 60,
  claimWindow: 120 * 86_400,
  pythWindowTolerance: 5,
  maxConfidenceBps: 100,
  maxDownSlotsRatio: 50_000,
  // Must cover the rent of the Snapshot account the keeper fronts (about
  // 0.009 SOL) with margin -- the program refuses anything lower.
  keeperReward: new BN(12_000_000),
  creationFee: new BN(10_000_000),
});

export const log = (...parts: unknown[]) => console.log(...parts);
