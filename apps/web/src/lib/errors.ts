import { errorByCode } from '@prediction-market/sdk';

/**
 * One place that turns whatever a failed action threw — an Anchor error, an
 * RPC refusal, a wallet dismissal, a dead network — into something a person
 * can act on. Everything user-facing goes through here: a raw
 * `cause.message` shown in a panel is a bug, not a shortcut.
 */
export interface ActionFailure {
  /** One plain sentence: what did not happen. */
  title: string;
  /** What to do about it, when there is something to do. */
  hint?: string;
  /** The raw message and salient log lines, for a bug report. Collapsed by default. */
  detail?: string;
  /** True when trying the same action again can genuinely succeed. */
  retryable: boolean;
  /** The wallet user said no. Not an error — render it quietly. */
  declined: boolean;
}

const failure = (
  title: string,
  options: Partial<Omit<ActionFailure, 'title'>> = {},
): ActionFailure => ({
  title,
  retryable: false,
  declined: false,
  ...options,
});

/**
 * An error whose message was written for the person, not the machine. Thrown
 * by this app's own guards ("connect a wallet first") and shown verbatim.
 * Matched by name rather than `instanceof`, so code that must not import this
 * module — the SDK's swap helper, say — can still mark a message as final.
 */
export class ExplainedError extends Error {
  constructor(message: string, public readonly hint?: string) {
    super(message);
    this.name = 'ExplainedError';
  }
}

/**
 * What each program error means *to the person who just clicked*, for every
 * error a user can reach from this interface. The IDL's message is the
 * fallback; an entry here exists when the protocol's reason needs translating
 * into what to do next.
 */
const PROGRAM_ERROR_HELP: Record<string, { title: string; hint?: string }> = {
  Paused: {
    title: 'The protocol is paused for new markets.',
    hint: 'Existing markets settle normally. Try again once governance unpauses.',
  },
  DepositsClosed: {
    title: 'Betting on this market has closed.',
    hint: 'Betting always ends before the measured window opens, so nobody bets while watching the prices that settle the market.',
  },
  CapExceeded: {
    title: 'This bet would push its side past the market’s cap.',
    hint: 'The cap bounds how much one side can win to what the price sources can honestly bear. Stake less, or take the other side.',
  },
  BelowMinimumStake: {
    title: 'The stake is below this market’s minimum.',
    hint: 'The minimum keeps the rent on the token accounts a sane fraction of the bet.',
  },
  ZeroAmount: { title: 'The amount is zero.' },
  OutsideSnapshotWindow: {
    title: 'Prices can only be frozen between settlement and the grace deadline.',
    hint: 'Too early: wait for the settlement time. Too late: the market can only be voided and refunded now.',
  },
  WrongState: {
    title: 'The market is not in a state where this action applies.',
    hint: 'The page may be showing a stale phase — it refreshes within a few seconds.',
  },
  VoidConditionNotMet: {
    title: 'This market cannot be voided yet.',
    hint: 'Voiding opens only when a side is empty past the deadline, or the grace period passes with no snapshot.',
  },
  FeedUnreadable: {
    title: 'A price source cannot be read for this window.',
    hint: 'A Raydium pool needs a trade at or after the settlement instant before its average exists. Push the price once, then try again.',
  },
  FeedNotActive: {
    title: 'A price source is disabled or still inside its timelock.',
  },
  NothingToClaim: { title: 'There is nothing to claim here.' },
  ClaimWindowClosed: {
    title: 'The claim window for this market has closed.',
    hint: 'Unclaimed funds have been swept to the treasury.',
  },
  ClaimWindowOpen: {
    title: 'Claiming is still open, so dust cannot be swept yet.',
  },
  RampTooNarrow: {
    title: 'The settlement band is narrower than governance allows.',
    hint: 'A hair-thin band is a step function again, which makes nudging the price across the strike worth the whole pot.',
  },
  SettlementTimeInvalid: {
    title: 'The timetable does not work.',
    hint: 'Settlement must be far enough out that betting opens and closes before the averaging window starts.',
  },
  PredicateInvalid: {
    title: 'The chain refused the expression.',
    hint: 'This should have been caught by the editor — please report it.',
  },
  CollateralNotRegistered: {
    title: 'That collateral is not approved for new markets.',
  },
  FeedAccountsMismatch: {
    title: 'The price-source accounts do not match what the market declares.',
    hint: 'The page may be out of date — reload and try again.',
  },
  DuplicateFeed: { title: 'The same price source is listed twice.' },
  NotAuthorized: { title: 'This wallet is not allowed to do that.' },
  ParameterOutOfRange: { title: 'A parameter is outside its permitted range.' },
  Overflow: { title: 'The amounts do not fit — this transaction cannot be made.' },
  PredicateAborted: {
    title: 'The predicate aborted, so the market voids and refunds.',
  },
  VaultNotEmpty: {
    title: 'The vault still holds funds, so the market cannot be closed yet.',
  },
  SpecHashMismatch: {
    title: 'The specification does not match the market.',
    hint: 'This should be impossible from this interface — please report it.',
  },
};

const programFailure = (name: string, fallbackMessage: string, detail?: string): ActionFailure => {
  const help = PROGRAM_ERROR_HELP[name];
  if (help) return failure(help.title, { hint: help.hint, detail });
  return failure(fallbackMessage || `The program refused: ${name}.`, { detail });
};

/** The named Anchor error inside `cause`, however the client surfaced it. */
const anchorError = (cause: unknown): { name: string; message: string } | null => {
  // The Anchor client throws a structured `AnchorError`.
  const shaped = cause as {
    error?: { errorCode?: { code?: string }; errorMessage?: string };
  };
  if (shaped?.error?.errorCode?.code) {
    return {
      name: shaped.error.errorCode.code,
      message: shaped.error.errorMessage ?? '',
    };
  }
  return null;
};

