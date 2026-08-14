import { fetchCollateral, type CollateralRecord } from '@prediction-market/sdk';
import { useConnection } from '@solana/wallet-adapter-react';
import type { PublicKey } from '@solana/web3.js';
import { useEffect, useState } from 'react';

/**
 * Chiefly the smallest stake a mint accepts. Without it the form submits stakes
 * the program refuses, and the refusal reads as though nothing was typed.
 */
export function useCollateral(mint: PublicKey | null): CollateralRecord | null {
  const { connection } = useConnection();
  const [record, setRecord] = useState<CollateralRecord | null>(null);

  useEffect(() => {
    if (!mint) return;
    let cancelled = false;
    fetchCollateral(connection, mint)
      .then((found) => {
        if (!cancelled) setRecord(found);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [connection, mint?.toBase58()]);

  return record;
}
