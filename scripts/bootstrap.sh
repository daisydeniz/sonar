#!/usr/bin/env bash
# Toolchain setup/check for Sonar on macOS (Apple Silicon).
# Idempotent: safe to run any time. Exits non-zero with instructions if a
# step needs the founder (e.g. installing Xcode).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
fail() { echo "✗ $*" >&2; exit 1; }
ok()   { echo "✓ $*"; }
# shellcheck source=lib/env.sh
source "$ROOT/scripts/lib/env.sh"

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
  echo "… installing rustup via Homebrew"
  brew install rustup || fail "brew install rustup failed. Alternative: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  # shellcheck source=lib/env.sh
  source "$ROOT/scripts/lib/env.sh"   # keg-only: put its bin dir on PATH now
  command -v rustup >/dev/null || fail "rustup installed but not on PATH. Add $(brew --prefix rustup)/bin to PATH and re-run."
fi
ok "$(rustup --version 2>/dev/null | head -1)"
# A default toolchain lets cargo work outside core/ (e.g. `cargo install`).
rustup default >/dev/null 2>&1 || rustup default stable
# Install the toolchain, components and targets pinned in rust-toolchain.toml.
# (rustup >= 1.28 no longer auto-installs on `rustup show`.)
# Older rustup (< 1.28) rejects the argument-less form and installs on `show`;
# any real failure is caught by the strict rustc/cargo checks below.
(cd "$ROOT/core" && { rustup toolchain install >/dev/null || rustup show active-toolchain >/dev/null; })
(cd "$ROOT/core" && rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null)
# Capture versions in variables first: a failing $(...) inside an argument
# would not trip `set -e` and would print an empty "✓".
v="$(cd "$ROOT/core" && rustc --version)"; ok "$v"
v="$(cd "$ROOT/core" && cargo --version)"; ok "$v"

if ! command -v cargo-deny >/dev/null; then (cd "$ROOT/core" && cargo install cargo-deny --locked); fi
v="$(cd "$ROOT/core" && cargo deny --version)"; ok "$v"

if ! grep -qs -e '.cargo/bin' -e '.cargo/env' -e 'opt/rustup/bin' \
    "$HOME/.zprofile" "$HOME/.zshrc" "$HOME/.zshenv" "$HOME/.profile" "$HOME/.bash_profile" "$HOME/.bashrc"; then
  echo "! Rust is not on your shell PATH yet (scripts handle it themselves). For interactive use add to ~/.zprofile:"
  echo "    export PATH=\"$(brew --prefix)/opt/rustup/bin:\$HOME/.cargo/bin:\$PATH\""
fi

command -v claude >/dev/null || fail "Claude Code CLI missing: https://docs.claude.com/en/docs/claude-code"
ok "claude $(claude --version 2>/dev/null | head -1)"

if command -v gh >/dev/null; then ok "gh $(gh --version | head -1 | awk '{print $3}')"; else echo "! gh not installed (optional): brew install gh"; fi

echo
echo "Bootstrap OK. Next: scripts/check.sh"
