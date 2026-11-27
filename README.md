# AetherCrush — Next-Gen Game Compressor (Rust + Tauri v2)

A native Windows desktop app built with **Rust + Tauri v2 + React/Tailwind + SQLite** that crushes game storage using the strongest native NTFS **LZX** compression stacked with **intra-game hardlink deduplication**, while keeping every game **100% playable directly from its `.exe`**.

## Key Capabilities

- **Max Crush (Strongest Default):** Applies Windows NTFS WOF **LZX** compression + fast **3-window Shannon entropy guard** (skips already-Oodle/encrypted noise so you don't waste hours or CPU).
- **Brute Max (Force LZX All):** Forces LZX across every eligible file regardless of entropy.
- **Adaptive Thermal Mode:** Routes cold/static files to **LZX** and open-world streaming archives to **XPRESS8K**.
- **Intra-Game Hardlink Deduplication:** Uses parallel **BLAKE3** hashing strictly *inside a single game folder* to collapse identical static assets into 1 physical NTFS inode (0% FPS cost, safe on uninstall). Automatically excludes writable `.ini`/`.cfg`/`.json`/save files.
- **Anti-Cheat Shield:** Auto-detects EAC, BattlEye, Vanguard, FACEIT, Ricochet, etc., locking multiplayer games to 100% hash-safe NTFS LZX + Hardlinks.
- **Optional Single-Player Cold Vault (Zstd-19):** Archives unused foreign voice packs at Zstd level 19 with 1-click lossless restore.
- **Persistent SQLite Telemetry Ledger:** Tracks exact **Before vs. After** physical NTFS bytes (`GetCompressedFileSizeW`), category breakdown, per-file action log, and 1-click full restore.
- **Direct `.exe` Play:** No custom launcher required — double-click your game's `.exe` (or Steam/Epic) normally, or launch directly from the dashboard.

## Local Build & Publish (Windows 10/11)

### Prerequisites
1. [Rust](https://rustup.rs/) (1.77+) with MSVC build tools (`Desktop development with C++`)
2. [Node.js](https://nodejs.org/) (20+)
3. WebView2 Runtime (pre-installed on Windows 10/11)

### Run in Development
```bash
npm install
npm run tauri dev
```

### Build Ready-to-Publish Windows Installers (`.exe` NSIS + `.msi`)
```bash
npm install
npm run tauri build
```
Your production installers will be generated in:
- `src-tauri/target/release/bundle/nsis/AetherCrush_1.0.0_x64-setup.exe`
- `src-tauri/target/release/bundle/msi/AetherCrush_1.0.0_x64_en-US.msi`

### Automated GitHub Releases
Push a tag (`git tag v1.0.0 && git push origin v1.0.0`) and the included `.github/workflows/release.yml` automatically builds and attaches both Windows installers to a GitHub Release.
