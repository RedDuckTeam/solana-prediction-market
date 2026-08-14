import { BN } from '@anchor-lang/core';

/**
 * Q64.64 fixed point, matching `crates/market-math`. Converts at the edges only:
 * a rounding difference between client and chain is a difference about money.
 */

export const FRACTIONAL_BITS = 64n;
export const ONE = 1n << FRACTIONAL_BITS;

/** Rounds toward negative infinity, as the on-chain arithmetic does. */
const floorDiv = (a: bigint, b: bigint): bigint => {
  const quotient = a / b;
  return a % b !== 0n && a < 0n !== b < 0n ? quotient - 1n : quotient;
};

/** Exactly, never via `parseFloat`: a strike settles markets and a double would shift it. */
export const fromDecimalString = (input: string): bigint => {
  const trimmed = input.trim();
  if (!/^-?\d+(\.\d+)?$/.test(trimmed)) {
    throw new Error(`not a decimal number: ${input}`);
  }
  const negative = trimmed.startsWith('-');
  const [whole, fraction = ''] = trimmed.replace('-', '').split('.');
  const scale = 10n ** BigInt(fraction.length);
  const scaled = (BigInt(whole ?? '0') * scale + BigInt(fraction || '0')) * ONE;
  const magnitude = scaled / scale;
  return negative ? -magnitude : magnitude;
};

/** For display only. Never feed the result back into anything on chain. */
export const toDisplayString = (raw: bigint, decimals = 4): string => {
  const negative = raw < 0n;
  const magnitude = negative ? -raw : raw;
  const whole = magnitude >> FRACTIONAL_BITS;
  const scale = 10n ** BigInt(decimals);
  const fraction = ((magnitude & (ONE - 1n)) * scale) >> FRACTIONAL_BITS;
  const padded = fraction.toString().padStart(decimals, '0');
  return `${negative ? '-' : ''}${whole}${decimals > 0 ? `.${padded}` : ''}`;
};

/**
 * At a fixed count of significant digits, since prices differ by orders of
 * magnitude with the quote direction: two places read 0.0109 as `0.01`.
 */
export const toSignificantString = (raw: bigint, significant = 5): string => {
  const magnitude = raw < 0n ? -raw : raw;
  const value = Number(magnitude) / 2 ** 64;
  const exponent = value > 0 ? Math.floor(Math.log10(value)) : 0;
  const places = Math.min(9, Math.max(2, significant - 1 - exponent));
  return toDisplayString(raw, places);
};

export { floorDiv };

/**
 * For any field wider than 32 bits. Anchor encodes those with `toArrayLike`, a
 * `BN` method, so a plain `bigint` type-checks and then throws at assembly.
 */
export const bn = (value: bigint | number): BN => new BN(value.toString());
