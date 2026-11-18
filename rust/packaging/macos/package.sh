#!/usr/bin/env bash
# Package the Rust Vortia for macOS (R9 of docs/architecture/rust-only.md):
#
#   $DIST/Vortia-<version>-macos-<arch>.dmg   Vortia.app in the designed installer window
#
# The bundle holds the desktop app and the `vortia` command-line tool next to it. It does not
# carry OpenFOAM: the engine is Rust. The window (background, icon size and positions) is the one
# of the existing installer, packaging/macos/create_dmg.sh and dmg-background.tiff.
#
# The result is signed ad hoc only: Apple silicon will not start unsigned code, but nothing is
# notarized (no Apple Developer ID), so the first launch goes through right click > Open.
#
# Usage: rust/packaging/macos/package.sh [--skip-build] [--out DIR]
# Needs: macOS with cargo, sips, iconutil, hdiutil and codesign.
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS packaging requires macOS" >&2
  exit 1
fi

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUST="$(cd "$HERE/../.." && pwd)"
REPO="$(cd "$RUST/.." && pwd)"

SKIP_BUILD=0
DIST="${DIST:-$REPO/dist/rust}"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --out) DIST="$2"; shift 2 ;;
    -h | --help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
mkdir -p "$DIST"
DIST="$(cd "$DIST" && pwd)"

# The version lives in one place: [workspace.package] in rust/Cargo.toml.
VERSION="${VORTIA_VERSION:-$(awk '/^\[/ { in_pkg = ($0 == "[workspace.package]"); next } in_pkg && $1 == "version" { gsub(/"/, "", $3); print $3; exit }' "$RUST/Cargo.toml")}"
[[ -n "$VERSION" ]] || { echo "error: no version in $RUST/Cargo.toml" >&2; exit 1; }
SHORT_VERSION="${VERSION%%-*}"

ARCH="$(uname -m)"
case "$ARCH" in
  arm64 | aarch64) ARCH=arm64 ;;
  x86_64) ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac

# Keep in sync with LSMinimumSystemVersion in Info.plist.in.
export MACOSX_DEPLOYMENT_TARGET=11.0
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$RUST/target}"
echo "==> Vortia $VERSION for macOS $ARCH"

if [[ "$SKIP_BUILD" = 0 ]]; then
  (cd "$RUST" && cargo build --release --locked -p vortia-app -p vortia-cli)
fi
BIN="$CARGO_TARGET_DIR/release"
for exe in vortia-app vortia; do
  [[ -x "$BIN/$exe" ]] || { echo "error: $BIN/$exe is missing" >&2; exit 1; }
done

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
APP="$WORK/Vortia.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN/vortia-app" "$BIN/vortia" "$APP/Contents/MacOS/"
chmod +x "$APP/Contents/MacOS/vortia-app" "$APP/Contents/MacOS/vortia"
sed -e "s/@VERSION@/$SHORT_VERSION/g" "$HERE/Info.plist.in" >"$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"
printf 'APPL????' >"$APP/Contents/PkgInfo"
cp "$REPO/LICENSE" "$APP/Contents/Resources/LICENSE"

# The icon: macOS applies its own mask, so it gets the full-bleed artwork.
ICONSET="$WORK/Vortia.iconset"
mkdir -p "$ICONSET"
ART="$REPO/resources/icons/vortia-macos.png"
[[ -f "$ART" ]] || { echo "error: $ART is missing" >&2; exit 1; }
for size in 16 32 128 256 512; do
  retina=$((size * 2))
  sips -s format png -z "$size" "$size" "$ART" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  sips -s format png -z "$retina" "$retina" "$ART" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Vortia.icns"

# Ad hoc signature, inside out: the tool first, then the bundle.
codesign --force --sign - --timestamp=none "$APP/Contents/MacOS/vortia"
codesign --force --sign - --timestamp=none "$APP"
codesign --verify --strict --verbose=2 "$APP"

DMG="$DIST/Vortia-$VERSION-macos-$ARCH.dmg"
"$REPO/packaging/macos/create_dmg.sh" "$APP" "$DMG" "$VERSION"
"$APP/Contents/MacOS/vortia" help >/dev/null
echo "==> done"
ls -lh "$DMG"
