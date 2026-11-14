#!/usr/bin/env bash
# Release pipeline (task T7.3, ADR-037): universal build -> codesign (hardened
# runtime) -> notarize -> DMG -> Sparkle appcast. Output goes to dist/
# (gitignored). Nothing is uploaded or published.
#
# Works with NO credentials: it then produces an ad-hoc signed, unnotarized
# DMG and prints what was skipped (that DMG runs on this Mac only).
#
# Usage: scripts/release.sh [--arch universal|arm64] [--version X.Y.Z]
#                           [--build N] [--dry-run] [--skip-build] [-h|--help]
#   --arch      universal (default) = arm64 + x86_64; arm64 = Apple Silicon only
#   --version   marketing version (default: MARKETING_VERSION in app/project.yml)
#   --build     build number (default: CURRENT_PROJECT_VERSION in project.yml)
#   --dry-run   ignore all credentials: ad-hoc sign, no notarize, no appcast
#   --skip-build  reuse dist/build from an earlier run (re-sign/DMG only)
#
# Environment (all optional; see ADR-037):
#   SONAR_SIGN_IDENTITY       "Developer ID Application: Name (TEAMID)"
#   SONAR_NOTARY_PROFILE      notarytool keychain profile
#                             (`xcrun notarytool store-credentials <name>`), or
#   SONAR_NOTARY_KEY_ID / SONAR_NOTARY_ISSUER / SONAR_NOTARY_KEY_PATH
#                             App Store Connect API key (.p8) triple
#   SONAR_SPARKLE_FEED_URL    appcast URL baked into the app (SUFeedURL)
#   SONAR_SPARKLE_PUBLIC_KEY  Sparkle EdDSA public key (SUPublicEDKey)
#   SONAR_SPARKLE_PRIVATE_KEY EdDSA private key (base64 string) or
#   SONAR_SPARKLE_PRIVATE_KEY_FILE  path to a file holding it
#   SONAR_SPARKLE_DOWNLOAD_URL_PREFIX  URL dir the DMG will be hosted at
#                             (default: directory of the feed URL)
#
# Required for a real build (not --dry-run / --skip-build), ADR-038:
#   SONAR_LICENSE_PUBLIC_KEY  base64 Ed25519 public key license keys are
#                             verified against, baked into the core at build
#                             time (`sonar license keygen --out <secret file>`
#                             prints it; keep the secret file private). Without
#                             it the core trusts the committed DEV key, whose
#                             secret is public: anyone could mint keys. Cargo
#                             rebuilds the core when this value changes.
# Optional:
#   SONAR_PURCHASE_URL        https store page for the "Buy" button on the
#                             expired-license gate (empty: button hidden).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=lib/env.sh
source "$ROOT/scripts/lib/env.sh"

ARCH="universal" VERSION="" BUILD="" DRY=0 SKIP_BUILD=0
usage() { sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; }
need_val() { [[ $# -ge 2 && -n "$2" ]] || { echo "release: $1 needs a value (see --help)" >&2; exit 2; }; }
while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch) need_val "$@"; ARCH="$2"; shift 2 ;;
    --version) need_val "$@"; VERSION="$2"; shift 2 ;;
    --build) need_val "$@"; BUILD="$2"; shift 2 ;;
    --dry-run) DRY=1; shift ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "release: unknown option '$1' (see --help)" >&2; exit 2 ;;
  esac
done
case "$ARCH" in
  universal) XC_ARCHS="arm64 x86_64" ;;
  arm64)     XC_ARCHS="arm64" ;;
  *) echo "release: --arch must be universal or arm64" >&2; exit 2 ;;
esac
[[ "$(uname -s)" == Darwin ]] || { echo "release: macOS only" >&2; exit 1; }
for tool in xcodegen xcodebuild codesign hdiutil lipo ditto; do
  command -v "$tool" >/dev/null || {
    echo "release: '$tool' not found. Install Xcode (App Store) + command line tools, and 'brew install xcodegen'." >&2
    exit 1
  }
done

say() { printf '\n▶ %s\n' "$*"; }
warn() { printf 'WARNING: %s\n' "$*" >&2; }

DEV_LICENSE_PUBLIC_KEY="ZUD3tqPTmjS5M6GHhTPlsBA20oooXz7cRHn6IIJHrQU"  # sonar-core license/keys.rs
if [[ "$SKIP_BUILD" == 1 && "$DRY" == 0 ]]; then
  warn "--skip-build: re-using dist/build as is; make sure it was built with the production SONAR_LICENSE_PUBLIC_KEY."
