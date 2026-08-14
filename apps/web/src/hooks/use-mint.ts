import { getMint } from '@solana/spl-token';
import { useConnection } from '@solana/wallet-adapter-react';
import type { PublicKey } from '@solana/web3.js';
import { useEffect, useState } from 'react';

/**
 * Read, never assumed: wSOL carries nine decimals and USDC six, so a constant
 * would misprice one by a thousand. `null` while unknown, and callers must
 * render that as loading rather than defaulting.
 */
export function useMintDecimals(mint: PublicKey | null): number | null {
  const { connection } = useConnection();
  const [decimals, setDecimals] = useState<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    if (!mint) {
      setDecimals(null);
      return;
    }
    getMint(connection, mint)
      .then((info) => !cancelled && setDecimals(info.decimals))
      .catch(() => !cancelled && setDecimals(null));
    return () => {
      cancelled = true;
    };
  }, [connection, mint?.toBase58()]);

  return decimals;
}
