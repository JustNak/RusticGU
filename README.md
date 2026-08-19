<p align="center">
  <picture>
    <source media="(prefers-color-scheme: light)" srcset="assets/brand/logo-light.png">
    <img src="assets/brand/logo.png" alt="RusticGU logo" width="128" height="128">
  </picture>
</p>

<h1 align="center">RusticGU</h1>

<p align="center">
  <strong>A Windows game-library compact tool.</strong><br>
  Transparent WOF <code>compact /EXE</code> so titles stay playable. Rust + GPUI, dark launcher-card UI, tray flyout.
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-green?style=for-the-badge" alt="MIT License"></a>
  <a href="https://github.com/JustNak/RusticGU/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/JustNak/RusticGU/ci.yml?branch=main&style=for-the-badge&label=CI" alt="CI status"></a>
  <img src="https://img.shields.io/badge/Platform-Windows-0078D6?style=for-the-badge&logo=windows&logoColor=white" alt="Windows">
  <img src="https://img.shields.io/badge/Rust-stable-orange?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
</p>

<p align="center">
  <a href="#download"><strong>Download</strong></a> ·
  <a href="#what-is-rusticgu"><strong>What it is</strong></a> ·
  <a href="#quick-start"><strong>Quick start</strong></a> ·
  <a href="#update-channels"><strong>Update channels</strong></a> ·
  <a href="#build-from-source"><strong>Build from source</strong></a> ·
  <a href="#license"><strong>License</strong></a>
</p>

---

## Download

Windows installers will appear on GitHub Releases **when a release is published**.

