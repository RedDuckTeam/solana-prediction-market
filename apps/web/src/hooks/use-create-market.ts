import {
  bn,
  configPda,
  collateralPda,
  fetchConfig,
  fromDecimalString,
  marketAddresses,
  client,
} from '@prediction-market/sdk';
import { TOKEN_PROGRAM_ID } from '@solana/spl-token';
import { Buffer } from 'buffer';
import { useWallet } from '@solana/wallet-adapter-react';
import { ComputeBudgetProgram, PublicKey, SystemProgram, Transaction } from '@solana/web3.js';
import { useCallback, useState } from 'react';

import { useAnchorProvider } from '@/components/solana-providers';
import { describeFailure, ExplainedError, type ActionFailure } from '@/lib/errors';

export interface SourceChoice {
  feed: PublicKey;
  /**
   * Read the pair the other way round, which lets one pool serve as either leg
   * of a composed price: TKN/SOL times SOL/USDC prices a token in dollars.
   */
  invert: boolean;
}

export interface NewMarket {
  feeds: SourceChoice[];
  collateralMint: PublicKey;
  /** As the user typed it. Parsed exactly, never through a float. */
  strike: string;
  rampBps: number;
  settleAt: number;
  /** Compiled by the graph editor and already verified against the chain's rules. */
  bytecode: Uint8Array;
  rulesUri: string;
}

/**
 * A market's address is derived from an identifier the caller picks, so it is
 * known before the transaction is signed. Random, because two people composing
 * the same question at the same second should not collide.
 */
const newMarketId = (): Uint8Array => crypto.getRandomValues(new Uint8Array(32));

export function useCreateMarket() {
  const provider = useAnchorProvider();
  const { publicKey } = useWallet();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<ActionFailure | null>(null);

  const create = useCallback(
    async (draft: NewMarket): Promise<PublicKey> => {
      if (!provider || !publicKey) throw new ExplainedError('Connect a wallet first.');
      setPending(true);
      setError(null);
      try {
        const program = client(provider);
        const config = await fetchConfig(provider.connection);
        const id = newMarketId();
        const addresses = marketAddresses(id);

        const instruction = await program.methods['createMarket']!({
          marketId: [...id],
          settleAt: bn(draft.settleAt),
          strike: bn(fromDecimalString(draft.strike)),
          rampBps: draft.rampBps,
          feeds: draft.feeds,
          // Anchor's borsh coder writes `bytes` with `Buffer.copy`, which a
          // plain Uint8Array does not have.
          bytecode: Buffer.from(draft.bytecode),
          rulesUri: draft.rulesUri,
        })
          .accounts({
            config: configPda(),
            collateral: collateralPda(draft.collateralMint),
            mint: draft.collateralMint,
            collateralMint: draft.collateralMint,
            market: addresses.market,
            spec: addresses.spec,
            vault: addresses.vault,
            yesMint: addresses.yesMint,
            noMint: addresses.noMint,
            creator: publicKey,
            treasury: config.treasury,
            tokenProgram: TOKEN_PROGRAM_ID,
            systemProgram: SystemProgram.programId,
          })
          // One `Feed` per declared input, in the same order the spec lists them.
          .remainingAccounts(
            draft.feeds.map(({ feed }) => ({
              pubkey: feed,
              isSigner: false,
              isWritable: false,
            })),
          )
          .instruction();

        const transaction = new Transaction()
          .add(ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }))
          .add(instruction);

        await provider.sendAndConfirm(transaction);
        return addresses.market;
      } catch (cause) {
        setError(describeFailure(cause));
        throw cause;
      } finally {
        setPending(false);
      }
    },
    [provider, publicKey],
  );

  return { create, pending, error, connected: Boolean(publicKey) };
}
