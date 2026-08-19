# RusticGU

Game library compact launcher for Windows. Scan your Steam library, estimate WOF savings, and compact or undo selected installs with `compact /EXE` (default **XPRESS8K**). Never NTFS LZNT1.

Built with [gpui](https://www.gpui.rs/) `0.2.2` and [gpui-component](https://github.com/longbridge/gpui-component) `0.5.1`.

## Features

- Steam library scan: `HKCU\Software\Valve\Steam\SteamPath` → both `libraryfolders.vdf` files → `appmanifest_*.acf` → `steamapps\common\{installdir}`
- Game cards with logical vs on-disk size when cheap to read
- WOF CompactOS only (`compact /C /EXE:XPRESS8K` / `compact /U /EXE`)
- Skip video, audio, archives, logs/dumps, shader cache, and SaveGames
- Refuse ReFS, `WindowsApps` paths, and a running exe in-tree
- Warn (and block) when `dstorage.dll` is present unless Settings → General override is On
- Dry-run estimate before apply; progress + toast on done/fail
- Settings: General / System / Appearance (live draft, Save commits)
- Update channel picker: Stable (`/releases/latest`) or Nightly (published `vX.Y.Z-nightly.*`)
- Minimize-to-tray via `SW_HIDE` plus a tray flyout (summary, pause Live Compact stub, recompact, open window)

## Requirements

- Windows 10 version 1607 or later
- NTFS volume
- Steam installed (for library discovery)

## Build

```powershell
cargo build --release -p rusticgu -p rusticgu-updater
```

Package an NSIS installer:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/package-windows.ps1
```

## Data

`%APPDATA%\RusticGU\settings.json` and `state.json` (camelCase keys).

## License

MIT
