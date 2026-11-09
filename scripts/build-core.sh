#!/usr/bin/env bash
# Build the Rust core for the macOS app (task T0.5, ADR-002).
#
# Outputs (gitignored, under app/Generated/):
#   SonarCore.xcframework  static lib libsonar_ffi.a (macos-arm64) + headers
#                          (SonarCoreFFI.h, module.modulemap -> C module
#                          `SonarCoreFFI`)
#   SonarCore.swift        UniFFI Swift bindings; compile it into the app
#                          target, it does `import SonarCoreFFI`.
#
# Steps: cargo build sonar-ffi (staticlib) -> uniffi-bindgen in library mode
# -> xcodebuild -create-xcframework. Called by scripts/check.sh before
# xcodegen/xcodebuild; the cargo build deliberately lives here and not in an
# Xcode script phase.
#
# Env:
#   SONAR_CORE_PROFILE=debug|release  (default: debug). debug keeps check.sh
#       fast (workspace deps are already opt-level 2 in dev); the release
#       pipeline must set `release` (thin LTO, codegen-units=1).
#
# Architectures: arm64 only (aarch64-apple-darwin), matching `ARCHS: arm64`
# in app/project.yml. x86_64 is optional and not built yet; adding it means a
# second cargo target + `lipo -create` before -create-xcframework.
#
# Idempotent and incremental: cargo only rebuilds what changed, and the
# outputs in app/Generated/ are only replaced when their content changed, so
# Xcode does not recompile/relink needlessly.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=lib/env.sh
source "$ROOT/scripts/lib/env.sh"
# Output paths below assume the default target dir and an explicit --target.
unset CARGO_TARGET_DIR CARGO_BUILD_TARGET

TARGET="aarch64-apple-darwin"
PROFILE="${SONAR_CORE_PROFILE:-debug}"
case "$PROFILE" in
  debug)   CARGO_PROFILE_ARGS=() ;;
  release) CARGO_PROFILE_ARGS=(--release) ;;
  *) echo "build-core: SONAR_CORE_PROFILE must be debug or release (got '$PROFILE')" >&2; exit 2 ;;
esac

CORE="$ROOT/core"
OUT="$ROOT/app/Generated"
XCFRAMEWORK="$OUT/SonarCore.xcframework"
SWIFT_OUT="$OUT/SonarCore.swift"
LIB="$CORE/target/$TARGET/$PROFILE/libsonar_ffi.a"

# The static lib must match the app's deployment target (app/project.yml),
# otherwise the linker warns about objects built for a newer macOS.
export MACOSX_DEPLOYMENT_TARGET="14.0"

cd "$CORE"  # rust-toolchain.toml applies from here

if ! rustup target list --installed | grep -qx "$TARGET"; then
  rustup target add "$TARGET"
fi

cargo build -q -p sonar-ffi --lib --target "$TARGET" "${CARGO_PROFILE_ARGS[@]+"${CARGO_PROFILE_ARGS[@]}"}"
# Host build of the generator (matches the workspace uniffi version).
cargo build -q -p uniffi-bindgen
BINDGEN="$CORE/target/debug/uniffi-bindgen"

[[ -f "$LIB" ]] || { echo "build-core: missing $LIB" >&2; exit 1; }

STAGE="$(mktemp -d "${TMPDIR:-/tmp}/sonar-build-core.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT

# Library mode: metadata is read from the built lib; uniffi.toml of the
# sonar-ffi crate (found via cargo metadata, hence cwd = core/) sets the
# module names. --no-format keeps output deterministic.
"$BINDGEN" generate "$LIB" --language swift --no-format --out-dir "$STAGE/bindings" >/dev/null

mkdir -p "$STAGE/headers"
cp "$STAGE/bindings/SonarCoreFFI.h" "$STAGE/headers/"
# Swift only discovers the C module of an xcframework via `module.modulemap`.
cp "$STAGE/bindings/SonarCoreFFI.modulemap" "$STAGE/headers/module.modulemap"

xcodebuild -create-xcframework \
  -library "$LIB" \
  -headers "$STAGE/headers" \
  -output "$STAGE/SonarCore.xcframework" >/dev/null

mkdir -p "$OUT"
if [[ -d "$XCFRAMEWORK" ]] && diff -rq "$STAGE/SonarCore.xcframework" "$XCFRAMEWORK" >/dev/null 2>&1; then
  :  # unchanged; keep mtimes so Xcode does not relink
else
  rm -rf "$XCFRAMEWORK"  # xcodebuild/mv refuse to overwrite a framework dir
  mv "$STAGE/SonarCore.xcframework" "$XCFRAMEWORK"
fi
if ! cmp -s "$STAGE/bindings/SonarCore.swift" "$SWIFT_OUT"; then
  cp "$STAGE/bindings/SonarCore.swift" "$SWIFT_OUT"
fi

echo "build-core: $PROFILE $TARGET -> ${XCFRAMEWORK#"$ROOT/"}, ${SWIFT_OUT#"$ROOT/"}"
