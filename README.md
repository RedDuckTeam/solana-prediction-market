<p align="center">
  <a href="https://redduck.io">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset=".github/assets/redduck-logo-dark.svg">
      <img src=".github/assets/redduck-logo.svg" alt="RedDuck" width="240">
    </picture>
  </a>
</p>

<h1 align="center">Solana Prediction Market</h1>

<p align="center">
  Parimutuel prediction markets that settle from on-chain price history.
  No oracle reporter, no privileged resolver.
</p>

---

A market asks whether a token's time-weighted price will be above a strike at a
given instant, and answers it from price data already recorded on chain: Raydium
CLMM observation rings and Pyth TWAP attestations. Settlement is permissionless
and deterministic — anyone can run it, and everyone who runs it gets the same
result.

## Repository layout

| Path | Contents |
|---|---|
| `programs/prediction-market` | Anchor program, 20 instructions |
| `crates/market-math` | Q64.64 fixed-point arithmetic with deterministic flooring |
| `crates/market-vm` | Predicate bytecode: verifier and interpreter |
| `crates/market-feeds` | Raydium CLMM ring and Pyth TWAP readers |
| `crates/market-core` | Payout arithmetic, settlement ramp, schedule |
| `crates/market-wasm` | Verifier compiled to WebAssembly |
| `packages/sdk` | TypeScript client |
| `packages/predicate-wasm` | Compiled verifier used by the web editor |
| `apps/web` | React front end, deployed as a static bundle plus a Cloudflare worker |
| `scripts/devnet` | Devnet bootstrap scripts |
| `tests/` | Integration, attack, compute-budget, and end-to-end suites |

All money arithmetic lives in crates with no Solana dependency. The program
validates accounts and authority, extracts numbers, calls the crates, and writes
results back. The same Rust code that settles on chain is compiled to
WebAssembly and runs in the browser, so the editor and the program cannot
disagree about what a predicate does.

CI runs `cargo fmt --check`, `clippy -D warnings`, `anchor build`, the full Rust
suite, and the TypeScript typecheck on every push
([`ci.yml`](.github/workflows/ci.yml)).

## Protocol

### Lifecycle

A market moves through fixed stages on a timetable derived from governance
parameters copied at creation:

1. **Creation.** Anyone creates a market over 3–8 registered price sources: a
   predicate, a strike, a settlement band, and a settlement instant. The
   predicate is verified before the market is created and its hash is stored in
   the market account. The creator posts a bond covering keeper rewards and pays
   a non-refundable creation fee.
2. **Open.** Betting opens after a creation cooldown. Stakers deposit collateral
   on YES or NO and receive transferable SPL tokens, one per unit staked. Each
   side is capped at a fraction of the thinnest source's attested depth.
3. **Lock.** Betting closes `skew` seconds before the averaging window starts.
   No one can stake while the window that decides the outcome is being measured.
4. **Window.** The TWAP window of `twap_window` seconds runs up to the
   settlement instant. Nothing happens on chain during this stage.
5. **Snapshot.** After the settlement instant, anyone may call `snapshot`. It
   reads every declared source over the window and freezes the readings in a
   `Snapshot` account. The snapshot must land within the grace period;
   otherwise the market voids.
6. **Resolve.** Anyone may call `resolve` at any time after the snapshot. It
   runs the predicate over the frozen readings and splits the pot. The result is
   a pure function of the snapshot: it does not depend on who calls or when.
7. **Claim.** Winners redeem pro rata during the claim window. Afterwards,
   unclaimed dust and uncollected fee shares are swept to the treasury, and
   `close_market` returns the remaining bond and rent to the creator.

If honest settlement is impossible — one side has no stake after lock, a source
cannot be read, the predicate aborts, or the grace period expires without a
snapshot — the market voids and every staker is refunded at par.

### Price sources

Governance registers sources into an on-chain registry behind a timelock. A
source is a specific Raydium CLMM pool or a specific Pyth instrument. Market
creators select from the registry and cannot supply their own; pool creation on
Raydium is permissionless and has no liquidity floor, so unrestricted sources
would let a creator control the median of three with two shallow pools. Mints
and decimals are read from the pool account itself, not taken as arguments.

**Raydium CLMM.** A pool's observation ring holds 100 entries, written at most
once per 15 seconds, on swaps that move the tick. Each observation records the
tick as it stood before the swap, so a price cannot be backdated: it must have
actually held. The reader reconstructs the TWAP from cumulative tick values with
these rules:

- No segment inside the window may exceed `max_segment` seconds, and at least
  `min_observations` observations must fall inside the window. A pool nobody
  traded through is rejected here.
