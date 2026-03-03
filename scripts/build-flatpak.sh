#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="$ROOT_DIR/packaging/flatpak/com.krish.finalcall.yaml"
BUILD_DIR="$ROOT_DIR/.flatpak-build"
REPO_DIR="$ROOT_DIR/dist/flatpak-repo"
BUNDLE_PATH="$ROOT_DIR/dist/finalcall.flatpak"
APP_ID="com.krish.finalcall"

if ! command -v flatpak-builder >/dev/null 2>&1; then
  echo "flatpak-builder not found. Install flatpak-builder first." >&2
  exit 1
fi

mkdir -p "$REPO_DIR" "$ROOT_DIR/dist"

flatpak-builder --force-clean --repo="$REPO_DIR" "$BUILD_DIR" "$MANIFEST"
flatpak build-bundle "$REPO_DIR" "$BUNDLE_PATH" "$APP_ID"

echo "Flatpak bundle generated: $BUNDLE_PATH"
