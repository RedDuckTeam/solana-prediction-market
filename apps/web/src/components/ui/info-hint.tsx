import { Info } from 'lucide-react';
import type { ReactNode } from 'react';

/**
 * A detail worth knowing, behind a tap.
 *
 * The interface keeps one short line visible per control and moves the
 * reasoning here, so reading it is a choice rather than a toll. The prose it
 * holds is the same prose that used to sit on the page — nothing honest was
 * cut, only demoted.
 */
export function InfoHint({ children }: { children: ReactNode }) {
  return (
    <details className="relative inline-block align-middle leading-none">
      <summary
        aria-label="Explain"
        className="inline-flex cursor-pointer list-none items-center text-muted-foreground/60 transition-colors hover:text-foreground [&::-webkit-details-marker]:hidden"
      >
        <Info className="size-3.5" />
      </summary>
      <div className="absolute left-0 top-5 z-20 w-72 max-w-[80vw] rounded-md border bg-popover p-3 text-xs font-normal normal-case leading-relaxed text-muted-foreground shadow-md">
        {children}
      </div>
    </details>
  );
}
