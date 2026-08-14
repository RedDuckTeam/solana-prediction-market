import {
  bn,
  configPda,
  fetchSpec,
  marketAddresses,
  client,
  settlementAccounts,
  type Market,
} from '@prediction-market/sdk';
import {
  ASSOCIATED_TOKEN_PROGRAM_ID,
  NATIVE_MINT,
  TOKEN_PROGRAM_ID,
  createAssociatedTokenAccountIdempotentInstruction,
  createSyncNativeInstruction,
  getAssociatedTokenAddressSync,
} from '@solana/spl-token';
import { useConnection, useWallet } from '@solana/wallet-adapter-react';
import { ComputeBudgetProgram, PublicKey, SystemProgram, Transaction } from '@solana/web3.js';
import { useCallback, useEffect, useState } from 'react';

import { useAnchorProvider } from '@/components/solana-providers';
import { describeFailure, ExplainedError, type ActionFailure } from '@/lib/errors';

/**
 * Settlement reads several observation rings, far past the 200 000 units a
 * transaction gets by default. Asking for headroom everywhere costs nothing.
 */
const COMPUTE_BUDGET = 400_000;

export interface ActionState {
  pending: boolean;
  error: ActionFailure | null;
  signature: string | null;
}

const idle: ActionState = { pending: false, error: null, signature: null };

