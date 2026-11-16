#!/usr/bin/env bash
# Renders the main screens (light + dark, deterministic sample data) to
# logs/ui/<screen>-<light|dark>.png so agents can open them (docs/DESIGN.md §6).
# No Screen Recording permission, no real library. `--rebuild` rebuilds the
# core xcframework and regenerates the Xcode project first.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=lib/env.sh
source "$ROOT/scripts/lib/env.sh"

OUT="$ROOT/logs/ui"
mkdir -p "$OUT"
find "$OUT" -maxdepth 1 -name '*.png' -delete

if [[ "${1:-}" == "--rebuild" || ! -d "$ROOT/app/Generated/SonarCore.xcframework" ]]; then
  "$ROOT/scripts/build-core.sh"
fi
if [[ "${1:-}" == "--rebuild" || ! -d "$ROOT/app/Sonar.xcodeproj" ]]; then
  (cd "$ROOT/app" && xcodegen generate --quiet)
fi

# xcodebuild strips the TEST_RUNNER_ prefix, so the test sees SONAR_UI_SNAPSHOT_DIR.
LOG="$ROOT/logs/ui-snapshots.log"
if ! TEST_RUNNER_SONAR_UI_SNAPSHOT_DIR="$OUT" xcodebuild -project "$ROOT/app/Sonar.xcodeproj" \
  -scheme Sonar -destination 'platform=macOS' -only-testing:SonarTests/UISnapshotTests \
  -quiet test > "$LOG" 2>&1; then
  tail -40 "$LOG"
  echo "ui-snapshots failed (full log: logs/ui-snapshots.log)" >&2
  exit 1
fi
ls -1 "$OUT"/*.png