- No extrapolation. Extending the cumulative to the settlement instant with the
  pool's live tick would make the answer depend on when `snapshot` was called.
- A ring that has not wrapped is read from what it holds. A window that reaches
  into the unwritten tail is rejected, never read as zeros.
- Every segment's cumulative must divide its duration exactly. This is a
  property of how Raydium writes and serves as a canary against a layout change.

The ring guarantees `99 × 15 = 1485` seconds of history in the worst case; that
bounds `twap_window + grace`. Segment parameters were calibrated against live
mainnet accounts: the deepest SOL/USDC pool records observations a median of 50
seconds apart with gaps up to 767 seconds.

**Pyth.** A reading is accepted only if the attested TWAP window matches the
market's declared window within `pyth_window_tolerance`, publisher confidence is
within `max_confidence_bps` of the price, and the share of the window in missed
slots is within `max_down_slots_ratio`. Without the window check, a poster could
select a profitable window out of signed history.

### Predicates

A market's resolution rule is a stack program over the snapshot prices — an
instruction set with arithmetic, comparison, boolean, hashing, and aggregation
opcodes. Limits: 2048 bytes of code, stack depth 32, 8 inputs.

- **No control flow.** `SELECT` evaluates both arms and picks one. Instruction
  count equals running time, and both are known at creation.
- **Verified once.** Stack depth, operand validity, and types are proved by
  abstract interpretation in `create_market`. A program that would fail at
  resolution cannot be created. The interpreter re-checks nothing.
- **No allocation, no panics.** Fixed-size structures and `Result` throughout.
- **Aborts void.** A runtime failure such as overflow or division by zero voids
  the market. It is never converted to a boolean outcome.

A predicate cannot call another program. A resolution rule must be a pure
function of the snapshot, and a call into an upgradeable program is not: its
authority could change the answer after bets are placed. Opcode numbering is
permanent wire format, since `spec_hash` commits to exact bytes and markets
outlive releases.

The web editor builds predicates as a typed dataflow graph and checks it with
the same verifier the chain runs, compiled to WebAssembly. It reports
instruction count, code size, and peak stack depth before anything is signed.

### Settlement band

Payout is not a step function of the settled price. Each market declares a band
around its strike (at least `min_ramp_bps` wide); inside the band the pot is
split linearly, outside it the market is binary.

With a step payout, manipulation profit diverges: the gain is the entire losing
pool while the cost of pushing the price across the threshold goes to zero as
the price approaches it. Over a band, gain is linear in the distance pushed
while cost remains convex. The ramp is applied by the program, not by the
predicate — a band implemented in bytecode could not be verified, and a creator
could publish a step function.

### Payouts and fees

The pot is split parimutuel: the winning side shares the losing side's stake pro
rata. The fee (`fee_bps`) is charged on the amount that changes hands, never on
the whole pot, so a winning side always recovers at least its principal and a
side that neither wins nor loses pays nothing. The fee splits 70% to the
treasury, 25% to the market creator, 5% to the snapshot keeper. Every division
floors toward the vault; residue is swept to the treasury after the claim
window. The vault can never owe more than it holds, by construction, and this is
property-tested over the full input space.

Stake totals are counters updated on deposit, deliberately not the outcome
mints' supplies: holders can burn their own SPL tokens, and supply-anchored
payout would overpay whoever claims last.

### Incentives

Every settlement step is permissionless and paid. The creator's bond pays
`keeper_reward` to whoever lands `snapshot` and to whoever lands `resolve`; the
resolver is paid before the predicate runs, so the reward is earned even when
resolution ends in a void. `keeper_reward` is validated at or above the
rent of the `Snapshot` account, which the snapshot keeper fronts and which is
never closed: it is the audit record a settlement is re-derived from.

The bond is never forfeited. On-chain state cannot distinguish an idle keeper
from an unreadable feed after the ring has been overwritten, so forfeiture would
fine creators for feed failures. Unspent bond returns at `close_market`; spam is
priced by the non-refundable creation fee.

Snapshotting has a deadline (the grace period) because Raydium rings overwrite:
settling later with later data would answer a different question and would give
the losing side a free option to wait. Resolution has no deadline because the
snapshot is permanent and the predicate is pure.

### Accounts

All accounts are PDAs. `market_id` is caller-chosen, so addresses are known
before signing.

