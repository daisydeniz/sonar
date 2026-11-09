# shellcheck shell=bash
# Shared environment for Sonar scripts. Source it, do not execute it.
# Puts the Rust toolchain on PATH even when the founder's shell profile has
# not been updated yet (fresh rustup install, non-login shells, autopilot).

# Homebrew's rustup is keg-only, so its proxies (cargo, rustc, ...) are not
# linked into the brew prefix. ~/.cargo/bin holds `cargo install` binaries
# (e.g. cargo-deny) and the proxies of a curl-installed rustup.
# Each dir is prepended, so the last one listed ends up first on PATH; both
# kinds of rustup proxies share ~/.rustup, so the order is not significant.
for _sonar_dir in \
  "$HOME/.cargo/bin" \
  "/usr/local/opt/rustup/bin" \
  "/opt/homebrew/opt/rustup/bin"; do
  if [[ -d "$_sonar_dir" && ":$PATH:" != *":$_sonar_dir:"* ]]; then
    PATH="$_sonar_dir:$PATH"
  fi
done
unset _sonar_dir
export PATH
