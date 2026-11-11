#!/usr/bin/env bash
# Runs the autonomous build loop: one fresh Claude Code session per ROADMAP
# task until the roadmap is complete, the loop is blocked, or MAX_ITER task
# attempts have been made.
#
#   scripts/autopilot.sh                 # defaults
#   MAX_ITER=10 scripts/autopilot.sh     # limit task attempts
#   SONAR_MODEL=sonnet scripts/autopilot.sh   # orchestrator model (default opus)
#   SONAR_YOLO=1 scripts/autopilot.sh    # skip ALL permission checks (not recommended)
#   SONAR_OUTPUT_FORMAT=text scripts/autopilot.sh   # old plain-text logs, no usage.tsv
#   SONAR_SILVER_DIR=~/Splice/sounds SONAR_GOLDEN_DIR=~/sonar-golden scripts/autopilot.sh
#                                        # real-audio eval sets (see CLAUDE.md, loop step 6)
#
# Usage limits: when Claude reports "hit your … limit · resets <time>", the
# loop sleeps until that time (+2 min) and retries. Limit waits do not count
# as attempts or failures.
#
# The script runs from a temporary copy of itself, so agents may update
# scripts/autopilot.sh in the repo while the loop is running.
#
# Logs: logs/<timestamp>-runN.log (final message), logs/<timestamp>-runN.json
# (raw result), logs/usage.tsv (turns, duration, cost per run; per model when
# reported).   Live state: logs/autopilot.status
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
OUT_FORMAT="${SONAR_OUTPUT_FORMAT:-json}"   # json (usage logged) | text

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

# Turn the JSON result of `claude -p --output-format json` into the plain-text
# log the rest of this script greps, and append one line to logs/usage.tsv.
# If the output is not the expected JSON, the log is the raw output.
render_log() {  # <raw json> <stderr file> <log>
  python3 - "$1" "$2" "$3" <<'PY'
import json, os, sys, time
raw_path, err_path, log_path = sys.argv[1:4]
raw = open(raw_path, errors="replace").read()
err = open(err_path, errors="replace").read() if os.path.exists(err_path) else ""

def pick(obj):
    if isinstance(obj, dict) and obj.get("type") == "result":
        return obj
    if isinstance(obj, list):
        for item in reversed(obj):
            if isinstance(item, dict) and item.get("type") == "result":
                return item
    return None

res = None
try:
    res = pick(json.loads(raw))
except Exception:
    for line in reversed(raw.splitlines()):
        line = line.strip()
        if not line.startswith(("{", "[")):
            continue
        try:
            res = pick(json.loads(line))
        except Exception:
            res = None
        if res:
            break

text = res.get("result") if res else None
if not isinstance(text, str) or not text.strip():
    text = raw
with open(log_path, "w") as f:
    f.write(err)
    f.write(text if text.endswith("\n") else text + "\n")

if res:
    models = res.get("modelUsage")
    per_model = ""
    if isinstance(models, dict):
        per_model = " ".join(
            "%s=%s" % (name, usage.get("costUSD", "?"))
            for name, usage in models.items() if isinstance(usage, dict))
    row = [time.strftime("%Y-%m-%d %H:%M:%S"), os.path.basename(log_path),
           res.get("num_turns", ""), res.get("duration_ms", ""),
           res.get("total_cost_usd", ""), res.get("is_error", ""), per_model]
    usage_path = os.path.join(os.path.dirname(log_path) or ".", "usage.tsv")
    new = not os.path.exists(usage_path)
    with open(usage_path, "a") as f:
        if new:
            f.write("time\tlog\tturns\tduration_ms\tcost_usd\tis_error\tper_model_cost\n")
        f.write("\t".join(str(v) for v in row) + "\n")
PY
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
  if [[ "$OUT_FORMAT" == "json" ]]; then
    raw="${log%.log}.json"
    claude -p "$PROMPT" --model "$MODEL" "${PERM_ARGS[@]}" --max-turns "$MAX_TURNS" \
      --output-format json > "$raw" 2> "$raw.err"
    code=$?
    render_log "$raw" "$raw.err" "$log" || cat "$raw.err" "$raw" > "$log"
    rm -f "$raw.err"
    cat "$log"
  else
    raw="$log"
    claude -p "$PROMPT" --model "$MODEL" "${PERM_ARGS[@]}" --max-turns "$MAX_TURNS" \
      --output-format text 2>&1 | tee "$log"
    code=${PIPESTATUS[0]}
  fi
  set -e

  # Usage limit → wait for reset; not an attempt, not a failure.
  limit_file=""
  for f in "$log" "$raw"; do
    if grep -qiE "hit your .*limit|usage limit reached|limit reached.*resets" "$f"; then
      limit_file="$f"; break
    fi
  done
  if [[ -n "$limit_file" ]]; then
    secs="$(seconds_until_reset "$limit_file")"
    until_epoch=$(( $(date +%s) + secs ))
    until_h="$(date -r "$until_epoch" '+%H:%M' 2>/dev/null || date -d "@$until_epoch" '+%H:%M')"
    echo "   usage limit — sleeping until ~$until_h"
    status "waiting for usage limit reset until ~$until_h"
    sleep_until_epoch "$until_epoch"
    continue
  fi

  attempt=$((attempt + 1))
  result="$(grep -E '^RESULT:' "$log" | tail -1 || true)"
  if [[ -z "$result" ]]; then   # result text not rendered (unexpected JSON shape)
    result="$(grep -oE 'RESULT: (DONE|SKIPPED|BLOCKED|COMPLETE)[^"\\]*' "$raw" | tail -1 || true)"
  fi
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
