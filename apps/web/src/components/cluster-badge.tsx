import { Badge } from '@/components/ui/badge';
import { CLUSTER, IS_MAINNET } from '@/lib/cluster';

/**
 * Always visible: with several wallets open, which network this is decides
 * whether the numbers on screen are money.
 */
export function ClusterBadge() {
  return (
    <Badge
      variant={IS_MAINNET ? 'default' : 'secondary'}
      className="font-mono text-xs"
      title={IS_MAINNET ? 'Real funds' : 'Test funds only'}
    >
      {CLUSTER}
    </Badge>
  );
}
