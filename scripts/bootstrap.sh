#!/usr/bin/env bash
# Toolchain setup/check for Sonar on macOS (Apple Silicon).
# Idempotent: safe to run any time. Exits non-zero with instructions if a
# step needs the founder (e.g. installing Xcode).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
fail() { echo "✗ $*" >&2; exit 1; }
ok()   { echo "✓ $*"; }

[[ "$(uname -s)" == "Darwin" ]] || fail "macOS required (found $(uname -s))."
[[ "$(uname -m)" == "arm64" ]] || echo "! Not arm64 ($(uname -m)); Apple Silicon is the primary target."
ok "macOS $(sw_vers -productVersion) $(uname -m)"

# Full Xcode (not only Command Line Tools) is required for app builds.
if ! xcodebuild -version >/dev/null 2>&1; then
  fail "Full Xcode not found. Install Xcode from the App Store, then run:
  sudo xcode-select -s /Applications/Xcode.app/Contents/Developer && sudo xcodebuild -runFirstLaunch"
fi
ok "$(xcodebuild -version | head -1)"

command -v brew >/dev/null || fail "Homebrew missing: https://brew.sh"
command -v xcodegen >/dev/null || brew install xcodegen
ok "xcodegen $(xcodegen --version 2>/dev/null | head -1)"

if ! command -v rustup >/dev/null; then
  fail "rustup missing. Install: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh  (then restart the shell)"
fi
(cd "$ROOT/core" && rustup show active-toolchain >/dev/null)   # installs toolchain from rust-toolchain.toml
rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
ok "$(cd "$ROOT/core" && rustc --version)"

if ! command -v cargo-deny >/dev/null; then cargo install cargo-deny --locked; fi
ok "$(cargo deny --version)"

command -v claude >/dev/null || fail "Claude Code CLI missing: https://docs.claude.com/en/docs/claude-code"
ok "claude $(claude --version 2>/dev/null | head -1)"

if command -v gh >/dev/null; then ok "gh $(gh --version | head -1 | awk '{print $3}')"; else echo "! gh not installed (optional): brew install gh"; fi

echo
echo "Bootstrap OK. Next: scripts/check.sh"
