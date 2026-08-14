/**
 * Creates the markets a visitor finds waiting.
 *
 *   pnpm --filter @prediction-market/devnet markets
 *
 * Run once the registered feeds are past their timelock; it refuses early
 * rather than failing halfway. Safe to run again -- it only creates the
 * settlement times that are missing, so it can be used to top the demo up.
 */
import { BN } from '@anchor-lang/core';
import {
  configPda,
  fetchConfig,
  fetchFeeds,
  fetchMarkets,
  marketAddresses,
  fromDecimalString,
  medianOfSources,
  readClmmPool,
} from '@prediction-market/sdk';
import { PublicKey, SystemProgram, type Connection } from '@solana/web3.js';
import { TOKEN_PROGRAM_ID } from '@solana/spl-token';
import { createHash } from 'node:crypto';

import { WSOL, client, connect, log, readPools } from './shared.ts';

/**
 * Staggered across two orders of magnitude: the short ones settle inside a
 * sitting, the long ones keep something open to bet on the next day. The floor
 * is the schedule itself: the cooldown plus the window plus the skew, twelve
 * minutes on this timetable.
 */
const HORIZONS_MINUTES = [15, 30, 60, 3 * 60, 12 * 60, 48 * 60];

/**
 * What the token costs in SOL. Raydium's ordering puts SOL first here, so the
 * pool quotes the pair the other way and the markets read it inverted.
 */
const tokenPrice = async (connection: Connection, pool: string): Promise<number> =>
  1 / (await readClmmPool(connection, new PublicKey(pool))).price;

/** Half-width of the settlement band, in basis points of the strike. */
const RAMP_BPS = 50;

const RULES_URI = 'https://github.com/RedDuckTeam/solana-prediction-market/blob/main/README.md';

/** Deterministic from the settlement instant, so re-running collides instead of
 * quietly creating a duplicate. */
const marketId = (settleAt: number): Uint8Array =>
  Uint8Array.from(createHash('sha256').update(`demo-market:${settleAt}`).digest());

const main = async () => {
  const { connection, keypair, provider } = connect();
  const program = client(provider);
  const now = Math.floor(Date.now() / 1000);

  // Feeds are matched to the pools this deployment actually created, by
  // address. A registry keeps every feed ever registered, including ones whose
  // pool has been replaced, and a market built on those would ask a visitor to
  // trade a pool they hold no tokens for.
  const pools = readPools().pools;
  const registered = new Map(
    (await fetchFeeds(connection)).map((feed) => [feed.pool.toBase58(), feed]),
  );
  const chosen = pools.map(({ pool, fee }) => {
    const feed = registered.get(pool);
    if (!feed) throw new Error(`pool ${pool} (${fee}) has no registered feed`);
    return feed;
  });

  const early = chosen.filter((feed) => !feed.enabled || Number(feed.effectiveAt) > now);
  if (early.length > 0) {
    const wait = Math.max(...early.map((feed) => Number(feed.effectiveAt) - now));
    log(`${early.length} of ${chosen.length} feeds not effective; ${Math.ceil(wait / 60)}m to go`);
    process.exit(1);
  }
  log(`using ${chosen.map((feed) => feed.label).join(', ')}`);

  // The strike is set against what the pools actually say right now, read the
  // same way the protocol will read it at settlement.
  const observed = await tokenPrice(connection, pools[0]!.pool);
  log(`token price ${observed.toFixed(6)} SOL`);

  const existing = new Set(
    (await fetchMarkets(connection)).map(({ account }) => Number(account.settleAt)),
  );

  const config = await fetchConfig(connection);

  for (const [index, minutes] of HORIZONS_MINUTES.entries()) {
    const settleAt = now + minutes * 60;
    if (existing.has(settleAt)) {
      log(`+${minutes}m: already exists`);
      continue;
    }

    // Strikes straddle the current price, so the demo shows a market that is
    // clearly going one way and one that is genuinely close. The further out a
    // market settles the wider it is set, since a price has longer to move.
    const offset = [0, 0.005, -0.005, 0.02, -0.05, 0.1][index] ?? 0;
    const strike = observed * (1 + offset);

    const id = marketId(settleAt);
    const addresses = marketAddresses(id);
    const collateral = PublicKey.findProgramAddressSync(
      [Buffer.from('collateral'), WSOL.toBuffer()],
      program.programId,
    )[0];

    await program.methods['createMarket']!({
      marketId: [...id],
      settleAt: new BN(settleAt),
      strike: new BN(fromDecimalString(strike.toFixed(9)).toString()),
      rampBps: RAMP_BPS,
      feeds: chosen.map((feed) => ({ feed: feed.address, invert: true })),
      // Anchor's borsh coder writes `bytes` with `Buffer.copy`.
      bytecode: Buffer.from(medianOfSources(chosen.length)),
      rulesUri: RULES_URI,
    })
      .accounts({
        config: configPda(),
        collateral,
        mint: WSOL,
        collateralMint: WSOL,
        market: addresses.market,
        spec: addresses.spec,
        vault: addresses.vault,
        yesMint: addresses.yesMint,
        noMint: addresses.noMint,
        creator: keypair.publicKey,
        treasury: config.treasury,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .remainingAccounts(
        chosen.map((feed) => ({ pubkey: feed.address, isSigner: false, isWritable: false })),
      )
      .rpc();

    log(
      `+${minutes}m: above ${strike.toFixed(6)} SOL at ` +
        `${new Date(settleAt * 1000).toISOString()} -> ${addresses.market.toBase58()}`,
    );
  }

  log('');
  log('Markets created. Move the pools with `nudge`, or the button on the market page.');
};

main().catch((error: unknown) => {
  console.error(error);
  process.exit(1);
});
