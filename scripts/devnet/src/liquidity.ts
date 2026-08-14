/**
 * Funds the devnet pools so a swap can move their price.
 *
 *   pnpm --filter @prediction-market/devnet liquidity
 *
 * A concentrated-liquidity pool holds nothing until somebody opens a position.
 * The description is assembled from chain state rather than fetched: the SDK's
 * lookups expect a pool listed in Raydium's API, which a new devnet pool is not.
 */
// The SDK is CommonJS, so only its default export carries every named symbol.
import raydiumSdk from '@raydium-io/raydium-sdk-v2';

const { Raydium, TxVersion } = raydiumSdk;
import BN from 'bn.js';
import Decimal from 'decimal.js';
import { WSOL, connect, log, readPools, type PoolRecord } from './shared.ts';

/**
 * Close in on purpose: Raydium only creates the tick arrays holding a position's
 * boundaries, so a wide range leaves the arrays a swap must name far from the
 * price. Concentrating near it is what keeps a swap routable.
 */
const RANGE_FACTOR = 1.5;

/** Wrapped SOL committed to each pool, in whole SOL. */
const SOL_PER_POOL = 1;

const main = async () => {
  const { connection, keypair } = connect();
  const { pools } = readPools();

  const raydium = await Raydium.load({
    connection,
    owner: keypair,
    cluster: 'devnet',
    disableLoadToken: true,
  });

  for (const record of pools) {
    const keys = (await raydium.clmm.getClmmPoolKeys(record.pool)) as never as {
      programId: string;
      id: string;
      mintA: { address: string; programId: string; decimals: number };
      mintB: { address: string; programId: string; decimals: number };
      config: Record<string, unknown>;
    };
    const state = (await raydium.clmm.getRpcClmmPoolInfo({
      poolId: record.pool,
    })) as never as { currentPrice: number; tickSpacing: number };

    const poolInfo = {
      ...keys,
      type: 'Concentrated',
      price: state.currentPrice,
      config: { ...keys.config, tickSpacing: state.tickSpacing },
    } as never as Parameters<typeof raydium.clmm.openPositionFromBase>[0]['poolInfo'];

    const solIsMintA = keys.mintA.address === WSOL.toBase58();
    const price = new Decimal(state.currentPrice);
    log(`\n${record.fee} pool at ${price.toFixed(4)}`);

    // A tick denotes the price 1.0001^tick of one *raw* unit in the other, so
    // the decimal difference is part of the conversion. Position bounds must
    // land on a multiple of the pool's tick spacing.
    const decimalShift = 10 ** (keys.mintA.decimals - keys.mintB.decimals);
    const edge = (target: Decimal) => {
      const exact = Math.log(target.toNumber() / decimalShift) / Math.log(1.0001);
      return Math.floor(exact / state.tickSpacing) * state.tickSpacing;
    };
    const lower = edge(price.div(RANGE_FACTOR));
    const upper = edge(price.mul(RANGE_FACTOR));

    const { execute } = await raydium.clmm.openPositionFromBase({
      poolInfo,
      poolKeys: keys as never,
      tickLower: Math.min(lower, upper),
      tickUpper: Math.max(lower, upper),
      // Raydium orders the pair by address, so which side is wrapped SOL is
      // its decision, not ours. The base is whichever side that turned out to
      // be, since that is the side we are committing a known amount of.
      base: solIsMintA ? 'MintA' : 'MintB',
      ownerInfo: { useSOLBalance: true },
      baseAmount: new BN(SOL_PER_POOL).mul(
        new BN(10).pow(new BN(solIsMintA ? keys.mintA.decimals : keys.mintB.decimals)),
      ),
      // Generous but finite: an unbounded ceiling overflows the SDK's own
      // arithmetic before it ever reaches the chain.
      otherAmountMax: new BN(10_000_000).mul(new BN(10).pow(new BN(9))),
      txVersion: TxVersion.V0,
      computeBudgetConfig: { units: 600_000, microLamports: 100_000 },
    });

    const { txId } = await execute({ sendAndConfirm: true });
    log(`  ticks ${Math.min(lower, upper)} … ${Math.max(lower, upper)}`);
    log(`  tx    ${txId}`);
  }

  log('\nPools are funded. A swap will now move the price and write an observation.');
};

main().catch((error: unknown) => {
  console.error(error);
  process.exit(1);
});
