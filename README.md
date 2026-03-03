# FinalCall

FinalCall is a Tauri + React desktop app (Rust backend + SQLite) that helps employees maintain healthy sign-off boundaries.

## What the app does

- Daily check-in (`Check In Now` or manual time)
- Computes out-time from `check-in + target duration` (default `8h 30m`)
- Sends pre-out reminder (`5 | 10 | 20` minutes)
- Sends out-time reminders with `Stop` and `Snooze 10m`
- Persists settings and history in local SQLite

## Why this matters

Overwork can lead to burnout, lower concentration, poor sleep, and declining long-term productivity. A consistent stop-time routine improves recovery, sustained output, and work-life balance. FinalCall enforces that routine with daily boundaries.

## Stack

- Frontend: React + TypeScript + Vite
- Desktop shell: Tauri v2
- Backend: Rust
- Database: SQLite (`rusqlite`)
- Linux notifications: `notify-rust`

## Quick start (development)

1. Install dependencies

```bash
bun install
```

2. Start desktop dev app

```bash
bun run tauri dev
```

3. Optional: enable tray in Linux dev

By default tray is off in Linux debug mode to avoid GTK tray warnings.

```bash
FINALCALL_ENABLE_TRAY_DEV=1 bun run tauri dev
```

4. Frontend-only dev server

```bash
bun run dev
```

5. Type-check + web build

```bash
bun run build
```

6. Rust compile check

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

## Build commands

### Linux (`.deb` and `.rpm`)

```bash
bun run bundle:linux
```

Output:
- `src-tauri/target/release/bundle/deb/`
- `src-tauri/target/release/bundle/rpm/`

### Arch package (`.pkg.tar.zst`)

Requires Arch + `base-devel`.

```bash
bun run bundle:arch
```

### Snap (`.snap`)

Requires `snapcraft` installed.

```bash
bun run bundle:snap
```

### Flatpak (`.flatpak`)

Requires `flatpak` and `flatpak-builder`.

```bash
bun run bundle:flatpak
```

Output:
- `dist/finalcall.flatpak`

### Windows installer builds (`.exe` and `.msi`)

Must run on Windows (or GitHub Actions windows runner).

```bash
bun run bundle:windows:exe
bun run bundle:windows:msi
```

CI workflow:
- `.github/workflows/windows-bundles.yml`

## Important notes

- AppImage is intentionally disabled.
- Data is local-only (no cloud sync in v1).
- Auto-start on Linux uses `~/.config/autostart/com.krish.finalcall.desktop`.

## Project layout

- `src/` -> React UI
- `src-tauri/src/lib.rs` -> Rust commands, scheduler, DB, startup behavior
- `packaging/arch/` -> Arch assets
- `packaging/flatpak/` -> Flatpak manifest
- `snapcraft.yaml` -> Snap config

## Previews
- Mini windows slap
  <img width="493" height="446" alt="image" src="https://github.com/user-attachments/assets/d300352d-60a9-4318-977a-604ec5e99aab" />

- Full window slap
  <img width="1920" height="1080" alt="image" src="https://github.com/user-attachments/assets/233f5c00-f178-4d59-bafd-42420e362b6d" />