fi
if [[ "$SKIP_BUILD" == 0 ]]; then
  if [[ -n "${SONAR_LICENSE_PUBLIC_KEY:-}" ]]; then
    # Ed25519 public key = 32 bytes = 43 base64 chars (either alphabet, `=`
    # optional). A bad value would make every license check error (and the
    # app fail open).
    if [[ ! "$SONAR_LICENSE_PUBLIC_KEY" =~ ^[A-Za-z0-9_+/-]{43}=?$ ]]; then
      echo "release: SONAR_LICENSE_PUBLIC_KEY is not a 32-byte base64 Ed25519 key (use the public_key printed by 'sonar license keygen')." >&2
      exit 1
    fi
    if [[ "$SONAR_LICENSE_PUBLIC_KEY" == "$DEV_LICENSE_PUBLIC_KEY" && "$DRY" == 0 ]]; then
      echo "release: SONAR_LICENSE_PUBLIC_KEY is the DEV key; a release needs the production key." >&2
      exit 1
    fi
    export SONAR_LICENSE_PUBLIC_KEY   # picked up by build-core.sh -> cargo (option_env!)
  elif [[ "$DRY" == 1 ]]; then
    warn "SONAR_LICENSE_PUBLIC_KEY not set: building with the DEV license key (anyone can mint keys). Fine for a dry run, never ship it."
  else
    echo "release: SONAR_LICENSE_PUBLIC_KEY is not set. A release must be built with the production license public key (sonar license keygen --out <file>; see --help). Use --dry-run for a local build with the dev key." >&2
    exit 1
  fi
fi

yml() { sed -n "s/^ *$1: *\"\{0,1\}\([^\"]*\)\"\{0,1\} *\$/\1/p" "$ROOT/app/project.yml" | head -n1; }
VERSION="${VERSION:-$(yml MARKETING_VERSION)}"
BUILD="${BUILD:-$(yml CURRENT_PROJECT_VERSION)}"
[[ -n "$VERSION" && -n "$BUILD" ]] || { echo "release: cannot determine version/build" >&2; exit 1; }

DIST="$ROOT/dist"
WORK="$DIST/build"
DERIVED="$WORK/DerivedData"
APP="$WORK/Sonar.app"
DMG="$DIST/Sonar-$VERSION.dmg"
ENTITLEMENTS="$ROOT/app/Release.entitlements"
mkdir -p "$DIST" "$WORK"

SKIPPED=()

# ---- credential detection (all gracefully optional) ------------------------
IDENTITY="" NOTARY_ARGS=() FEED_URL="" PUB_KEY="" HAVE_SPARKLE_PRIV=0
if [[ "$DRY" == 0 ]]; then
  IDENTITY="${SONAR_SIGN_IDENTITY:-}"
  if [[ -n "${SONAR_NOTARY_PROFILE:-}" ]]; then
    NOTARY_ARGS=(--keychain-profile "$SONAR_NOTARY_PROFILE")
  elif [[ -n "${SONAR_NOTARY_KEY_ID:-}" && -n "${SONAR_NOTARY_ISSUER:-}" && -n "${SONAR_NOTARY_KEY_PATH:-}" ]]; then
    NOTARY_ARGS=(--key "$SONAR_NOTARY_KEY_PATH" --key-id "$SONAR_NOTARY_KEY_ID" --issuer "$SONAR_NOTARY_ISSUER")
  fi
  FEED_URL="${SONAR_SPARKLE_FEED_URL:-}"
  PUB_KEY="${SONAR_SPARKLE_PUBLIC_KEY:-}"
  [[ -n "${SONAR_SPARKLE_PRIVATE_KEY:-}" || -n "${SONAR_SPARKLE_PRIVATE_KEY_FILE:-}" ]] && HAVE_SPARKLE_PRIV=1
fi
if [[ -n "${IDENTITY}" ]] && [[ "$(security find-identity -v -p codesigning)" != *"$IDENTITY"* ]]; then
  echo "release: signing identity '$IDENTITY' not found in the keychain (security find-identity -v -p codesigning)." >&2
  exit 1
fi
if [[ -z "$IDENTITY" ]]; then
  warn "SONAR_SIGN_IDENTITY not set: ad-hoc signing. The DMG will NOT run on other Macs (Gatekeeper)."
  SKIPPED+=("Developer ID signing")
