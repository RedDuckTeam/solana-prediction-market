/**
 * Brings a fresh devnet deployment to the point where markets can be created.
 *
 *   pnpm --filter @prediction-market/devnet seed
 *
 * Idempotent: every step checks whether it has already happened, so a partial
 * run can simply be repeated. Feeds only become usable one timelock later, so
 * markets are a separate step.
 */
import { BN } from '@anchor-lang/core';
import { configPda, feedPda } from '@prediction-market/sdk';
import { PublicKey, SystemProgram } from '@solana/web3.js';

import { RAYDIUM_CLMM, WSOL, client, connect, demoParams, log, readPools } from './shared.ts';

/** The real Pyth receiver. Registered nowhere, but `Config` names it. */
const PYTH_RECEIVER = new PublicKey('rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ');

/** The floor a deployment may configure, so the demo waits the shortest legal time. */
const TIMELOCK = 3_600;

/** 0.05 SOL. Small enough to bet with a faucet, large enough to be visible. */
const MIN_STAKE = 50_000_000;

/**
 * What the demo attests each pool is worth. A real deployment measures this and
 * stands behind it — it sets the per-side cap.
 */
const DEPTH_QUOTE = 400_000_000_000;



/** Raydium's `ObservationState`: a 100-entry ring of 44-byte observations. */
const RING = { newestIndex: 17, observations: 51, entry: 44, slots: 100 } as const;

/**
 * A window the ring can already answer, for the registration probe, which has
 * to satisfy the coverage rule a settlement will: a reading at each end. That
 * takes two swaps, so a pool traded through once cannot be registered.
 */
const ringWindow = (data: Buffer): { from: number; to: number; written: number } => {
  const newest = data.readUInt16LE(RING.newestIndex);
  const stamp = (slot: number) => data.readUInt32LE(RING.observations + slot * RING.entry);
  const wrapped = stamp((newest + 1) % RING.slots) !== 0;
  return {
    from: stamp(wrapped ? (newest + 1) % RING.slots : 0),
    to: stamp(newest),
    written: wrapped ? RING.slots : newest + 1,
  };
};

const main = async () => {
  const { connection, keypair, provider } = connect();
  const program = client(provider);
  const { pools } = readPools();

  log(`authority ${keypair.publicKey.toBase58()}`);
  log(`reading prices from ${RAYDIUM_CLMM.toBase58()}\n`);

  const exists = async (key: PublicKey) => (await connection.getAccountInfo(key)) !== null;

  // -- Config ---------------------------------------------------------
  if (await exists(configPda())) {
    log('config: already initialised');
  } else {
    await program.methods['initializeConfig']!({
      treasury: keypair.publicKey,
      raydiumClmmProgram: RAYDIUM_CLMM,
      pythReceiverProgram: PYTH_RECEIVER,
      timelock: TIMELOCK,
      params: demoParams(),
    })
      .accounts({
        config: configPda(),
        payer: keypair.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .rpc();
    log('config: initialised');
  }

  // -- Collateral -----------------------------------------------------
  const collateral = PublicKey.findProgramAddressSync(
    [Buffer.from('collateral'), WSOL.toBuffer()],
    program.programId,
  )[0];
  if (await exists(collateral)) {
    log('collateral: wSOL already registered');
  } else {
    await program.methods['registerCollateral']!(new BN(MIN_STAKE))
      .accounts({
        config: configPda(),
        authority: keypair.publicKey,
        mint: WSOL,
        collateral,
        payer: keypair.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .rpc();
    log('collateral: wSOL registered');
  }

  // -- Feeds ----------------------------------------------------------
  //
  // Registration reads the ring for real, so a pool that cannot answer today is
  // refused today. The probe asks for a window the ring already covers.
  for (const record of pools) {
    const ring = new PublicKey(record.observation);
    const feed = feedPda(ring);
    if (await exists(feed)) {
      log(`feed ${record.fee}: already registered`);
      continue;
    }

    const account = await connection.getAccountInfo(ring);
    if (!account) throw new Error(`${record.observation} does not exist`);
    const window = ringWindow(account.data);
    if (window.written < 2) {
      log(
        `feed ${record.fee}: only ${window.written} observation — swap through it once more`,
      );
      continue;
    }

    // The pair's mints and decimals are read on chain out of the pool account
    // itself, so registration cannot be handed a transposed pair.
    const label = Buffer.alloc(32);
    label.write(`TKN/SOL ${record.fee}`);

    await program.methods['registerFeed']!({
      depthQuote: new BN(DEPTH_QUOTE),
      label: [...label],
      probeFrom: new BN(window.from),
      probeTo: new BN(window.to),
      // The probe only has to prove the ring is readable, so it is asked under
      // the loosest bound the protocol permits rather than the market's own.
      probeMaxSegment: Math.max(1, window.to - window.from),
      probeMinObservations: 0,
    })
      .accounts({
        config: configPda(),
        authority: keypair.publicKey,
        pool: new PublicKey(record.pool),
        observationState: ring,
        feed,
        payer: keypair.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .rpc();
    log(`feed ${record.fee}: registered from a ${window.to - window.from}s probe`);
  }

  const effective = new Date((Math.floor(Date.now() / 1000) + TIMELOCK) * 1000);
  log(`\nSeeded. Feeds become usable at ${effective.toISOString()}.`);
};

main().catch((error: unknown) => {
  console.error(error);
  process.exit(1);
});
