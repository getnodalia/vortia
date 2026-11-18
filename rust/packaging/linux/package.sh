#!/usr/bin/env bash
# Package the Rust Vortia for Linux (R9 of docs/architecture/rust-only.md):
#
#   $DIST/Vortia-<version>-linux-<arch>.AppImage   any distro with glibc >= the build host's
#   $DIST/Vortia-<version>-linux-<arch>.tar.gz     bin/vortia-app, bin/vortia and share/
#
# The AppImage holds the desktop app and the `vortia` command-line tool. It does not carry
# OpenFOAM: the engine is Rust. Build on an old distro (CI: Ubuntu 22.04) so it runs on newer ones.
#
# Usage: rust/packaging/linux/package.sh [--skip-build] [--out DIR]
# Needs: cargo; appimagetool for the AppImage (downloaded into the target directory if missing).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUST="$(cd "$HERE/../.." && pwd)"
REPO="$(cd "$RUST/.." && pwd)"
APP_ID=dev.danielmiguelt.Vortia

SKIP_BUILD=0
DIST="${DIST:-$REPO/dist/rust}"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --out) DIST="$2"; shift 2 ;;
    -h | --help) sed -n '2,11p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
mkdir -p "$DIST"
DIST="$(cd "$DIST" && pwd)"

# The version lives in one place: [workspace.package] in rust/Cargo.toml.
VERSION="${VORTIA_VERSION:-$(awk '/^\[/ { in_pkg = ($0 == "[workspace.package]"); next } in_pkg && $1 == "version" { gsub(/"/, "", $3); print $3; exit }' "$RUST/Cargo.toml")}"
[[ -n "$VERSION" ]] || { echo "error: no version in $RUST/Cargo.toml" >&2; exit 1; }

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) ;;
  aarch64 | arm64) ARCH=aarch64 ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$RUST/target}"
BASENAME="Vortia-$VERSION-linux-$ARCH"
echo "==> Vortia $VERSION for Linux $ARCH"

if [[ "$SKIP_BUILD" = 0 ]]; then
  (cd "$RUST" && cargo build --release --locked -p vortia-app -p vortia-cli)
fi
BIN="$CARGO_TARGET_DIR/release"
for exe in vortia-app vortia; do
  [[ -x "$BIN/$exe" ]] || { echo "error: $BIN/$exe is not built" >&2; exit 1; }
done

# A new folder each run, so nothing of an earlier one is mixed in and nothing is deleted.
mkdir -p "$CARGO_TARGET_DIR"
WORK="$(mktemp -d "$CARGO_TARGET_DIR/linux-package.XXXXXX")"
STAGE="$WORK/root"
install -Dm755 "$BIN/vortia-app" "$STAGE/usr/bin/vortia-app"
install -Dm755 "$BIN/vortia" "$STAGE/usr/bin/vortia"
strip "$STAGE/usr/bin/vortia-app" "$STAGE/usr/bin/vortia" 2>/dev/null || true
install -Dm644 "$HERE/$APP_ID.desktop" "$STAGE/usr/share/applications/$APP_ID.desktop"
install -Dm644 "$REPO/resources/icons/vortia.png" "$STAGE/usr/share/icons/hicolor/512x512/apps/$APP_ID.png"
mkdir -p "$STAGE/usr/share/doc/vortia"
for f in README.md COPYING LICENSE NOTICE; do
  if [[ -f "$REPO/$f" ]]; then cp "$REPO/$f" "$STAGE/usr/share/doc/vortia/"; fi
done
if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$STAGE/usr/share/applications/$APP_ID.desktop"
fi

# ---- .tar.gz ------------------------------------------------------------------------------------
mkdir -p "$WORK/tar"
cp -R "$STAGE/usr" "$WORK/tar/$BASENAME"
tar -C "$WORK/tar" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
echo "wrote $DIST/$BASENAME.tar.gz"

# ---- AppImage -----------------------------------------------------------------------------------
APPDIR="$WORK/Vortia.AppDir"
cp -R "$STAGE" "$APPDIR"
cat >"$APPDIR/AppRun" <<'RUN'
#!/bin/sh
# Starts the app, or the command-line tool when the first argument is a subcommand's name.
HERE="$(dirname "$(readlink -f "$0")")"
export PATH="$HERE/usr/bin:$PATH"
exec "$HERE/usr/bin/vortia-app" "$@"
RUN
chmod +x "$APPDIR/AppRun"
cp "$HERE/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
cp "$REPO/resources/icons/vortia.png" "$APPDIR/$APP_ID.png"
ln -s "$APP_ID.png" "$APPDIR/.DirIcon"

TOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
if [[ -z "$TOOL" ]]; then
  TOOL="$CARGO_TARGET_DIR/appimagetool-$ARCH.AppImage"
  if [[ ! -x "$TOOL" ]]; then
    curl --proto '=https' -fsSL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"
    chmod +x "$TOOL"
  fi
fi
OUT="$DIST/$BASENAME.AppImage"
rm -f "$OUT"
# Extract-and-run works without FUSE (containers, CI); the output embeds the static runtime.
(cd "$DIST" && ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream "$APPDIR" "$OUT")
echo "wrote $OUT"

"$STAGE/usr/bin/vortia" --version 2>/dev/null || "$STAGE/usr/bin/vortia" version 2>/dev/null || true
echo "==> done"
ls -lh "$DIST"
