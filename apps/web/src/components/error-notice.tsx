import type { ActionFailure } from '@/lib/errors';

/**
 * The one way a failed action is shown. Every panel renders failures through
 * this, so a dismissal, a program refusal and a dead network all read the same
 * way everywhere: what did not happen, what to do about it, and the technical
 * trail folded away for a bug report.
 */
export function ErrorNotice({ failure }: { failure: ActionFailure | null }) {
  if (!failure) return null;

  // Saying no in the wallet is a decision, not a malfunction.
  if (failure.declined) {
    return <p className="text-xs text-muted-foreground">{failure.title}</p>;
  }

  return (
    <div
      role="alert"
      className="space-y-1 rounded-md border border-destructive/30 bg-destructive/5 p-3"
    >
      <p className="text-sm font-medium text-destructive">{failure.title}</p>
      {failure.hint && <p className="text-xs text-muted-foreground">{failure.hint}</p>}
      {failure.retryable && (
        <p className="text-xs text-muted-foreground">Trying again can work — nothing is stuck.</p>
      )}
      {failure.detail && (
        <details className="pt-1">
          <summary className="cursor-pointer text-xs text-muted-foreground/70">
            Technical detail
          </summary>
          <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-all text-[11px] leading-snug text-muted-foreground/80">
            {failure.detail}
          </pre>
        </details>
      )}
    </div>
  );
}
