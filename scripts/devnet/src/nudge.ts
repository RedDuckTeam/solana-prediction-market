/**
 * Moves a pool's price by swapping through it.
 *
 *   pnpm --filter @prediction-market/devnet nudge [buy|sell] [amount in SOL]
 *
 * The same transaction the front end's button sends. One swap is one
 * observation, which is why reviving a window takes two presses: an average
 * needs readings at two times, and a transaction lands at one.
 */
import { readClmmPool } from '@prediction-market/sdk';
import { buildNudge, mintToSpend } from '@prediction-market/sdk/swap';
import { PublicKey } from '@solana/web3.js';

import { RAYDIUM_CLMM, WSOL, connect, log, readPools } from './shared.ts';

const main = async () => {
  const buying = (process.argv[2] ?? 'buy') === 'buy';
  const sol = Number(process.argv[3] ?? 0.05);
  const { connection, keypair, provider } = connect();

  for (const record of readPools().pools) {
    const address = new PublicKey(record.pool);
    const pool = await readClmmPool(connection, address);

    // Raydium orders the pair by address, so the token's price is the pool's
    // own only when SOL is the second mint. Buying the token raises it.
    const inverted = pool.mint0.equals(WSOL);
    const quote = (price: number) => (inverted ? 1 / price : price).toFixed(6);

    const transaction = await buildNudge({
      connection,
      programId: RAYDIUM_CLMM,
      pool,
      payer: keypair.publicKey,
      inputMint: mintToSpend(pool, buying !== inverted),
      sol,
    });

    const signature = await provider.sendAndConfirm(transaction);
    const after = await readClmmPool(connection, address);
    log(
      `${record.fee.padEnd(6)} ${quote(pool.price)} -> ${quote(after.price)} SOL` +
        `  (tick ${pool.tickCurrent} -> ${after.tickCurrent})  ${signature.slice(0, 12)}…`,
    );
  }
};

main().catch((error: unknown) => {
  console.error(error);
  process.exit(1);
});
