import { PublicKey } from '@solana/web3.js';

export type Cluster = 'localnet' | 'devnet' | 'mainnet-beta';

export const CLUSTER: Cluster =
  (import.meta.env['VITE_CLUSTER'] as Cluster | undefined) ?? 'devnet';

/**
 * A path, not a host: the worker serving this page answers JSON-RPC there and
 * adds the upstream key itself, so no key is compiled in. A full URL in
 * `VITE_RPC_URL` overrides it and bypasses the worker entirely.
 */
const SAME_ORIGIN_RPC = '/rpc';

const configured = (import.meta.env['VITE_RPC_URL'] as string | undefined) ?? SAME_ORIGIN_RPC;

export const ENDPOINT: string = configured.startsWith('/')
  ? new URL(configured, globalThis.location?.origin ?? 'http://localhost').toString()
  : configured;

/**
 * Derived here because the library shifts the port by one when the endpoint
 * names one — right for a bare validator, wrong behind anything serving both.
 */
export const WS_ENDPOINT: string = ENDPOINT.replace(/^http/, 'ws');

/** Markets hold real money on exactly one of these. */
export const IS_MAINNET = CLUSTER === 'mainnet-beta';

/** Devnet's address is not mainnet's, and devnet carries an older one besides. */
export const RAYDIUM_CLMM = new PublicKey(
  CLUSTER === 'mainnet-beta'
    ? 'CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK'
    : 'DRayAUgENGQBKVaX8owNhgzkEDyoHTGVEGHVJT1E9pfH',
);

/** Where a visitor gets tokens to try this with. */
export const FAUCET_URL =
  CLUSTER === 'devnet' ? 'https://faucet.solana.com/' : null;

export const explorerUrl = (address: string): string => {
  const suffix =
    CLUSTER === 'mainnet-beta'
      ? ''
      : CLUSTER === 'devnet'
        ? '?cluster=devnet'
        : '?cluster=custom&customUrl=' + encodeURIComponent(ENDPOINT);
  return `https://explorer.solana.com/address/${address}${suffix}`;
};