| Account | Seeds | Written |
|---|---|---|
| `Config` | `["config"]` | by governance, behind a timelock |
| `Collateral` | `["collateral", mint]` | by governance |
| `Feed` | `["feed", source_id]` | by governance, behind a timelock |
| `Market` | `["market", market_id]` | every deposit, then settlement |
| `MarketSpec` | `["spec", market]` | once, at creation |
| `Snapshot` | `["snapshot", market]` | once, at settlement |
| `Vault` | `["vault", market]` | token account owned by the market |
| `YesMint`, `NoMint` | `["yes"\|"no", market]` | mint authority is the market |

`MarketSpec` holds the immutable specification (bytecode, sources, strike) and
is bound to `Market` by `spec_hash`, so resolution cannot be pointed at a
different specification. `Market` is the small, frequently written account.

Markets copy all governance parameters at creation. A parameter change never
alters a market that already holds money.

### Governance

A single authority (intended to be a multisig) registers price sources and
collateral mints and sets parameters. Feed registrations and parameter changes
wait out a timelock, bounded between 1 hour and 30 days. Authority transfer is
two-step (nominate, then accept).

Two levers act immediately, as incident response: disabling a feed and disabling
a collateral. A market over a disabled feed cannot be snapshotted and voids into
refunds when its grace period lapses; a disabled collateral stops further
deposits. Governance can therefore force any unresolved market into a refund.
It cannot pick a winner, move the vault, or profit from a void.

Collateral is restricted to classic SPL mints. A Token-2022 mint with a transfer
fee would make the vault insolvent, since amounts received would be smaller than
amounts booked.

### Compute

Measured on a real binary: deposit ≈ 20k CU, resolve ≈ 24k CU, snapshot ≈ 67k CU
per Raydium feed, since every segment of every ring is validated. Clients must
request a raised limit for `snapshot`: 400k covers three sources, and the
maximum of eight fits inside the 1.4M transaction ceiling. Both figures are
pinned by a test, because a snapshot that stops fitting is a market that voids.

## Trust model

**Governance** decides which sources exist. Admitting a manipulable source is
the most consequential failure available in this system and nothing downstream
compensates for it. All other governance powers are bounded as described above.

**Raydium** owns the layout and semantics of the accounts this program parses,
and is upgradeable behind their multisig. A layout change would produce wrong
prices silently. Mitigations: the divisibility canary on every segment, and the
pause switch, which stops new markets while existing ones settle out.

**Pyth and Wormhole.** A Pyth reading trusts the publisher set and the Wormhole
guardians that carried it to Solana — two independent groups.

### Accepted limitations

- **Manipulating the underlying market.** Nothing prevents capital from moving a
  token's real price near settlement, and no oracle design does. Cost is raised
  by the settlement band, the time-weighted window, per-side stake caps tied to
  attested source depth, and curated sources. None of these eliminates it.
- **Late betting dominates.** In a parimutuel, betting late gives the same odds
  with better information. The displayed stake ratio is not a probability and
  the front end labels it accordingly.
- **Position tokens remain transferable after lock.** Freezing them would
  require Token-2022 transfer hooks and cost wallet compatibility. The window is
  short, and with void conditions fixed by pre-settlement state, post-lock
  speculation reduces to a bet on keeper failure.
- **Mint rent is unrecoverable.** The legacy Token program cannot close a mint;
  each market strands about 0.003 SOL in its two outcome mints.

## Development

