#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if ! command -v snapcraft >/dev/null 2>&1; then
  echo "snapcraft not found. Install snapcraft first." >&2
  exit 1
fi

(
  cd "$ROOT_DIR"
  snapcraft --destructive-mode --verbosity=brief
)

echo "Snap build complete. Check *.snap in project root."
