const STEPS = [
  {
    title: 'One question, one number',
    body: 'Will the price clear the strike at the stated moment — measured, never judged.',
  },
  {
    title: 'Two sides, one pot',
    body: 'Stake on yes or no; the pot is divided when the market settles.',
  },
  {
    title: 'Betting closes early',
    body: 'Before the measured window opens, so nobody bets on prices they can watch.',
  },
  {
    title: 'Nobody decides',
    body: 'The median of several on-chain sources settles it. No reporter, no appeal.',
  },
];

/**
 * The explanation a first-time visitor needs, folded so a returning one never
 * scrolls past it twice. Shown without a wallet, because asking someone to
 * connect before telling them what the thing does is backwards.
 */
export function HowItWorks() {
  return (
    <details className="rounded-xl border bg-card text-card-foreground">
      <summary className="cursor-pointer list-none p-4 text-sm font-medium [&::-webkit-details-marker]:hidden">
        How this works
      </summary>
      <div className="grid gap-4 p-4 pt-0 sm:grid-cols-2">
        {STEPS.map((step, index) => (
          <div key={step.title} className="space-y-1">
            <p className="text-sm font-medium">
              <span className="mr-2 text-muted-foreground">{index + 1}</span>
              {step.title}
            </p>
            <p className="text-sm text-muted-foreground">{step.body}</p>
          </div>
        ))}
      </div>
    </details>
  );
}