fi
if [[ ${#NOTARY_ARGS[@]} -eq 0 || -z "$IDENTITY" ]]; then
  warn "notarization skipped (needs SONAR_SIGN_IDENTITY plus SONAR_NOTARY_PROFILE or SONAR_NOTARY_KEY_ID/_ISSUER/_KEY_PATH)."
  NOTARY_ARGS=()
  SKIPPED+=("notarization")
fi
if [[ -z "$FEED_URL" || -z "$PUB_KEY" ]]; then
  warn "SONAR_SPARKLE_FEED_URL / SONAR_SPARKLE_PUBLIC_KEY not set: the app is built with updates DISABLED."
  FEED_URL="" PUB_KEY=""
  SKIPPED+=("Sparkle updates in app")
fi

# ---- 1. build --------------------------------------------------------------
if [[ "$SKIP_BUILD" == 0 ]]; then
  say "Rust core ($ARCH, release)"
  SONAR_CORE_PROFILE=release SONAR_CORE_ARCHS="$ARCH" "$ROOT/scripts/build-core.sh"
  say "xcodegen"
  (cd "$ROOT/app" && xcodegen generate --quiet)
  say "xcodebuild Release ($XC_ARCHS)"
  rm -rf "$APP"
  # Built ad-hoc by Xcode; re-signed inside-out below (Xcode's own signing of
  # nested Sparkle code is not Developer ID / hardened-runtime clean).
  xcodebuild -project "$ROOT/app/Sonar.xcodeproj" -scheme Sonar -configuration Release \
    -destination 'generic/platform=macOS' -derivedDataPath "$DERIVED" \
    ARCHS="$XC_ARCHS" ONLY_ACTIVE_ARCH=NO \
    MARKETING_VERSION="$VERSION" CURRENT_PROJECT_VERSION="$BUILD" \
    SONAR_SPARKLE_FEED_URL="$FEED_URL" SONAR_SPARKLE_PUBLIC_KEY="$PUB_KEY" \
    SONAR_PURCHASE_URL="${SONAR_PURCHASE_URL:-}" \
    CODE_SIGN_IDENTITY="-" CODE_SIGNING_ALLOWED=YES \
    -quiet build
  ditto "$DERIVED/Build/Products/Release/Sonar.app" "$APP"
fi
[[ -d "$APP" ]] || { echo "release: $APP missing (run without --skip-build)" >&2; exit 1; }

# ---- 2. codesign, inside-out ----------------------------------------------
sign_id="${IDENTITY:--}"
TS=(--timestamp)
[[ -z "$IDENTITY" ]] && TS=(--timestamp=none)
sign() { codesign --force --sign "$sign_id" --options runtime "${TS[@]}" "$@"; }

say "codesign ($([[ -n "$IDENTITY" ]] && echo "$IDENTITY" || echo ad-hoc), hardened runtime)"
# Sparkle 2 layout (https://sparkle-project.org/documentation/sandboxing/):
# XPC services, Autoupdate, Updater.app, then the framework, then the app.
# Never --deep for signing. Sparkle helpers keep their own entitlements.
while IFS= read -r fw; do
  v="$fw/Versions/B"
  [[ -d "$v" ]] || v="$fw/Versions/Current"
  for x in "$v"/XPCServices/*.xpc; do
    [[ -e "$x" ]] && sign --preserve-metadata=entitlements "$x"
  done
  [[ -e "$v/Autoupdate" ]] && sign "$v/Autoupdate"
  [[ -e "$v/Updater.app" ]] && sign "$v/Updater.app"
  sign "$fw"
done < <(find "$APP/Contents/Frameworks" -maxdepth 1 -name '*.framework' 2>/dev/null)
# Any other loose nested Mach-O dylibs.
while IFS= read -r d; do sign "$d"; done < <(find "$APP/Contents" -name '*.dylib' -not -path '*.framework/*' 2>/dev/null)
sign --entitlements "$ENTITLEMENTS" "$APP"
codesign --verify --deep --strict --verbose=1 "$APP"

# ---- 3. notarize helper ----------------------------------------------------
notarize() { # <file>; fails unless Apple reports status "Accepted"
  local out id
  out="$(xcrun notarytool submit "$1" "${NOTARY_ARGS[@]}" --wait --output-format json)" || true
  echo "$out"
  if [[ ! "$out" =~ \"status\"[[:space:]]*:[[:space:]]*\"Accepted\" ]]; then
    id="$(sed -n 's/.*"id"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' <<<"$out" | head -n1)"
    echo "release: notarization of $1 was not accepted." >&2
    [[ -n "$id" ]] && { xcrun notarytool log "$id" "${NOTARY_ARGS[@]}" || true; }
    exit 1
  fi
}
if [[ ${#NOTARY_ARGS[@]} -gt 0 ]]; then
  say "notarize app"
  ZIP="$WORK/Sonar-notarize.zip"
  rm -f "$ZIP"; ditto -c -k --keepParent "$APP" "$ZIP"
  notarize "$ZIP"
  xcrun stapler staple "$APP"
fi

# ---- 4. DMG ----------------------------------------------------------------
say "DMG $DMG"
STAGE="$WORK/dmg-root"
rm -rf "$STAGE" "$DMG"; mkdir -p "$STAGE"
ditto "$APP" "$STAGE/Sonar.app"
ln -s /Applications "$STAGE/Applications"
hdiutil create -quiet -volname "Sonar $VERSION" -srcfolder "$STAGE" -fs HFS+ -format UDZO -ov "$DMG"
[[ -n "$IDENTITY" ]] && codesign --force --sign "$IDENTITY" --timestamp "$DMG"
if [[ ${#NOTARY_ARGS[@]} -gt 0 ]]; then
  say "notarize DMG"
  notarize "$DMG"
  xcrun stapler staple "$DMG"
fi

# ---- 5. verify -------------------------------------------------------------
say "verify"
BIN="$APP/Contents/MacOS/Sonar"
LIPO_INFO="$(lipo -info "$BIN")"; echo "$LIPO_INFO"
if [[ "$ARCH" == universal ]]; then
  [[ "$LIPO_INFO" == *arm64* && "$LIPO_INFO" == *x86_64* ]] \
    || { echo "release: binary is not universal" >&2; exit 1; }
fi
codesign --verify --deep --strict "$APP" && echo "codesign --verify --deep --strict: ok"
codesign -dv "$APP" 2>&1 | grep -E 'flags|Signature|Authority|TeamIdentifier' || true
hdiutil verify -quiet "$DMG" && echo "hdiutil verify: ok"
MNT="$WORK/mnt"; mkdir -p "$MNT"
hdiutil detach -quiet "$MNT" 2>/dev/null || true  # stale mount from a killed run
hdiutil attach -readonly -nobrowse -quiet -mountpoint "$MNT" "$DMG"
trap 'hdiutil detach -quiet "$MNT" 2>/dev/null || true' EXIT
[[ -d "$MNT/Sonar.app" && -L "$MNT/Applications" ]] || { echo "release: DMG content wrong" >&2; exit 1; }
echo "dmg mount: ok (Sonar.app + Applications link)"
hdiutil detach -quiet "$MNT"; trap - EXIT
if [[ ${#NOTARY_ARGS[@]} -gt 0 ]]; then
  xcrun stapler validate "$DMG"
  spctl --assess --type open --context context:primary-signature -v "$DMG"
fi

# ---- 6. Sparkle appcast ----------------------------------------------------
if [[ -n "$FEED_URL" && "$HAVE_SPARKLE_PRIV" == 1 && ${#NOTARY_ARGS[@]} -gt 0 ]]; then
  say "Sparkle appcast"
  BIN_DIR="$(find "$DERIVED/SourcePackages/artifacts" -type d -path '*Sparkle/bin' 2>/dev/null | head -n1)"
  GEN="$BIN_DIR/generate_appcast"
  [[ -x "$GEN" ]] || { echo "release: Sparkle generate_appcast not found under $DERIVED (build first)." >&2; exit 1; }
  UPD="$DIST/updates"; mkdir -p "$UPD"
  cp "$DMG" "$UPD/"
  PREFIX="${SONAR_SPARKLE_DOWNLOAD_URL_PREFIX:-${FEED_URL%/*}}/"
  if [[ -n "${SONAR_SPARKLE_PRIVATE_KEY_FILE:-}" ]]; then
    "$GEN" --ed-key-file "$SONAR_SPARKLE_PRIVATE_KEY_FILE" --download-url-prefix "$PREFIX" "$UPD"
  else
    printf '%s' "$SONAR_SPARKLE_PRIVATE_KEY" | "$GEN" --ed-key-file - --download-url-prefix "$PREFIX" "$UPD"
  fi
  echo "appcast: dist/updates/appcast.xml"
else
  warn "appcast skipped (needs a notarized DMG plus SONAR_SPARKLE_FEED_URL, SONAR_SPARKLE_PUBLIC_KEY and SONAR_SPARKLE_PRIVATE_KEY[_FILE])."
  SKIPPED+=("Sparkle appcast")
fi

# ---- summary ---------------------------------------------------------------
say "done: ${DMG#"$ROOT/"} ($ARCH, version $VERSION build $BUILD)"
if [[ ${#SKIPPED[@]} -gt 0 ]]; then
  why="no credentials"; [[ "$DRY" == 1 ]] && why="dry run"
  printf 'Skipped (%s): %s\n' "$why" "$(IFS=,; echo "${SKIPPED[*]}")"
  echo "This DMG is for local testing only. See ADR-037 / T7.4 for what to provide."
else
  echo "Next: upload dist/Sonar-$VERSION.dmg and dist/updates/appcast.xml to your host (manual)."
fi
