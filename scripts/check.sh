#!/usr/bin/env bash
# All quality gates. Must be green before any commit (see CLAUDE.md).
# Stages are added as the project grows; the architect agent owns this file.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
step() { printf '\n▶ %s\n' "$*"; }
# shellcheck source=lib/env.sh
source "$ROOT/scripts/lib/env.sh"

cd "$ROOT/core"
step "rustfmt";  cargo fmt --all --check
step "clippy";   cargo clippy --workspace --all-targets -- -D warnings
step "tests";    cargo test --workspace

if [[ -f deny.toml ]]; then
  step "cargo deny (licenses, bans, sources)"
  cargo deny check licenses bans sources
fi

# Fixture eval gate: once `sonar eval` exists it must support
# `--fixtures --check` and exit non-zero on regression vs BASELINE.
SONAR_BIN="$ROOT/core/target/debug/sonar"
cargo build -q -p sonar-cli
if "$SONAR_BIN" eval --help >/dev/null 2>&1; then
  step "eval (fixtures, regression check)"
  "$SONAR_BIN" eval --fixtures --check
fi

if [[ "$(uname -s)" == "Darwin" && -f "$ROOT/app/project.yml" ]]; then
  # The app links SonarCore.xcframework; it cannot build without this step.
  step "build core xcframework"; "$ROOT/scripts/build-core.sh"
  step "xcodegen"; (cd "$ROOT/app" && xcodegen generate --quiet)
  step "xcodebuild build"
  xcodebuild -project "$ROOT/app/Sonar.xcodeproj" -scheme Sonar \
    -destination 'platform=macOS' -quiet build
  if [[ "${SONAR_SKIP_XCTEST:-0}" != "1" ]] && grep -q "SonarTests" "$ROOT/app/project.yml"; then
    step "xcodebuild test"
    # EngineBridgeTests drives a `sonar fixtures` library through the FFI.
    # xcodebuild strips the TEST_RUNNER_ prefix, so the tests see
    # SONAR_FIXTURES_DIR.
    FIXTURES_DIR="$(mktemp -d "${TMPDIR:-/tmp}/sonar-xctest-fixtures.XXXXXX")"
    trap 'rm -rf "$FIXTURES_DIR"' EXIT
    "$SONAR_BIN" fixtures "$FIXTURES_DIR" --skip-long >/dev/null
    # -quiet hides assertion text; on failure print it from the result bundle.
    RESULT_BUNDLE="$FIXTURES_DIR/xctest.xcresult"
    if ! TEST_RUNNER_SONAR_FIXTURES_DIR="$FIXTURES_DIR" TEST_RUNNER_SONAR_REQUIRE_FIXTURES=1 xcodebuild -project "$ROOT/app/Sonar.xcodeproj" -scheme Sonar \
      -destination 'platform=macOS' -resultBundlePath "$RESULT_BUNDLE" -quiet test; then
      xcrun xcresulttool get test-results summary --path "$RESULT_BUNDLE" --compact || true
      exit 1
    fi
  fi
fi

printf '\n✓ all checks passed\n'
