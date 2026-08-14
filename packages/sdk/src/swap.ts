import {
  ClmmInstrument,
  PoolInfoLayout,
  TickArrayBitmapExtensionLayout,
  TickArrayBitmapUtil,
} from '@raydium-io/raydium-sdk-v2';
import {
  createAssociatedTokenAccountIdempotentInstruction,
  createSyncNativeInstruction,
  getAssociatedTokenAddressSync,
  NATIVE_MINT,
} from '@solana/spl-token';
import {
  PublicKey,
  SystemProgram,
  Transaction,
  type Connection,
  type TransactionInstruction,
} from '@solana/web3.js';
import BN from 'bn.js';

import type { ClmmPool } from './raydium.ts';

/**
 * Swapping through a Raydium pool, using Raydium's own client: the account list
 * is theirs to change, and which tick arrays a swap needs is answered by a
 * bitmap in the pool. Probing for them instead is a guess that fails on chain,
 * after the transaction is paid for.
 *
 * A separate entry point so nothing pulls an exchange client in to read a price.
 */

/** Raydium keeps ticks in arrays covering this many spacings each. */
const TICK_ARRAY_SIZE = 60;

/** Tick arrays to hand a swap. Three covers any of these and still fits a message. */
const ROUTE_LENGTH = 3;

const bitmapExtensionAddress = (programId: PublicKey, pool: PublicKey) =>
  PublicKey.findProgramAddressSync(
    [new TextEncoder().encode('pool_tick_array_bitmap_extension'), pool.toBytes()],
    programId,
  )[0];

/** A decimal amount as base units, without going through a float. */
const baseUnits = (amount: number, decimals: number): bigint => {
  const [whole = '0', fraction = ''] = amount.toFixed(decimals).split('.');
  return BigInt(`${whole}${fraction.padEnd(decimals, '0')}`);
};

/**
 * A pool prices its first mint in its second, so spending the second raises it.
 * Which is first is Raydium's decision, taken by address.
 */
export const mintToSpend = (pool: ClmmPool, priceRising: boolean): PublicKey =>
  priceRising ? pool.mint1 : pool.mint0;

export interface SwapArgs {
  connection: Connection;
  programId: PublicKey;
  pool: PublicKey;
  payer: PublicKey;
  /** The mint being spent; the other side of the pair comes back. */
  inputMint: PublicKey;
  inputAccount: PublicKey;
  outputAccount: PublicKey;
  /** In base units of the input mint. */
  amount: bigint;
}

/**
 * One swap, with the tick arrays it crosses. No minimum out and no price limit:
 * a deliberate nudge, not a trade wanting a good fill. A live network needs both.
 */
export const buildSwap = async ({
  connection,
  programId,
  pool,
  payer,
  inputMint,
  inputAccount,
  outputAccount,
  amount,
}: SwapArgs): Promise<TransactionInstruction> => {
  const extensionAddress = bitmapExtensionAddress(programId, pool);
  const [poolAccount, extensionAccount] = await connection.getMultipleAccountsInfo([
    pool,
    extensionAddress,
  ]);
  if (!poolAccount) throw new Error(`pool ${pool.toBase58()} does not exist`);
  if (!extensionAccount) throw new Error(`pool ${pool.toBase58()} has no bitmap extension`);

  const state = PoolInfoLayout.decode(poolAccount.data);
  const extension = TickArrayBitmapExtensionLayout.decode(extensionAccount.data);
  const zeroForOne = inputMint.equals(state.mintA);
  const stride = state.tickSpacing * TICK_ARRAY_SIZE;

  const tickArrays = TickArrayBitmapUtil.findTickArrayAddress({
    programId,
    poolId: pool,
    tickSpacing: state.tickSpacing,
    poolBitmap: state.tickArrayBitmap,
    tickArrayBitmap: extension,
    findInfo: {
      type: zeroForOne ? 'zeroForOne' : 'oneForZero',
      tickArrayCurrent: Math.floor(state.tickCurrent / stride) * stride,
      count: ROUTE_LENGTH,
    },
  });

  return ClmmInstrument.swapV2Instruction(
    programId,
    payer,
    pool,
    state.configId,
    inputAccount,
    outputAccount,
    zeroForOne ? state.vaultA : state.vaultB,
    zeroForOne ? state.vaultB : state.vaultA,
    zeroForOne ? state.mintA : state.mintB,
    zeroForOne ? state.mintB : state.mintA,
    tickArrays,
    state.observationId,
    new BN(amount.toString()),
    new BN(0),
    new BN(0),
    true,
    extensionAddress,
  );
};

/** Shown to whoever pressed the button, so it says what to do instead. */
export const NOT_ENOUGH_TO_SELL =
  'Moving the price this way means selling the token. Push it the other way first — that swap hands you some.';

export interface NudgeArgs {
  connection: Connection;
  programId: PublicKey;
  pool: ClmmPool;
  payer: PublicKey;
  inputMint: PublicKey;
  /** What to commit, valued in SOL whichever side is being spent. */
  sol: number;
}

/**
 * A whole nudge: wrap, make room for what comes back, swap. Self-contained, so
 * several can be signed together and land in any order. Sized by value, not by
 * count — the same number of token units is worth a fraction of the SOL.
 */
export const buildNudge = async ({
  connection,
  programId,
  pool,
  payer,
  inputMint,
  sol,
}: NudgeArgs): Promise<Transaction> => {
  const spendingSol = inputMint.equals(NATIVE_MINT);
  const outputMint = inputMint.equals(pool.mint0) ? pool.mint1 : pool.mint0;
  const decimals = inputMint.equals(pool.mint0) ? pool.decimals0 : pool.decimals1;
  const inputAccount = getAssociatedTokenAddressSync(inputMint, payer);
  const outputAccount = getAssociatedTokenAddressSync(outputMint, payer);

  const tokensPerSol = pool.mint0.equals(NATIVE_MINT) ? pool.price : 1 / pool.price;
  const amount = baseUnits(spendingSol ? sol : sol * tokensPerSol, decimals);

  // Only one direction can be taken with SOL alone: moving a price the other way
  // means selling the thing being priced.
  if (!spendingSol) {
    const held = await connection
      .getTokenAccountBalance(inputAccount)
      .then((balance) => BigInt(balance.value.amount))
      .catch(() => 0n);
    if (held < amount) {
      // Named so the front end shows it verbatim instead of dissecting it:
      // the sentence above is the explanation.
      const refusal = new Error(NOT_ENOUGH_TO_SELL);
      refusal.name = 'ExplainedError';
      throw refusal;
    }
  }

  const transaction = new Transaction();
  // A pool holds tokens and native SOL is not one, so lamports are wrapped here
  // rather than beforehand, which keeps the nudge to a single signature.
  if (spendingSol) {
    transaction.add(
      createAssociatedTokenAccountIdempotentInstruction(payer, inputAccount, payer, NATIVE_MINT),
      SystemProgram.transfer({
        fromPubkey: payer,
        toPubkey: inputAccount,
        lamports: Number(amount),
      }),
      createSyncNativeInstruction(inputAccount),
    );
  }
  transaction.add(
    createAssociatedTokenAccountIdempotentInstruction(payer, outputAccount, payer, outputMint),
    await buildSwap({
      connection,
      programId,
      pool: pool.address,
      payer,
      inputMint,
      inputAccount,
      outputAccount,
      amount,
    }),
  );
  return transaction;
};
