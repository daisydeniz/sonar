#!/usr/bin/env bash
# Runs the autonomous build loop: one fresh Claude Code session per ROADMAP
# task until the roadmap is complete, the loop is blocked, or MAX_ITER is hit.
#
#   scripts/autopilot.sh                 # defaults
#   MAX_ITER=10 scripts/autopilot.sh     # limit iterations
#   SONAR_YOLO=1 scripts/autopilot.sh    # skip ALL permission checks (not recommended)
#
# Logs: logs/<timestamp>-iterN.log   Progress: docs/STATUS.md, git log
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Keep the Mac awake while the loop runs.
if [[ -z "${SONAR_CAFFEINATED:-}" ]] && command -v caffeinate >/dev/null; then
  SONAR_CAFFEINATED=1 exec caffeinate -i "$0" "$@"
fi

MAX_ITER="${MAX_ITER:-200}"
MAX_TURNS="${MAX_TURNS:-300}"
BACKOFF_SECS="${BACKOFF_SECS:-900}"   # wait after an error (e.g. usage limit)
MAX_FAILS="${MAX_FAILS:-24}"     # 24 × 15 min ≈ 6 h: rides out usage-limit windows

PERM_ARGS=(--permission-mode acceptEdits)
if [[ "${SONAR_YOLO:-0}" == "1" ]]; then
  echo "! SONAR_YOLO=1: running with --dangerously-skip-permissions"
  PERM_ARGS=(--dangerously-skip-permissions)
fi

# Claude Code ignores .claude/settings.json allow rules until the folder is
# trusted (interactive trust dialog). Fail fast instead of burning a session.
trust_ok() {
  python3 - "$(pwd -P)" <<'PY'
import json, os, sys
try:
    data = json.load(open(os.path.expanduser("~/.claude.json")))
except Exception:
    sys.exit(1)
sys.exit(0 if data.get("projects", {}).get(sys.argv[1], {}).get("hasTrustDialogAccepted") else 1)
PY
}
if [[ "${SONAR_SKIP_TRUST_CHECK:-0}" != "1" ]] && ! trust_ok; then
  echo "✗ This folder is not trusted by Claude Code yet, so its permission rules are ignored."
  echo "  Fix (once): run 'claude' in $(pwd -P), accept the trust prompt, type /exit, then rerun this script."
  exit 3
fi

PROMPT='Run the /next command (one iteration of the build loop). If slash commands are unavailable, read .claude/commands/next.md and follow it exactly.'

mkdir -p logs
fails=0
for ((i = 1; i <= MAX_ITER; i++)); do
  state="$(grep -m1 -o 'Loop state: [A-Z ]*' docs/STATUS.md | sed 's/Loop state: //; s/ *$//')"
  case "$state" in
    "ROADMAP COMPLETE") echo "✓ Roadmap complete."; exit 0 ;;
    "BLOCKED") echo "■ Loop is BLOCKED — see 'Needs human' in docs/STATUS.md."; exit 2 ;;
  esac

  log="logs/$(date +%Y%m%d-%H%M%S)-iter$i.log"
  echo "── iteration $i/$MAX_ITER → $log"
  set +e
  claude -p "$PROMPT" "${PERM_ARGS[@]}" --max-turns "$MAX_TURNS" --output-format text 2>&1 | tee "$log"
  code=${PIPESTATUS[0]}
  set -e

  result="$(grep -E '^RESULT:' "$log" | tail -1 || true)"
  echo "   ${result:-RESULT: (none)} (exit $code)"

  case "$result" in
    "RESULT: COMPLETE"*) echo "✓ Roadmap complete."; exit 0 ;;
    "RESULT: BLOCKED"*)  echo "■ Blocked — see docs/STATUS.md."; exit 2 ;;
    "RESULT: DONE"*|"RESULT: SKIPPED"*) fails=0 ;;
    *)
      fails=$((fails + 1))
      if (( fails >= MAX_FAILS )); then
        echo "✗ $fails consecutive iterations without a result. Stopping; check $log"; exit 1
      fi
      echo "   no result (exit $code); retrying in ${BACKOFF_SECS}s"
      sleep "$BACKOFF_SECS"
      ;;
  esac
done
echo "Reached MAX_ITER=$MAX_ITER."
