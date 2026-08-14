import { Component, type ErrorInfo, type ReactNode } from 'react';

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

/**
 * The last line: a rendering crash anywhere below lands here instead of on a
 * blank page. Money is on chain, not in this tab, so the honest remedy is to
 * say so and offer a reload — no state worth preserving lives in the page.
 */
export class ErrorBoundary extends Component<Props, State> {
  override state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  override componentDidCatch(error: Error, info: ErrorInfo) {
    // The console is the right sink: this page has no telemetry by design.
    console.error('render crash:', error, info.componentStack);
  }

  override render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="flex min-h-screen items-center justify-center p-6">
        <div className="w-full max-w-md space-y-3 rounded-lg border p-6 text-center">
          <h1 className="text-lg font-semibold">This page hit a bug and stopped.</h1>
          <p className="text-sm text-muted-foreground">
            Nothing on chain is affected — markets, positions and claims live on Solana, not in
            this tab. Reloading starts the page clean.
          </p>
          <button
            type="button"
            onClick={() => window.location.reload()}
            className="inline-flex h-9 items-center justify-center rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground hover:bg-primary/90"
          >
            Reload
          </button>
          <details className="text-left">
            <summary className="cursor-pointer text-xs text-muted-foreground/70">
              Technical detail
            </summary>
            <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-all text-[11px] text-muted-foreground/80">
              {this.state.error.message}
            </pre>
          </details>
        </div>
      </div>
    );
  }
}
