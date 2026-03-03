#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$ROOT_DIR/dist/arch"
PKG_DIR="$ROOT_DIR/packaging/arch"
VERSION="0.1.0"
STAGE_DIR="$OUT_DIR/finalcall-$VERSION"

if ! command -v makepkg >/dev/null 2>&1; then
  echo "makepkg not found. Install base-devel on Arch Linux first." >&2
  exit 1
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

cp "$PKG_DIR/PKGBUILD" "$OUT_DIR/PKGBUILD"
cp "$PKG_DIR/finalcall.desktop" "$OUT_DIR/finalcall.desktop"

# Create source tarball expected by PKGBUILD with top-level folder finalcall-$VERSION/.
mkdir -p "$STAGE_DIR"
rsync -a \
  --exclude='.git' \
  --exclude='node_modules' \
  --exclude='dist' \
  --exclude='src-tauri/target' \
  "$ROOT_DIR/" "$STAGE_DIR/"

tar -czf "$OUT_DIR/finalcall-${VERSION}.tar.gz" -C "$OUT_DIR" "finalcall-$VERSION"
rm -rf "$STAGE_DIR"

(
  cd "$OUT_DIR"
  makepkg -fs --noconfirm
)

echo "Arch package artifacts generated in: $OUT_DIR"
