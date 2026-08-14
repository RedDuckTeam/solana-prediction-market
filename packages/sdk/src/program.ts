import { AnchorProvider, Program, type Idl } from '@anchor-lang/core';
import { PublicKey, type Connection } from '@solana/web3.js';

// The attribute is required by Node's ESM loader and ignored by bundlers.
import idl from './idl/prediction_market.json' with { type: 'json' };

export const IDL = idl as Idl;

/** The deployed program. Taken from the IDL so the two can never disagree. */
export const PROGRAM_ID = new PublicKey((idl as { address: string }).address);

/** A client bound to a wallet, for anything that sends a transaction. */
export const client = (provider: AnchorProvider): Program =>
  new Program(IDL, provider);

/** A read-only client. Markets, prices and outcomes are public; reading them
 * should not require connecting a wallet first. */
export const readOnlyClient = (connection: Connection): Program =>
  new Program(IDL, { connection } as AnchorProvider);

/**
 * Fewest price sources a market may declare. Read from the IDL — the program
 * enforces it, so a copy typed here would drift the first time it changed.
 */
export const MIN_MARKET_FEEDS: number = Number(
  (idl as { constants?: Array<{ name: string; value: string }> }).constants?.find(
    (entry) => entry.name === 'MIN_MARKET_FEEDS',
  )?.value ?? 3,
);

/** One program error, as the IDL declares it. */
export interface ProgramError {
  code: number;
  name: string;
  message: string;
}

const PROGRAM_ERRORS: ReadonlyMap<number, ProgramError> = new Map(
  ((idl as { errors?: Array<{ code: number; name: string; msg?: string }> }).errors ?? []).map(
    (entry) => [entry.code, { code: entry.code, name: entry.name, message: entry.msg ?? '' }],
  ),
);

/**
 * The program error behind a numeric code, if it is one of ours. Anchor
 * surfaces failed instructions as `custom program error: 0x…`, and this is how
 * a client turns that number back into a name it can explain.
 */
export const errorByCode = (code: number): ProgramError | undefined => PROGRAM_ERRORS.get(code);