/** Every log line, from whichever field this error kept them in. */
const logsOf = (cause: unknown): string[] => {
  const shaped = cause as { logs?: unknown; transactionLogs?: unknown };
  for (const candidate of [shaped?.logs, shaped?.transactionLogs]) {
    if (Array.isArray(candidate) && candidate.every((line) => typeof line === 'string')) {
      return candidate as string[];
    }
  }
  return [];
};

const messageOf = (cause: unknown): string => {
  if (cause instanceof Error) return cause.message;
  if (typeof cause === 'string') return cause;
  try {
    // `stringify` returns undefined for symbols and functions, not a string.
    return JSON.stringify(cause) ?? String(cause);
  } catch {
    return String(cause);
  }
};

/** A compact technical trail: the message, then the logs that say anything. */
const detailOf = (message: string, logs: string[]): string => {
  const salient = logs.filter(
    (line) => /error|failed|panicked|insufficient/i.test(line) || line.includes('Error'),
  );
  return [message, ...salient].filter(Boolean).join('\n');
};

/** Pulls an Anchor error name or a custom error code out of raw logs. */
const errorFromLogs = (logs: string[], detail: string): ActionFailure | null => {
  for (const line of logs) {
    // Anchor's own runtime log: names the error outright.
    const named = /Error Code: (\w+)\. Error Number: \d+\. Error Message: ([^.]*(?:\.[^E]*)?)/.exec(
      line,
    );
    if (named?.[1]) return programFailure(named[1], named[2]?.trim() ?? '', detail);
  }
  for (const line of logs) {
    const custom = /custom program error: (0x[0-9a-fA-F]+)/.exec(line);
    if (custom?.[1]) {
      const code = Number.parseInt(custom[1], 16);
      const known = errorByCode(code);
      if (known) return programFailure(known.name, known.message, detail);
      // Not one of ours: most commonly the Token program refusing a transfer.
      if (code === 1 && logs.some((entry) => entry.includes('Tokenkeg'))) {
        return failure('Not enough tokens for this transaction.', {
          hint: 'The balance is smaller than the amount being moved.',
          detail,
        });
      }
      return failure(`A program this transaction calls refused it (code ${code}).`, { detail });
    }
  }
  return null;
};

/** Classifies everything that is not a program saying no. */
const environmentFailure = (message: string, detail: string): ActionFailure | null => {
  if (/user rejected|rejected the request|declined|denied|cancell/i.test(message)) {
    return failure('The wallet request was dismissed.', { declined: true });
  }
  if (/insufficient lamports|debit an account but found no record|insufficient funds for rent/i.test(message)) {
    return failure('Not enough SOL to pay for this transaction.', {
      hint: 'Fees and account rent come out of the wallet’s SOL balance. Top it up and try again.',
      detail,
    });
  }
  if (/block height exceeded|blockhash not found|has expired/i.test(message)) {
    return failure('The transaction expired before it landed.', {
      hint: 'Nothing was spent. This happens when the network is ahead of the page — just try again.',
      retryable: true,
      detail,
    });
  }
  if (/429|too many requests|rate.?limit/i.test(message)) {
    return failure('The RPC endpoint is rate-limiting this page.', {
      hint: 'Wait a few seconds and try again, or point the app at your own endpoint.',
      retryable: true,
      detail,
    });
  }
  if (/method not allowed through this endpoint/i.test(message)) {
    return failure('This deployment’s RPC proxy does not allow that call.', {
      hint: 'The worker forwards only the methods the app itself makes. If this appears in normal use, the allowlist is missing one — please report it.',
      detail,
    });
  }
  if (/failed to fetch|networkerror|fetch failed|load failed|timed? ?out|connection refused/i.test(message)) {
    return failure('The network request did not go through.', {
      hint: 'Check the connection — nothing was signed or spent.',
      retryable: true,
      detail,
    });
  }
  if (/exceeded CUs|computational budget exceeded/i.test(message)) {
    return failure('The transaction ran out of compute budget.', {
      hint: 'This is a bug in how the request was built — please report it.',
      detail,
    });
  }
  if (/simulation failed/i.test(message)) {
    // A simulation refusal with no recognisable program error inside it.
    return failure('The transaction would fail, so it was not sent.', { detail });
  }
  return null;
};

/**
 * The one entry point. Total: every input produces something renderable, and
 * the unrecognised case keeps its evidence in `detail` instead of losing it.
 */
export const describeFailure = (cause: unknown): ActionFailure => {
  if (cause instanceof Error && cause.name === 'ExplainedError') {
    return failure(cause.message, {
      hint: (cause as ExplainedError).hint,
    });
  }

  const message = messageOf(cause);
  const logs = logsOf(cause);
  const detail = detailOf(message, logs);

  // Order matters: a wallet dismissal or a dead network can wrap fragments of
  // program-log text, but a *named* program error is always authoritative.
  const named = anchorError(cause);
  if (named) return programFailure(named.name, named.message, detail);

  const fromLogs = errorFromLogs(logs, detail);
  if (fromLogs) return fromLogs;

  const environmental = environmentFailure(message, detail);
  if (environmental) return environmental;

  // Also try the message itself for log-shaped fragments: some wallets throw
  // a string with the whole simulation result glued in.
  const fromMessage = errorFromLogs([message], detail);
  if (fromMessage) return fromMessage;

  return failure('Something went wrong with this action.', {
    hint: 'Nothing was spent unless the wallet showed a confirmation. The technical detail below says what the software saw.',
    detail,
    retryable: true,
  });
};
