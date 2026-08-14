/**
 * Creates the Raydium pools a devnet deployment reads prices from.
 *
 *   pnpm --filter @prediction-market/devnet pools
 *
 * Run once. One concentrated-liquidity pool per fee tier — the shape a real pair
 * has on mainnet, so a market's median is a median of separate venues. These are
 * real pools: every price the protocol reads is written by Raydium itself.
 */
import { Raydium, DEVNET_PROGRAM_ID, TxVersion } from '@raydium-io/raydium-sdk-v2';
import { createMint, mintTo, getOrCreateAssociatedTokenAccount } from '@solana/spl-token';
import { PublicKey } from '@solana/web3.js';
import BN from 'bn.js';
import Decimal from 'decimal.js';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';

import { WSOL, connect, log, poolsFile } from './shared.ts';

/** The three fee tiers Raydium runs on devnet, by tick spacing. */
const FEE_TIERS = [
  { config: 'CD4aJtX11cqTCAc83nxSPkkh5JW2yjD6uwHeovjqQ1qu', spacing: 60, label: '0.25%' },
  { config: 'FZdkW5jiYsjTnCVqFqPrxrQisQkCYrohd7ArZhoKnM8q', spacing: 10, label: '0.05%' },
  { config: 'F8aaMZVpXaQHk3Qo9BPDhsa7RgpfrfiRsk8L3iXnq3AT', spacing: 1, label: '0.01%' },
];

const TOKEN_DECIMALS = 9;

/**
 * The quote side is wSOL rather than a mint of our own: a visitor arrives
 * holding nothing else, and a pool they cannot trade is one they cannot revive.
 */
const OPENING_PRICE = 0.01;

/** Supply minted to ourselves, to be split across the pools as liquidity. */
const TOKEN_SUPPLY = 100_000_000;

/** The SDK returns addresses as strings in some versions and keys in others. */
const asAddress = (value: unknown): string =>
  typeof value === 'string' ? value : (value as PublicKey).toBase58();

const main = async () => {
  const { connection, keypair } = connect();

  if (existsSync(poolsFile)) {
    const done = JSON.parse(readFileSync(poolsFile, 'utf8')) as { pools: unknown[] };
    log(`${poolsFile} already lists ${done.pools.length} pools. Delete it to start over.`);
    return;
  }

  log('issuing the token…');
  const token = await createMint(connection, keypair, keypair.publicKey, null, TOKEN_DECIMALS);
  const quote = WSOL;
  log(`  token ${token.toBase58()} (${TOKEN_DECIMALS} decimals)`);
  log(`  quote ${quote.toBase58()} (wrapped SOL)`);

  const account = await getOrCreateAssociatedTokenAccount(
    connection,
    keypair,
    token,
    keypair.publicKey,
  );
  await mintTo(
    connection,
    keypair,
    token,
    account.address,
    keypair,
    BigInt(TOKEN_SUPPLY) * 10n ** BigInt(TOKEN_DECIMALS),
  );
  log('  supply minted');

  const raydium = await Raydium.load({
    connection,
    owner: keypair,
    cluster: 'devnet',
    disableLoadToken: true,
  });

  const mintA = { address: token.toBase58(), programId: '', decimals: TOKEN_DECIMALS };
  const mintB = { address: quote.toBase58(), programId: '', decimals: 9 };
  const { TOKEN_PROGRAM_ID } = await import('@solana/spl-token');
  mintA.programId = TOKEN_PROGRAM_ID.toBase58();
  mintB.programId = TOKEN_PROGRAM_ID.toBase58();

  const created: Array<{ pool: string; observation: string; fee: string; spacing: number }> = [];

  for (const tier of FEE_TIERS) {
    log(`\nopening the ${tier.label} pool…`);
    const { execute, extInfo } = await raydium.clmm.createPool({
      programId: DEVNET_PROGRAM_ID.CLMM_PROGRAM_ID,
      mint1: mintA as never,
      mint2: mintB as never,
      ammConfig: {
        id: new PublicKey(tier.config),
        index: 0,
        protocolFeeRate: 0,
        tradeFeeRate: 0,
        tickSpacing: tier.spacing,
        fundFeeRate: 0,
        description: '',
      } as never,
      initialPrice: new Decimal(OPENING_PRICE),
      txVersion: TxVersion.V0,
    });
    const { txId } = await execute({ sendAndConfirm: true });
    const poolId = asAddress(extInfo.address.id);
    const observation = asAddress(extInfo.address.observationId);
    log(`  pool ${poolId}`);
    log(`  ring ${observation}`);
    log(`  tx   ${txId}`);

    created.push({
      pool: poolId,
      observation,
      fee: tier.label,
      spacing: tier.spacing,
    });
  }

  writeFileSync(
    poolsFile,
    `${JSON.stringify({ token: token.toBase58(), quote: quote.toBase58(), pools: created }, null, 2)}\n`,
  );
  log(`\nwritten to ${poolsFile}`);
  log('next: add liquidity, then register the rings as feeds');
  void BN;
};

main().catch((error: unknown) => {
  console.error(error);
  process.exit(1);
});
