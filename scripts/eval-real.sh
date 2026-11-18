#!/usr/bin/env bash
# `sonar eval` on the founder's real samples (silver and/or golden tier).
#
# Agents run this instead of typing `--silver "$SONAR_SILVER_DIR"`: headless
# sessions deny commands that expand variables or call printenv/env, so the
# real-sample tiers were silently skipped before (T10.2).
#
#   scripts/eval-real.sh                     # silver + golden, whatever exists
#   scripts/eval-real.sh --fixtures --check  # extra `sonar eval` args pass through
#
# Silver: $SONAR_SILVER_DIR, else <repo>/Samples (gitignored, read-only).
# Golden: $SONAR_GOLDEN_DIR, else <repo>/eval/golden when it has labels.csv.
# Exits 2 when neither exists (nothing to evaluate).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=lib/env.sh
source "$ROOT/scripts/lib/env.sh"

silver="${SONAR_SILVER_DIR:-$ROOT/Samples}"
golden="${SONAR_GOLDEN_DIR:-$ROOT/eval/golden}"

args=()
if [[ -d "$silver" ]]; then
  echo "silver: $silver"
  args+=(--silver "$silver")
else
  echo "silver: not found ($silver)"
fi
if [[ -f "$golden/labels.csv" ]]; then
  echo "golden: $golden"
  args+=(--golden --golden-dir "$golden")
else
  echo "golden: not found ($golden/labels.csv)"
fi
if [[ ${#args[@]} -eq 0 ]]; then
  echo "eval-real: no real-sample set available" >&2
  exit 2
fi

cd "$ROOT/core"
cargo build --release -q -p sonar-cli
exec "$ROOT/core/target/release/sonar" eval "${args[@]}" "$@"
