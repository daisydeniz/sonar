#!/usr/bin/env bash
# Runs the autonomous build loop: one fresh Claude Code session per ROADMAP
# task until the roadmap is complete, the loop is blocked, or MAX_ITER task
# attempts have been made.
#
#   scripts/autopilot.sh                 # defaults
#   MAX_ITER=10 scripts/autopilot.sh     # limit task attempts
#   SONAR_MODEL=sonnet scripts/autopilot.sh   # orchestrator model (default opus)
#   SONAR_YOLO=1 scripts/autopilot.sh    # skip ALL permission checks (not recommended)
#
# Usage limits: when Claude reports "hit your … limit · resets <time>", the
# loop sleeps until that time (+2 min) and retries. Limit waits do not count
# as attempts or failures.
#
# The script runs from a temporary copy of itself, so agents may update
# scripts/autopilot.sh in the repo while the loop is running.
#
# Logs: logs/<timestamp>-iterN.log   Live state: logs/autopilot.status
set -euo pipefail

# ── locate repo root ────────────────────────────────────────────────────────
if [[ -z "${SONAR_ROOT:-}" ]]; then
  script_dir="$(cd "$(dirname "$0")" && pwd)"
  if [[ -f "$script_dir/../CLAUDE.md" ]]; then
    SONAR_ROOT="$(cd "$script_dir/.." && pwd)"
  else
    SONAR_ROOT="$(git rev-parse --show-toplevel)"
  fi
fi
export SONAR_ROOT
cd "$SONAR_ROOT"

# ── run from a private copy (repo edits can't corrupt the running script) ──
if [[ -z "${SONAR_AUTOPILOT_COPY:-}" ]]; then
  copy="$(mktemp "${TMPDIR:-/tmp}/sonar-autopilot.XXXXXX")"
  cp "$0" "$copy"
  export SONAR_AUTOPILOT_COPY="$copy"
  # Keep the Mac awake (idle sleep) while the loop runs.
  if command -v caffeinate >/dev/null; then
    exec caffeinate -i bash "$copy" "$@"
  fi
  exec bash "$copy" "$@"
fi
trap 'rm -f "$SONAR_AUTOPILOT_COPY"' EXIT

# Rust toolchain on PATH for the Claude sessions (they run cargo directly).
# shellcheck source=/dev/null
source "$SONAR_ROOT/scripts/lib/env.sh"

MAX_ITER="${MAX_ITER:-200}"          # task attempts (limit waits not counted)
MAX_TURNS="${MAX_TURNS:-300}"
MAX_FAILS="${MAX_FAILS:-5}"          # consecutive real failures (not limits)
FAIL_BACKOFF_SECS="${FAIL_BACKOFF_SECS:-300}"
MODEL="${SONAR_MODEL:-opus}"

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

