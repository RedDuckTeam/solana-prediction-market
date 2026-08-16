#!/usr/bin/env bash
#
# Deploys the program to a cluster and points the client at it.
#
#   ./scripts/deploy.sh devnet [path/to/wallet.json]
#
# The program's address is its keypair, `target/deploy/prediction_market-keypair.json`. It
# is generated on first build and never committed: whoever holds it holds upgrade
# authority. Keep it, or the deployment can neither be upgraded nor frozen.
#
set -euo pipefail

CLUSTER="${1:-devnet}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

case "$CLUSTER" in
  localnet|devnet|mainnet-beta) ;;
  *) echo "Unknown cluster: $CLUSTER" >&2; exit 1 ;;
esac

if [ "$CLUSTER" = "mainnet-beta" ]; then
  cat >&2 <<'WARNING'
Refusing to deploy to mainnet from a script.

This program has not been audited and has never held money. If you have decided
to deploy it anyway, do so deliberately, by hand, with an upgrade authority you
control and a plan for what happens when something is wrong.
WARNING
  exit 1
fi

# Never inherited from `solana config`: that points wherever it was last left,
# and a deploy paid for by the wrong key is not recoverable.
WALLET="${2:-${DEPLOYER_KEYPAIR:-}}"
if [ -z "$WALLET" ]; then
  echo "No wallet given. Pass one as the second argument or set DEPLOYER_KEYPAIR." >&2
  exit 1
fi
WALLET="${WALLET/#\~/$HOME}"
[ -f "$WALLET" ] || { echo "Wallet not found: $WALLET" >&2; exit 1; }

# The same variable the rest of the repository uses. A program upload is
# thousands of requests, and a public endpoint refuses partway through.
case "$CLUSTER" in
  localnet) RPC="${UPSTREAM_RPC_URL:-http://127.0.0.1:8899}" ;;
  devnet)   RPC="${UPSTREAM_RPC_URL:-https://api.devnet.solana.com}" ;;
esac

PAYER="$(solana address -k "$WALLET")"
echo "==> Paying from $PAYER on $CLUSTER"

echo "==> Building"
anchor build

echo "==> Syncing the program id into declare_id! and Anchor.toml"
anchor keys sync
# The id changes on a first build, so the binary has to be rebuilt against it.
anchor build

PROGRAM_ID="$(solana address -k target/deploy/prediction_market-keypair.json)"
BINARY_BYTES="$(wc -c < target/deploy/prediction_market.so | tr -d ' ')"

# `solana program deploy` allocates twice the binary so it can be upgraded in
# place. Checking first turns an out-of-funds failure -- which strands a buffer
# account holding real lamports -- into a refusal that costs nothing.
NEEDED_LAMPORTS="$(solana rent $((BINARY_BYTES * 2)) -u "$RPC" --output json | sed -n 's/.*"rentExemptMinimumLamports"[[:space:]]*:[[:space:]]*\([0-9]*\).*/\1/p')"
BALANCE_LAMPORTS="$(solana balance "$PAYER" -u "$RPC" --lamports | awk '{print $1}')"

echo "==> Program id:   $PROGRAM_ID"
echo "    binary:       $BINARY_BYTES bytes"
echo "    deploy costs: $((NEEDED_LAMPORTS / 1000000000)).$(printf '%09d' $((NEEDED_LAMPORTS % 1000000000))) SOL"
echo "    balance:      $((BALANCE_LAMPORTS / 1000000000)).$(printf '%09d' $((BALANCE_LAMPORTS % 1000000000))) SOL"

if [ "$BALANCE_LAMPORTS" -lt "$((NEEDED_LAMPORTS + 100000000))" ]; then
  echo "Not enough to deploy and leave a margin for fees." >&2
  exit 1
fi

# An upgrade cannot write more bytes than the program account already holds, and
# the loader refuses to grow it by less than 10240 at a time -- so a binary that
# grew by even one byte fails to deploy until the account is extended by hand.
# `-k` matters: without a signer the command errors out rather than reporting,
# and a first deployment has no account to report on at all.
ON_CHAIN_BYTES="$(solana program show "$PROGRAM_ID" -u "$RPC" -k "$WALLET" 2>/dev/null \
  | sed -n 's/^Data Length: \([0-9]*\).*/\1/p' || true)"
if [ -n "$ON_CHAIN_BYTES" ] && [ "$BINARY_BYTES" -gt "$ON_CHAIN_BYTES" ]; then
  GROWTH=$((BINARY_BYTES - ON_CHAIN_BYTES))
  [ "$GROWTH" -lt 10240 ] && GROWTH=10240
  echo "==> Extending the program account by $GROWTH bytes"
  solana program extend "$PROGRAM_ID" "$GROWTH" -u "$RPC" -k "$WALLET"
fi

echo "==> Deploying"
# Anchor also writes an IDL account for explorers to read, and that write can
# fail on its own, so its exit code alone cannot be trusted either way. What
# landed is checked against the binary below instead.
anchor deploy --provider.cluster "$RPC" --provider.wallet "$WALLET" \
  || echo "    (anchor reported a failure; checking what actually landed)"

echo "==> Verifying the program on chain is the binary just built"
solana program show "$PROGRAM_ID" -u "$RPC" -k "$WALLET"
DUMP="$(mktemp)"
trap 'rm -f "$DUMP"' EXIT
solana program dump "$PROGRAM_ID" "$DUMP" -u "$RPC" >/dev/null
# The dump is padded out to the account's length, so only the binary's own
# bytes are compared.
LOCAL_HASH="$(shasum -a 256 target/deploy/prediction_market.so | awk '{print $1}')"
CHAIN_HASH="$(head -c "$BINARY_BYTES" "$DUMP" | shasum -a 256 | awk '{print $1}')"
if [ "$LOCAL_HASH" != "$CHAIN_HASH" ]; then
  echo "The program on chain is not the binary just built. Nothing was deployed." >&2
  exit 1
fi
echo "    matches: $LOCAL_HASH"

echo "==> Refreshing the client IDL"
node packages/sdk/scripts/copy-idl.mjs

cat <<DONE

Deployed.

  program   $PROGRAM_ID
  cluster   $CLUSTER

The client reads the address out of the IDL, so nothing else needs changing.
The client reads it from there. To run the site:

  pnpm --filter @prediction-market/web dev

A freshly deployed program has no configuration, no registered price sources and
no markets. See the README for what has to happen next.
DONE