Prerequisites: Rust, [Solana CLI](https://solana.com/docs/intro/installation)
3.1+, [Anchor](https://www.anchor-lang.com/) 1.1+, Node 20+, pnpm.

```bash
anchor build      # required first: integration tests load the binary
cargo test        # unit, property, byte-level, in-process integration
pnpm install
pnpm --filter @prediction-market/web test   # editor round-trip tests
pnpm --filter @prediction-market/web dev
```

Additional suites:

```bash
cargo test -p market-feeds --test mainnet -- --nocapture           # recorded mainnet accounts
cargo test -p e2e-tests --test localnet -- --ignored --nocapture   # boots a validator
```

The test layers, and what each one establishes:

| Layer | Establishes |
|---|---|
| Property tests | Payout arithmetic never panics, never overflows, never owes more than the vault holds |
| Differential tests | Tick-to-price conversion matches a 120-digit reference over the supported range |
| Byte-level tests | Parsers read the wire format as the producing program writes it |
| Mainnet fixtures | The Raydium parser reads real accounts; independent pools agree to four decimals |
| Integration | Every lifecycle path, all void branches, both source kinds, every value flow out of the vault |
| Attack suite | Account substitution, double claims, cross-market confusion, cap evasion, authority takeover |
| Compute budget | Settlement cost measured and pinned |
| Editor round-trip | Every template compiles, verifies, and executes through the real interpreter |
| Localnet e2e | The full protocol against a live validator |

Mainnet fixtures are committed rather than fetched: three parameters in an
earlier design were derived analytically, contradicted by live data on first
contact, and could not have been caught by synthetic fixtures written under the
same wrong assumptions.

Native tests do not exercise the BPF stack. A BPF frame is 4 KB against
megabytes on the host, and stack overflows in this codebase were only visible
once the program ran as a real binary. The integration suite loads the compiled
`.so` for this reason.

Conventions:

- Money arithmetic lives in the crates; instruction handlers are glue.
- One file per instruction, one module per account type.
- No `unwrap`, no `panic!`, no silent wrapping; overflow is an error.
- A test's name is a claim its body asserts.

## Deployment

```bash
./scripts/deploy.sh devnet path/to/wallet.json
```

The script builds, syncs the program id into `declare_id!` and `Anchor.toml`,
rebuilds, verifies the wallet balance covers rent, deploys, and refreshes the
IDL the client reads. The wallet argument is mandatory; nothing is inherited
from `solana config`. The script refuses mainnet.

Keep `target/deploy/prediction_market-keypair.json`. It is the program address
and upgrade authority; losing it makes the deployment permanently unupgradeable
and unfreezable.

A fresh deployment is inert until, in order: `initialize_config`,
`register_collateral`, `register_feed`, then one timelock. Program upgrades must
preserve account layouts; there is no migration mechanism.

### RPC endpoint

The web app is a static bundle plus one Cloudflare worker that serves it and
proxies JSON-RPC on `/rpc`, attaching the real endpoint from a secret. The
browser never sees the endpoint, and the worker enforces a method allowlist
(`apps/web/worker/index.ts`) so the proxied endpoint is useless for anything but
this app's reads. Subscriptions pass through unfiltered — that is how a wallet
confirms its transaction landed.

```bash
cp apps/web/.dev.vars.example apps/web/.dev.vars                      # local secret
pnpm --filter @prediction-market/web exec wrangler secret put UPSTREAM_RPC_URL

pnpm --filter @prediction-market/web dev       # site + worker, same runtime as production
pnpm --filter @prediction-market/web preview   # built output through the same worker
pnpm --filter @prediction-market/web deploy    # build, then wrangler
```

To bypass the worker entirely, set `VITE_RPC_URL` to a full endpoint URL at
build time. The value is compiled into the public bundle, so it must be an
endpoint fit to be public. Unset, with no worker, the app falls back to the
cluster's public endpoint.

The devnet scripts run outside the browser and take the endpoint from the
shell:

```bash
export UPSTREAM_RPC_URL=https://devnet.your-provider.com/?api-key=...
pnpm --filter @prediction-market/devnet markets
```

## Devnet deployment

| | |
|---|---|
| Program | `CDJdcKyxiBHDKbyLMTi9988Bw2Vu1i9FnqRcQp7dJreD` |
| Sources | three Raydium CLMM pools, one per fee tier |
| Collateral | wSOL |

Bootstrap, in order:

```bash
pnpm --filter @prediction-market/devnet pools      # issue a token, open three CLMM pools
pnpm --filter @prediction-market/devnet liquidity  # fund them
pnpm --filter @prediction-market/devnet nudge      # trade through them, filling the rings
pnpm --filter @prediction-market/devnet seed       # config, collateral, three feeds
pnpm --filter @prediction-market/devnet markets    # waits out the feed timelock (1 hour)
pnpm --filter @prediction-market/devnet params     # propose, then adopt, demo parameters
```

`seed` refuses a pool traded through fewer than twice, applying the same
coverage rule at registration that settlement will apply later. `markets` is
idempotent and only creates settlement times that are missing.

The pools are real Raydium pools; every price the protocol reads was written by
Raydium as the result of an actual swap. Nothing in this repository writes a
price. What devnet lacks is traders, so the front end offers (on test networks
only) a button that swaps through a market's sources. The swap is real: the
price moves, the fee is paid, the observation is Raydium's to write. A TWAP
needs a reading at each end of its window, so a market whose window passed
without trades needs exactly one swap after the window closes before it can be
snapshotted; the market page shows which sources still lack coverage, computed
by the same reader the chain uses.

`Config` pins the AMM program id at `initialize_config` permanently. Were it
editable, governance could repoint every registered feed at its own program.
The cost is operational: when Raydium retires a devnet deployment, as they have
done before, the only recourse is redeployment.

## Security

Report vulnerabilities through a private security advisory on this repository.
Do not open public issues for anything that could move funds.

## License

MIT. See [LICENSE](LICENSE).
