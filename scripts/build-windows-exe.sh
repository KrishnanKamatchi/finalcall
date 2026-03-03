#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != MINGW* && "$(uname -s)" != CYGWIN* && "$(uname -s)" != MSYS* ]]; then
  echo "Windows EXE bundling must run on Windows. Use GitHub Actions workflow: .github/workflows/windows-bundles.yml" >&2
  exit 1
fi

bun run tauri build --target x86_64-pc-windows-msvc --bundles nsis
