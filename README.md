<p align="center">
  <picture>
    <source media="(prefers-color-scheme: light)" srcset="assets/brand/logo-light.png">
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/logo.png">
    <img src="assets/brand/logo.png" alt="RusticGU logo" width="128" height="128">
  </picture>
</p>

# RusticGU

RusticGU is a Windows game-library compact tool. It uses transparent WOF `compact /EXE` so titles stay playable — not NTFS LZNT1, not a rewriter, not WindowsApps, and with **no savings-percentage promise**.

Built in Rust with [gpui](https://www.gpui.rs/) `0.2.2` and [gpui-component](https://github.com/longbridge/gpui-component) `0.5.1`. Dark default, launcher-style game cards, tray flyout.

**Live Compact** (recompact after patches, XPRESS8K) and **Shelf** (the only LZX path, for cold games) are later work. This build does not implement them.

## What this build does

- Scan **Steam** via launcher indexes: `HKCU\Software\Valve\Steam\SteamPath` → both `libraryfolders.vdf` files → `appmanifest_*.acf` → `steamapps\common\{installdir}`
- Show those games as cards with logical vs on-disk size when cheap to read
- Compact / uncompact a selected folder with WOF only (`compact /C /EXE:<algo>` / `compact /U /EXE`)
- Default algorithm: **XPRESS8K**. XPRESS4K / XPRESS16K are selectable. **LZX is not** — reserved for Shelf
- Skip listed video / audio / image / archive / log-temp extensions (not `wav`, `dds`, or `bnk`); skip SaveGames, shader / pipeline / API caches, and logs / dumps
- Auto-exclude Guild Wars 2 and Secret World Legends (not offered for compact)
- If `dstorage.dll` or `dstoragecore.dll` is in the tree: warn and skip (do not compact) unless Settings → General override is On
- Refuse ReFS, a running exe in-tree, and WindowsApps paths
- Dry-run estimate before apply; progress + toast on done / fail
- Settings: General / System / Appearance (live draft; Save commits)
- GitHub updater: Stable (`/releases/latest`) or Nightly (`vX.Y.Z-nightly.*`), chosen in Settings → General
- Minimize-to-tray via `SW_HIDE`, plus a tray flyout

## What this build does not do

- Extra-store discovery (Epic / GOG / EA / Ubisoft / Riot / Battle.net / itch)
- File-system watch / patch detection (`src/watch`)
- Live Compact (tray Pause/Resume and “Recompact last patch” are stubs)
- Shelf (no LZX path in the live library)

## Requirements

- Windows 10 version 1607 or later
- NTFS volume
- Steam installed (this build’s only launcher index)

## Build

```powershell
cargo build --release -p rusticgu -p rusticgu-updater
```

Package an NSIS installer (`RusticGU-windows-x64-setup.exe`):

```powershell
powershell -ExecutionPolicy Bypass -File scripts/package-windows.ps1
```

Binaries: `rusticgu.exe`, `rusticgu-updater.exe`. App id `com.rusticgu.app`.

## Settings and data

`%APPDATA%\RusticGU\settings.json` and `state.json` (camelCase keys).

## License

MIT
