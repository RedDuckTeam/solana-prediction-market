import { AnchorProvider } from '@anchor-lang/core';
import { ConnectionProvider, WalletProvider, useAnchorWallet, useConnection } from '@solana/wallet-adapter-react';
import { WalletModalProvider } from '@solana/wallet-adapter-react-ui';
import { useMemo, type ReactNode } from 'react';

import { ENDPOINT, WS_ENDPOINT } from '@/lib/cluster';

import '@solana/wallet-adapter-react-ui/styles.css';

/**
 * No wallet list: every current wallet announces itself through the Wallet
 * Standard, and hard-coding adapters only excludes the ones nobody thought of.
 */
export function SolanaProviders({ children }: { children: ReactNode }) {
  return (
    <ConnectionProvider endpoint={ENDPOINT} config={{ commitment: 'confirmed', wsEndpoint: WS_ENDPOINT }}>
      <WalletProvider wallets={[]} autoConnect>
        <WalletModalProvider>{children}</WalletModalProvider>
      </WalletProvider>
    </ConnectionProvider>
  );
}

/** For signing, or `null` with no wallet. Reading needs neither. */
export function useAnchorProvider(): AnchorProvider | null {
  const { connection } = useConnection();
  const wallet = useAnchorWallet();
  return useMemo(
    () =>
      wallet
        ? new AnchorProvider(connection, wallet, { commitment: 'confirmed' })
        : null,
    [connection, wallet],
  );
}
