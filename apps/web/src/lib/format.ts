/**
 * Times are stored and settled in UTC, and shown in the reader's zone with the
 * offset spelled out. A market that settles "at 15:00" has cost people money
 * before, in every venue that left the zone implicit.
 */
export const formatInstant = (unixSeconds: number | bigint): string => {
  const date = new Date(Number(unixSeconds) * 1000);
  const local = date.toLocaleString(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  });
  const offsetMinutes = -date.getTimezoneOffset();
  const sign = offsetMinutes >= 0 ? '+' : '-';
  const hours = Math.floor(Math.abs(offsetMinutes) / 60);
  const minutes = Math.abs(offsetMinutes) % 60;
  const offset = `UTC${sign}${hours}${minutes ? `:${String(minutes).padStart(2, '0')}` : ''}`;
  return `${local} (${offset})`;
};

export const formatUtc = (unixSeconds: number | bigint): string =>
  `${new Date(Number(unixSeconds) * 1000).toISOString().slice(0, 16).replace('T', ' ')} UTC`;

export const formatCountdown = (secondsAway: number): string => {
  if (secondsAway <= 0) return 'now';
  const days = Math.floor(secondsAway / 86_400);
  const hours = Math.floor((secondsAway % 86_400) / 3_600);
  const minutes = Math.floor((secondsAway % 3_600) / 60);
  if (days) return `${days}d ${hours}h`;
  if (hours) return `${hours}h ${minutes}m`;
  if (minutes) return `${minutes}m`;
  return `${secondsAway}s`;
};

/** A base-unit amount as a decimal, given the mint's decimals. */
export const formatAmount = (raw: bigint, decimals: number, places = 2): string => {
  const scale = 10n ** BigInt(decimals);
  const whole = raw / scale;
  const fraction = ((raw % scale) * 10n ** BigInt(places)) / scale;
  return `${whole}.${fraction.toString().padStart(places, '0')}`;
};

export const parseAmount = (input: string, decimals: number): bigint => {
  const [whole = '0', fraction = ''] = input.trim().split('.');
  const padded = fraction.padEnd(decimals, '0').slice(0, decimals);
  return BigInt(whole || '0') * 10n ** BigInt(decimals) + BigInt(padded || '0');
};

/** A Q64.64 share of the pot, as a percentage. */
export const formatShare = (raw: bigint): string => {
  const tenths = Number((raw * 1000n) >> 64n);
  return `${(tenths / 10).toFixed(1)}%`;
};

export const shortAddress = (address: string): string =>
  `${address.slice(0, 4)}…${address.slice(-4)}`;