mkdir -p logs
status() { printf '%s  %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*" > logs/autopilot.status; }

# Seconds until the reset time named in a usage-limit message (+120 s).
# Understands "resets 4:30pm (Europe/Istanbul)", "resets 6am",
# "resets Nov 12, 9am (…)". Prints 900 if it cannot parse.
seconds_until_reset() {
  python3 - "$1" <<'PY'
import datetime as dt, re, sys
try:
    from zoneinfo import ZoneInfo
except Exception:
    ZoneInfo = None
text = open(sys.argv[1], errors="ignore").read()
m = re.search(r"resets\s+(?:(?P<mon>[A-Za-z]{3})[a-z]*\.?\s+(?P<day>\d{1,2}),?\s+(?:at\s+)?)?"
              r"(?P<h>\d{1,2})(?::(?P<mi>\d{2}))?\s*(?P<ap>am|pm)(?:\s*\((?P<tz>[^)]+)\))?",
              text, re.I)
if not m:
    print(900); sys.exit()
tz = None
if m["tz"] and ZoneInfo:
    try:
        tz = ZoneInfo(m["tz"].strip())
    except Exception:
        tz = None
now = dt.datetime.now(tz) if tz else dt.datetime.now()
hour = int(m["h"]) % 12 + (12 if m["ap"].lower() == "pm" else 0)
target = now.replace(hour=hour, minute=int(m["mi"] or 0), second=0, microsecond=0)
if m["mon"]:
    month = dt.datetime.strptime(m["mon"][:3].title(), "%b").month
    target = target.replace(month=month, day=int(m["day"]))
    if target < now - dt.timedelta(days=1):
        target = target.replace(year=target.year + 1)
elif target <= now:
    # Just passed (message is fresh) → retry now; otherwise it means tomorrow.
    target = now if now - target < dt.timedelta(minutes=30) else target + dt.timedelta(days=1)
secs = int((target - now).total_seconds()) + 120
print(max(60, min(secs, 8 * 24 * 3600)))
PY
}

# Sleep by wall clock in 60 s steps, so system sleep doesn't stretch the wait.
sleep_until_epoch() {
  while (( $(date +%s) < $1 )); do sleep 60; done
}

PROMPT='Run the /next command (one iteration of the build loop). If slash commands are unavailable, read .claude/commands/next.md and follow it exactly.'

attempt=0
fails=0
run=0
while (( attempt < MAX_ITER )); do
  state="$(grep -m1 -o 'Loop state: [A-Z ]*' docs/STATUS.md | sed 's/Loop state: //; s/ *$//')"
  case "$state" in
    "ROADMAP COMPLETE") status "complete"; echo "✓ Roadmap complete."; exit 0 ;;
    "BLOCKED") status "blocked"; echo "■ Loop is BLOCKED — see 'Needs human' in docs/STATUS.md."; exit 2 ;;
  esac

  run=$((run + 1))
  log="logs/$(date +%Y%m%d-%H%M%S)-run$run.log"
  echo "── run $run (attempt $((attempt + 1))/$MAX_ITER, model $MODEL) → $log"
  status "running · run $run · attempt $((attempt + 1)) · $log"
  set +e
  claude -p "$PROMPT" --model "$MODEL" "${PERM_ARGS[@]}" --max-turns "$MAX_TURNS" \
    --output-format text 2>&1 | tee "$log"
  code=${PIPESTATUS[0]}
  set -e

  # Usage limit → wait for reset; not an attempt, not a failure.
  if grep -qiE "hit your .*limit|usage limit reached|limit reached.*resets" "$log"; then
    secs="$(seconds_until_reset "$log")"
    until_epoch=$(( $(date +%s) + secs ))
    until_h="$(date -r "$until_epoch" '+%H:%M' 2>/dev/null || date -d "@$until_epoch" '+%H:%M')"
    echo "   usage limit — sleeping until ~$until_h"
    status "waiting for usage limit reset until ~$until_h"
    sleep_until_epoch "$until_epoch"
    continue
  fi

  attempt=$((attempt + 1))
  result="$(grep -E '^RESULT:' "$log" | tail -1 || true)"
  echo "   ${result:-RESULT: (none)} (exit $code)"

  case "$result" in
    "RESULT: COMPLETE"*) status "complete"; echo "✓ Roadmap complete."; exit 0 ;;
    "RESULT: BLOCKED"*)  status "blocked"; echo "■ Blocked — see docs/STATUS.md."; exit 2 ;;
    "RESULT: DONE"*|"RESULT: SKIPPED"*) fails=0 ;;
    *)
      fails=$((fails + 1))
      if (( fails >= MAX_FAILS )); then
        status "stopped: $fails consecutive failures"
        echo "✗ $fails consecutive runs without a result. Stopping; check $log"; exit 1
      fi
      echo "   no result (exit $code); retrying in ${FAIL_BACKOFF_SECS}s"
      status "retrying after failure ($fails/$MAX_FAILS)"
      sleep "$FAIL_BACKOFF_SECS"
      ;;
  esac
done
status "stopped: reached MAX_ITER=$MAX_ITER"
echo "Reached MAX_ITER=$MAX_ITER."