**[→ RusticGU releases](https://github.com/JustNak/RusticGU/releases)**

Nightly (unsigned, may be unstable) is an on-demand GitHub pre-release for testing new work before a Stable cut. In the app, set **Settings → General → Update channel** to **Nightly**, or grab a build from the [releases list](https://github.com/JustNak/RusticGU/releases).

| Asset | What it contains |
| --- | --- |
| **`RusticGU-windows-x64-setup.exe`** | **Recommended** — NSIS installer (Start Menu, uninstaller, optional app-data cleanup) |
| **`RusticGU-windows-x64.zip`** | Portable desktop app (`rusticgu.exe` + `rusticgu-updater.exe`) |

### Install (recommended)

1. Download **`RusticGU-windows-x64-setup.exe`** when a release is published.
2. Run the installer (per-user install; **no administrator rights** required unless an ACL on a game folder needs it).
3. Launch **RusticGU** from the Start Menu.

The installer places files under `%LOCALAPPDATA%\RusticGU\`, creates a Start Menu shortcut, and registers an uninstaller in Apps & Features.

Settings and window state live under `%APPDATA%\RusticGU\`. Uninstall via Apps & Features (optionally remove app data).

> **Note:** Builds are unsigned. SmartScreen may warn until code signing is added.

### Portable install (ZIP)

1. Download **`RusticGU-windows-x64.zip`** when a release is published.
2. Extract anywhere you like (for example `C:\Tools\RusticGU\`).
3. Run `rusticgu.exe`.

---

## What is RusticGU?

RusticGU is a **game-library compact tool** for Windows. It uses transparent WOF `compact /EXE` so titles stay playable. It is not LZNT1, not a rewriter, and not for WindowsApps. There is no savings-% promise.

- Dark launcher-card UI (Rust + [GPUI](https://gpui.rs/) + [gpui-component](https://github.com/longbridge/gpui-component))
- Steam library cards (logical vs on-disk size when cheap to read)
- Compact / uncompact a selected game folder (default **XPRESS8K**)
- Tray flyout: compact vs inflated summary, pause Live Compact, one-click recompact, open main window
- **Live Compact** (later) recompacts after patches with XPRESS8K; games stay playable
- **Shelf** (later) is the only LZX path, for cold games

**Requirements:** Windows 10 version 1607+, NTFS, launcher indexes.

**Skip:** saves, caches, already-compressed media. If `dstorage.dll` or `dstoragecore.dll` is in the tree, warn and skip unless the user overrides. Auto-exclude Guild Wars 2 and Secret World Legends.

### Not included (this repo / this first cut)

- Extra-store discovery (Epic, GOG, EA, Ubisoft, Riot, Battle.net, itch)
- `src/watch/**` (Live Compact file watching)
- Shelf implementation
- macOS / Linux
- MSIX / Microsoft Store

---

## Quick start

### From source (developers)

```bash
cargo run
```

Release build:

```bash
cargo build --release
```

Binary: `target/release/rusticgu.exe`.

### Commands

```text
rusticgu                 Open the launcher
rusticgu --help          Usage
```

---

## Update channels

Same model as [RusticDL](https://github.com/JustNak/RusticDL): GitHub Releases is the feed. Origin/Buildkite are not used.

| Channel | What the app follows | How it is published |
| --- | --- | --- |
| **Stable** | `/releases/latest` (non-prerelease) | Push a `vX.Y.Z` tag (not `v*-nightly.*`) |
| **Nightly** | Newest published `vX.Y.Z-nightly.*` pre-release | Actions → **Nightly** → **Run workflow** |

In the app: **Settings → General → Update channel**, then **Check for updates**. When a newer build is ready, **Update** hands off to **RusticGU Updater** (`rusticgu-updater.exe`).

To cut a **stable** release from a clean tree:

```bash
git tag v0.1.0
git push origin v0.1.0
```

To publish a **nightly**: Actions → **Nightly** → **Run workflow**.

---

## Build from source

### Requirements

| Tool | Notes |
| --- | --- |
| **Rust stable** | Install via [rustup](https://rustup.rs/) |
| **Windows C++ build tools** | Visual Studio Build Tools with the “Desktop development with C++” workload (needed by GPUI) |

### Layout

```text
.
├── src/
│   ├── main.rs
│   ├── app/              GPUI chrome, settings, toasts, tray flyout
│   ├── library/          Steam scan
│   ├── compact/          WOF compact /EXE engine
│   ├── settings.rs
│   ├── updater.rs
│   └── branding.rs
├── apps/updater/         Dedicated self-update helper
├── assets/brand/         Icons and logo
├── installer/nsis/       NSIS template
├── scripts/              Icon regen + NSIS packaging
└── .github/workflows/    CI, Nightly, Release
```

### Tests

```bash
cargo test
```

### Build the Windows installer (developers)

```powershell
cargo install cargo-packager --locked --version 0.11.8
powershell -ExecutionPolicy Bypass -File scripts/package-windows.ps1
```

Output: `dist-release/RusticGU-windows-x64-setup.exe`.

## Data location

- Windows: `%APPDATA%\RusticGU\`
  - `settings.json` — prefs (camelCase), including update channel
  - `state.json` — window placement and UI state

## Continuous integration

GitHub Actions workflows live in `.github/workflows/`:

| Workflow | When it runs | What it does |
| --- | --- | --- |
| **CI** (`ci.yml`) | Push / PR to `main` | `cargo fmt` check, `clippy`, `test` on Windows |
| **Release** (`release.yml`) | Tag `v*` except `v*-nightly.*` | Build Windows NSIS installer + zip; publish a **Stable** GitHub Release |
| **Nightly** (`nightly.yml`) | Manual **Run workflow** only | Same installer + zip as Release, stamped `X.Y.Z-nightly.YYYYMMDDHHMMSS`, published as a GitHub **pre-release** (`make_latest: false`). Skips when that commit already has a nightly. Keeps the last 14 nightlies. |

---

## Contributing / attribution

Issues and pull requests are welcome.

If you **fork, modify, redistribute, or ship** RusticGU (including commercial products), keep the MIT copyright notice and license text intact, and credit the original project:

- Project: **RusticGU**
- Author / maintainer: **[JustNak](https://github.com/JustNak)**
- Upstream: https://github.com/JustNak/RusticGU

That attribution requirement is part of the MIT license terms for this repository.

---

## License

RusticGU is released under the **[MIT License](LICENSE)**.

You may use, copy, modify, merge, publish, distribute, sublicense, and sell copies of the software — including for commercial purposes — provided you include the copyright notice and permission notice in all copies or substantial portions of the Software.
