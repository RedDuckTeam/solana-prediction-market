import { Link } from 'react-router';

import { Button } from '@/components/ui/button';

export function NotFoundRoute() {
  return (
    <div className="space-y-4 py-16 text-center">
      <h1 className="text-2xl font-semibold tracking-tight">Nothing here</h1>
      <p className="text-sm text-muted-foreground">
        That address does not correspond to a market.
      </p>
      <Button asChild variant="secondary">
        <Link to="/">Back to markets</Link>
      </Button>
    </div>
  );
}
