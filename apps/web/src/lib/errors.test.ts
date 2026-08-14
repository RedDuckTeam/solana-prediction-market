import { errorByCode } from '@prediction-market/sdk';
import { describe, expect, it } from 'vitest';

import { describeFailure, ExplainedError } from '@/lib/errors';

/**
 * The failure taxonomy is the interface a user meets when anything goes
 * wrong, so it is held to like one: every recognised shape must come out
 * actionable, and the unrecognised case must keep its evidence.
 */
describe('program errors', () => {
  it('maps a structured Anchor error to its explanation', () => {
    const failure = describeFailure({
      error: {
        errorCode: { code: 'DepositsClosed', number: 6013 },
        errorMessage: 'Market is not accepting deposits right now',
      },
      logs: [],
    });
    expect(failure.title).toMatch(/betting.*closed/i);
    expect(failure.hint).toBeTruthy();
    expect(failure.declined).toBe(false);
  });

  it('recovers a named error out of raw transaction logs', () => {
    const failure = describeFailure({
      message: 'Simulation failed.',
      logs: [
        'Program 2k1raPELJJQkwfZxMFNN8ywFNCMCnNJJvoej2Gy2EhuT invoke [1]',
        'Program log: AnchorError occurred. Error Code: CapExceeded. Error Number: 6014. Error Message: Deposit would push this side past its cap.',
      ],
    });
    expect(failure.title).toMatch(/cap/i);
    expect(failure.hint).toMatch(/other side|less/i);
  });

  it('turns a custom error code back into one of ours through the IDL', () => {
    // Whichever error the IDL numbers 6000 — the test survives renumbering.
    const known = errorByCode(6000)!;
    const failure = describeFailure({
      message: 'failed to send transaction',
      logs: ['Program failed: custom program error: 0x1770'],
    });
    expect(known).toBeTruthy();
    // Mapped through the same table, so at minimum it is not the generic fallback.
    expect(failure.title).not.toMatch(/something went wrong/i);
  });

  it('reads a Token program refusal as a balance problem', () => {
    const failure = describeFailure({
      message: 'Transaction simulation failed',
      logs: [
        'Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [2]',
        'Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA failed: custom program error: 0x1',
      ],
    });
    expect(failure.title).toMatch(/not enough tokens/i);
  });
});

describe('the environment failing', () => {
  it('treats a wallet dismissal as a decision, not an error', () => {
    const failure = describeFailure(new Error('User rejected the request.'));
    expect(failure.declined).toBe(true);
  });

  it('says what an expired blockhash means and that retrying works', () => {
    const failure = describeFailure(
      new Error('Signature verification failed: block height exceeded.'),
    );
    expect(failure.retryable).toBe(true);
    expect(failure.title).toMatch(/expired/i);
  });

  it('points a fee failure at the SOL balance', () => {
    const failure = describeFailure(
      new Error('Transaction results in an account with insufficient lamports for rent'),
    );
    expect(failure.title).toMatch(/not enough sol/i);
  });

  it('marks a dead network as retryable and harmless', () => {
    const failure = describeFailure(new TypeError('Failed to fetch'));
    expect(failure.retryable).toBe(true);
    expect(failure.hint).toMatch(/nothing was signed/i);
  });

  it('names the worker allowlist when the proxy refuses a method', () => {
    const failure = describeFailure(
      new Error('method not allowed through this endpoint: getClusterNodes'),
    );
    expect(failure.title).toMatch(/proxy/i);
  });
});

describe('the app speaking for itself', () => {
  it('shows an ExplainedError verbatim, hint included', () => {
    const failure = describeFailure(
      new ExplainedError('Connect a wallet first.', 'The button asks for a signature.'),
    );
    expect(failure.title).toBe('Connect a wallet first.');
    expect(failure.hint).toBe('The button asks for a signature.');
  });

  it('honours the marker by name, so the SDK can set it without importing us', () => {
    const refusal = new Error('Push the price the other way first.');
    refusal.name = 'ExplainedError';
    expect(describeFailure(refusal).title).toBe('Push the price the other way first.');
  });
});

describe('the unrecognised case', () => {
  it('never loses the evidence', () => {
    const failure = describeFailure(new Error('some novel catastrophe'));
    expect(failure.detail).toContain('some novel catastrophe');
    expect(failure.title).toBeTruthy();
  });

  it('is total over non-Error garbage', () => {
    for (const garbage of [undefined, null, 42, { a: 1 }, 'plain string', Symbol('x')]) {
      const failure = describeFailure(garbage);
      expect(failure.title.length).toBeGreaterThan(0);
    }
  });
});
