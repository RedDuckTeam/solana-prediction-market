import type { Market } from '@prediction-market/sdk';
import { NATIVE_MINT } from '@solana/spl-token';
import type { PublicKey } from '@solana/web3.js';
import { useState } from 'react';

import { Button } from '@/components/ui/button';
import { ErrorNotice } from '@/components/error-notice';
import { Input } from '@/components/ui/input';
import { InfoHint } from '@/components/ui/info-hint';
import { Label } from '@/components/ui/label';
import { useMarketActions, useTokenBalance } from '@/hooks/use-actions';
import { useCollateral } from '@/hooks/use-collateral';
import { useMintDecimals } from '@/hooks/use-mint';
import { formatAmount, parseAmount } from '@/lib/format';
import { cn } from '@/lib/utils';

/** One SOL, which is far more than any demo bet needs and saves a second trip. */
const WRAP_LAMPORTS = 1_000_000_000n;

/**
 * The betting form alone. When betting is not open the `LifecyclePanel`
 * shows the market's current stage instead of mounting this at all.
 */
export function StakePanel({
  address,
  market,
  onDone,
}: {
  address: PublicKey;
  market: Market;
  onDone: () => void;
}) {
  const [side, setSide] = useState<'yes' | 'no'>('yes');
  const [amount, setAmount] = useState('');
  const actions = useMarketActions(address, market);
  const { balance, refresh } = useTokenBalance(market.collateralMint);
  const decimals = useMintDecimals(market.collateralMint);
  const collateral = useCollateral(market.collateralMint);

  // Until the mint's scale is known there is no safe reading of what was typed.
  const parsed = amount && decimals !== null ? parseAmount(amount, decimals) : 0n;
  const staked = side === 'yes' ? market.stakedYes : market.stakedNo;
  const roomLeft = market.capPerSide > staked ? market.capPerSide - staked : 0n;
  const overCap = parsed > roomLeft;
  const overBalance = balance !== null && parsed > balance;
  const belowMin = collateral !== null && parsed > 0n && parsed < collateral.minStake;
  const needsWrapping =
    balance === 0n && market.collateralMint.equals(NATIVE_MINT) && actions.connected;

  const submit = async () => {
    try {
      await actions.stake(side === 'yes', parsed);
    } catch {
      // Shown through `actions.error`; swallowing here keeps the amount in
      // the field so a failed bet can be corrected instead of retyped.
      return;
    }
    setAmount('');
    refresh();
    onDone();
  };

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-2">
        {(['yes', 'no'] as const).map((option) => (
          <button
            key={option}
            type="button"
            onClick={() => setSide(option)}
            className={cn(
              'rounded-md border px-4 py-3 text-sm font-medium capitalize transition-colors',
              side === option
                ? option === 'yes'
                  ? 'border-yes bg-yes/10 text-yes'
                  : 'border-no bg-no/10 text-no'
                : 'text-muted-foreground hover:border-foreground/20',
            )}
          >
            {option}
          </button>
        ))}
      </div>

      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <Label htmlFor="amount">Amount</Label>
          {balance !== null && (
            <button
              type="button"
              className="text-xs text-muted-foreground hover:text-foreground"
              onClick={() => decimals !== null && setAmount(formatAmount(balance, decimals, 6))}
            >
              balance {decimals === null ? '—' : formatAmount(balance, decimals)}
            </button>
          )}
        </div>
        <Input
          id="amount"
          inputMode="decimal"
          placeholder="0.00"
          value={amount}
          onChange={(event) => setAmount(event.target.value.replace(/[^\d.]/g, ''))}
        />
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
          {collateral !== null && decimals !== null && (
            <>min {formatAmount(collateral.minStake, decimals, 3)} ·{' '}</>
          )}
          room on this side {decimals === null ? '—' : formatAmount(roomLeft, decimals)}
          <InfoHint>
            Each side is capped separately, against how expensive the thinnest
            price source is to move — so winning by moving the price costs more
            than it pays.
          </InfoHint>
        </div>
      </div>

      {needsWrapping && (
        <div className="rounded-md border border-dashed p-3 text-xs text-muted-foreground">
          <p>Faucet SOL is native; this market takes wrapped SOL.</p>
          <Button
            variant="secondary"
            size="sm"
            className="mt-2 w-full"
            disabled={!actions.connected || actions.pending}
            onClick={() =>
              void actions.wrapSol(WRAP_LAMPORTS).then(refresh).catch(() => {})
            }
          >
            Wrap 1 SOL
          </Button>
        </div>
      )}

      {belowMin && decimals !== null && (
        <p className="text-sm text-destructive">
          Below the smallest stake this market takes ({formatAmount(collateral!.minStake, decimals, 3)}).
        </p>
      )}
      {overCap && (
        <p className="text-sm text-destructive">More than this side can still take.</p>
      )}
      {overBalance && <p className="text-sm text-destructive">More than you hold.</p>}
      <ErrorNotice failure={actions.error} />

      <Button
        className="w-full"
        disabled={
          !actions.connected || actions.pending || parsed <= 0n || belowMin || overCap || overBalance
        }
        onClick={() => void submit()}
      >
        {!actions.connected
          ? 'Connect a wallet'
          : actions.pending
            ? 'Confirming…'
            : `Stake on ${side}`}
      </Button>

    </div>
  );
}
