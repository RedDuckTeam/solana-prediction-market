import { WalletMultiButton } from '@solana/wallet-adapter-react-ui';
import { NavLink } from 'react-router';

import { ClusterBadge } from '@/components/cluster-badge';
import { cn } from '@/lib/utils';

const link = ({ isActive }: { isActive: boolean }) =>
  cn('hover:text-foreground', isActive ? 'text-foreground' : 'text-muted-foreground');

export function Header() {
  return (
    <header className="border-b">
      <div className="mx-auto flex w-full max-w-5xl items-center gap-4 px-4 py-3">
        <NavLink to="/" className="font-semibold tracking-tight">
          Prediction Market
        </NavLink>
        <ClusterBadge />
        <nav className="ml-2 flex items-center gap-4 text-sm">
          <NavLink to="/" className={link} end>
            Markets
          </NavLink>
          <NavLink to="/create" className={link}>
            Create
          </NavLink>
        </nav>
        <div className="ml-auto">
          <WalletMultiButton />
        </div>
      </div>
    </header>
  );
}