export function useMarketActions(marketAddress: PublicKey | null, market: Market | null) {
  const provider = useAnchorProvider();
  const { publicKey } = useWallet();
  const [state, setState] = useState<ActionState>(idle);

  const run = useCallback(
    async (build: () => Promise<Transaction>) => {
      setState({ pending: true, error: null, signature: null });
      try {
        const transaction = await build();
        transaction.instructions.unshift(
          ComputeBudgetProgram.setComputeUnitLimit({ units: COMPUTE_BUDGET }),
        );
        const signature = await provider!.sendAndConfirm(transaction);
        setState({ pending: false, error: null, signature });
        return signature;
      } catch (cause) {
        setState({ pending: false, error: describeFailure(cause), signature: null });
        throw cause;
      }
    },
    [provider],
  );

  const stake = useCallback(
    async (sideIsYes: boolean, amount: bigint) => {
      if (!provider || !publicKey || !market || !marketAddress) {
        throw new ExplainedError('Connect a wallet first.');
      }
      const program = client(provider);
      const addresses = marketAddresses(market.marketId);
      const sideMint = sideIsYes ? market.yesMint : market.noMint;

      const collateralAccount = getAssociatedTokenAddressSync(market.collateralMint, publicKey);
      const outcomeAccount = getAssociatedTokenAddressSync(sideMint, publicKey);

      return run(async () => {
        const transaction = new Transaction();
        // Idempotent: the position account may or may not exist, and finding
        // out costs a round trip that this saves.
        transaction.add(
          createAssociatedTokenAccountIdempotentInstruction(
            publicKey,
            outcomeAccount,
            publicKey,
            sideMint,
            TOKEN_PROGRAM_ID,
            ASSOCIATED_TOKEN_PROGRAM_ID,
          ),
        );
        transaction.add(
          await program.methods['deposit']!(sideIsYes, bn(amount))
            .accounts({
              market: marketAddress,
              collateralMint: market.collateralMint,
              vault: addresses.vault,
              sideMint,
              depositorCollateral: collateralAccount,
              depositorOutcome: outcomeAccount,
              depositor: publicKey,
            })
            .instruction(),
        );
        return transaction;
      });
    },
    [provider, publicKey, market, marketAddress, run],
  );

  const claim = useCallback(
    async (sideIsYes: boolean) => {
      if (!provider || !publicKey || !market || !marketAddress) {
        throw new ExplainedError('Connect a wallet first.');
      }
      const program = client(provider);
      const addresses = marketAddresses(market.marketId);
      const sideMint = sideIsYes ? market.yesMint : market.noMint;

      return run(async () => {
        const transaction = new Transaction();
        const collateralAccount = getAssociatedTokenAddressSync(market.collateralMint, publicKey);
        transaction.add(
          createAssociatedTokenAccountIdempotentInstruction(
            publicKey,
            collateralAccount,
            publicKey,
            market.collateralMint,
            TOKEN_PROGRAM_ID,
            ASSOCIATED_TOKEN_PROGRAM_ID,
          ),
        );
        transaction.add(
          await program.methods['claim']!(sideIsYes)
            .accounts({
              market: marketAddress,
              vault: addresses.vault,
              sideMint,
              holderOutcome: getAssociatedTokenAddressSync(sideMint, publicKey),
              holderCollateral: collateralAccount,
              holder: publicKey,
            })
            .instruction(),
        );
        return transaction;
      });
    },
    [provider, publicKey, market, marketAddress, run],
  );

  const snapshot = useCallback(async () => {
    if (!provider || !publicKey || !market || !marketAddress) {
      throw new ExplainedError('Connect a wallet first.');
    }
    const program = client(provider);
    const addresses = marketAddresses(market.marketId);

    // Inside `run`, so the refusal reaches the panel as a failure state
    // rather than an unhandled rejection: unlike the wallet guards above,
    // this one is reachable from an enabled button.
    return run(async () => {
      const spec = await fetchSpec(provider.connection, marketAddress);
      const { accounts, unposted } = await settlementAccounts(provider.connection, spec);
      if (unposted.length > 0) {
        throw new ExplainedError(
          `${unposted.length} oracle ${unposted.length === 1 ? 'feed has' : 'feeds have'} no price posted for this window yet.`,
          'A Pyth reading has to be posted on chain before it can be frozen. Post the window’s TWAP update, then take the snapshot.',
        );
      }
      return new Transaction().add(
        await program.methods['snapshot']!()
          .accounts({
            config: configPda(),
            market: marketAddress,
            spec: addresses.spec,
            snapshot: addresses.snapshot,
            keeper: publicKey,
          })
          .remainingAccounts(accounts)
          .instruction(),
      );
    });
  }, [provider, publicKey, market, marketAddress, run]);

  const resolve = useCallback(async () => {
    if (!provider || !publicKey || !market || !marketAddress) {
      throw new ExplainedError('Connect a wallet first.');
    }
    const program = client(provider);
    const addresses = marketAddresses(market.marketId);

    return run(async () =>
      new Transaction().add(
        await program.methods['resolve']!()
          .accounts({
            market: marketAddress,
            spec: addresses.spec,
            snapshot: addresses.snapshot,
            resolver: publicKey,
          })
          .instruction(),
      ),
    );
  }, [provider, publicKey, market, marketAddress, run]);

  /**
   * Markets hold SPL tokens and a faucet gives native SOL, so without this the
   * demo is unreachable. `syncNative` is what makes the lamports count.
   */
  const wrapSol = useCallback(
    async (lamports: bigint) => {
      if (!provider || !publicKey) throw new ExplainedError('Connect a wallet first.');
      const account = getAssociatedTokenAddressSync(NATIVE_MINT, publicKey);

      return run(async () =>
        new Transaction()
          .add(
            createAssociatedTokenAccountIdempotentInstruction(
              publicKey,
              account,
              publicKey,
              NATIVE_MINT,
              TOKEN_PROGRAM_ID,
              ASSOCIATED_TOKEN_PROGRAM_ID,
            ),
          )
          .add(
            SystemProgram.transfer({
              fromPubkey: publicKey,
              toPubkey: account,
              lamports: Number(lamports),
            }),
          )
          .add(createSyncNativeInstruction(account, TOKEN_PROGRAM_ID)),
      );
    },
    [provider, publicKey, run],
  );

  const voidMarket = useCallback(async () => {
    if (!provider || !publicKey || !marketAddress) {
      throw new ExplainedError('Connect a wallet first.');
    }
    const program = client(provider);

    return run(async () =>
      new Transaction().add(
        await program.methods['void']!()
          .accounts({
            market: marketAddress,
            caller: publicKey,
          })
          .instruction(),
      ),
    );
  }, [provider, publicKey, marketAddress, run]);

  return {
    ...state,
    stake,
    claim,
    snapshot,
    resolve,
    voidMarket,
    wrapSol,
    connected: Boolean(publicKey),
  };
}

/**
 * `null` while unknown. Refreshed on demand rather than polled: it changes when
 * this app changes it, and a timer would spend requests to learn nothing.
 */
export function useTokenBalance(mint: PublicKey | null): {
  balance: bigint | null;
  refresh: () => void;
} {
  const { connection } = useConnection();
  const { publicKey } = useWallet();
  const [balance, setBalance] = useState<bigint | null>(null);
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    let cancelled = false;
    if (!publicKey || !mint) {
      setBalance(null);
      return;
    }
    (async () => {
      try {
        const address = getAssociatedTokenAddressSync(mint, publicKey);
        const account = await connection.getTokenAccountBalance(address);
        if (!cancelled) setBalance(BigInt(account.value.amount));
      } catch {
        // No account yet is a balance of zero, not a failure worth showing.
        if (!cancelled) setBalance(0n);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [connection, publicKey, mint?.toBase58(), nonce]);

  return { balance, refresh: useCallback(() => setNonce((n) => n + 1), []) };
}
