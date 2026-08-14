import { PublicKey, type Connection } from '@solana/web3.js';

/**
 * What a Raydium pool costs. Read-only and small on purpose: a page shows a
 * price on every load, and an exchange client would be most of the bundle.
 * Building a swap is `@prediction-market/sdk/swap`.
 */

/** A pool's price and the pair it prices, which is all a page displays. */
export interface ClmmPool {
  address: PublicKey;
  /** Raydium orders the pair by address, so which mint is first is its choice. */
  mint0: PublicKey;
  mint1: PublicKey;
  /** The ring this pool writes its observations to. */
  observation: PublicKey;
  decimals0: number;
  decimals1: number;
  tickCurrent: number;
  /** Price of `mint0` in `mint1`, decimals accounted for. */
  price: number;
}

/**
 * Offsets into Raydium's `PoolState`, which is `#[repr(C, packed)]`. Only for
 * display: anything that signs decodes with Raydium's own layout.
 */
const OFF = {
  mint0: 73,
  mint1: 105,
  observation: 201,
  decimals0: 233,
  decimals1: 234,
  sqrtPriceX64: 253,
  tickCurrent: 269,
} as const;

const key = (data: Uint8Array, at: number) => new PublicKey(data.subarray(at, at + 32));

export const readClmmPool = async (
  connection: Connection,
  address: PublicKey,
): Promise<ClmmPool> => {
  const [pool] = await readClmmPools(connection, [address]);
  if (!pool) throw new Error(`pool ${address.toBase58()} does not exist`);
  return pool;
};

/** Several pools in one request; separately is a round trip each. */
export const readClmmPools = async (
  connection: Connection,
  addresses: PublicKey[],
): Promise<ClmmPool[]> => {
  if (addresses.length === 0) return [];
  const accounts = await connection.getMultipleAccountsInfo(addresses);
  return accounts.flatMap((account, index) =>
    account ? [decodePool(addresses[index]!, account.data)] : [],
  );
};

const decodePool = (address: PublicKey, data: Uint8Array): ClmmPool => {
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);

  const decimals0 = data[OFF.decimals0]!;
  const decimals1 = data[OFF.decimals1]!;

  // `sqrt_price_x64` is a *u128*, and for any price above about one its high
  // half is not zero — reading only the low eight bytes silently returns a
  // different number rather than failing. Squared it is the ratio of raw
  // amounts, so the decimal difference completes the conversion.
  const low = view.getBigUint64(OFF.sqrtPriceX64, true);
  const high = view.getBigUint64(OFF.sqrtPriceX64 + 8, true);
  const sqrtPrice = Number((high << 64n) | low) / 2 ** 64;

  return {
    address,
    mint0: key(data, OFF.mint0),
    mint1: key(data, OFF.mint1),
    observation: key(data, OFF.observation),
    decimals0,
    decimals1,
    tickCurrent: view.getInt32(OFF.tickCurrent, true),
    price: sqrtPrice * sqrtPrice * 10 ** (decimals0 - decimals1),
  };
};
