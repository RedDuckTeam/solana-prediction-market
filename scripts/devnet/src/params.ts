/**
 * Moves a live deployment's governance parameters to the demo defaults.
 *
 *   pnpm --filter @prediction-market/devnet params
 *
 * Two-phase by construction, because the protocol is: the first run proposes
 * and reports when the timelock expires; a run after that adopts. Idempotent
 * like the rest — a proposal identical to what is already pending or already
 * adopted is reported, not re-sent.
 */
import { configPda, fetchConfig } from '@prediction-market/sdk';

import { client, connect, demoParams, log } from './shared.ts';

const main = async () => {
  const { connection, keypair, provider } = connect();
  const program = client(provider);
  const now = Math.floor(Date.now() / 1000);

  const config = await fetchConfig(connection);
  const wanted = demoParams();

  const matchesWanted = (params: Awaited<ReturnType<typeof fetchConfig>>['params']) =>
    params.feeBps === wanted.feeBps &&
    params.keeperReward === BigInt(wanted.keeperReward.toString()) &&
    params.creationFee === BigInt(wanted.creationFee.toString()) &&
    params.twapWindow === wanted.twapWindow &&
    params.grace === wanted.grace &&
    params.claimWindow === wanted.claimWindow;

  if (matchesWanted(config.params)) {
    log('params: already the demo defaults, nothing to do');
    return;
  }

  if (config.pending) {
    if (now >= config.pending.effectiveAt) {
      await program.methods['adoptParams']!()
        .accounts({ config: configPda(), authority: keypair.publicKey })
        .rpc();
      log('params: pending change adopted');
    } else {
      const minutes = Math.ceil((config.pending.effectiveAt - now) / 60);
      log(`params: a change is pending; adoptable in ${minutes}m — run this again then`);
    }
    return;
  }

  await program.methods['proposeParams']!(wanted)
    .accounts({ config: configPda(), authority: keypair.publicKey })
    .rpc();
  const minutes = Math.ceil(config.timelock / 60);
  log(`params: proposed; run this again in ${minutes}m to adopt`);
};

main().catch((error: unknown) => {
  console.error(error);
  process.exit(1);
});
