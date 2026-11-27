# GameShrink

**Smart, high-ratio game compression for Windows—built with Rust, Tauri v2, React, and SQLite.**

[Platform](https://www.google.com/search?q=https://github.com/tammam2/GameShrink/releases)
[Built With](https://www.google.com/search?q=https://github.com/tammam2/GameShrink)
[Latest Release](https://www.google.com/search?q=https://github.com/tammam2/GameShrink/releases)

GameShrink reclaims gigabytes of SSD space across your PC game library without breaking your workflow. Choose between **instant native Windows compression** (100% anti-cheat safe, zero wait time) or **Ultra Pack (Auto-Launch)**—our custom high-ratio codec with an automatic proxy `.exe` launcher and **Instant Re-Seal** when you close the game.

## Compression Modes

| Mode | How It Works | Launch Behavior | Anti-Cheat Safe | Best For |
| :--- | :--- | :--- | :--- | :--- |
| **Ultra Pack (Auto-Launch)** | Custom Smart Delta + Texture Planar + Auto-Transform + Zstd-19 | Double-click `game.exe` to auto-unpack, play, and **instantly re-seal** on exit | Single-player / offline games | Maximum possible storage savings |
| **Playable Max** | Strongest native Windows NTFS LZX + smart entropy guard | Instant direct `.exe` launch (zero wait) | Yes | Everyday games (recommended) |
| **Playable Extreme** | Forces native Windows LZX on every file regardless of type | Instant direct `.exe` launch (zero wait) | Yes | Squeezing out every last MB while staying instant-play |
| **Playable Balanced** | LZX on static/startup files + XPRESS8K on streaming map packs | Instant direct `.exe` launch (fastest streaming) | Yes | Massive open-world titles |

## How Ultra Pack (Auto-Launch + Instant Re-Seal) Works

1. **Deep Compression:** GameShrink clusters near-duplicate assets inside your game (Smart Delta), pre-conditions DDS/BCn textures and structured binaries with automatically discovered invertible transforms, and compresses them at Zstd level 19 into a hidden `.gameshrink_vault` folder while saving each file's exact transform recipe in SQLite.
2. **Seamless Proxy `.exe`:** Your original `game.exe` is safely hidden as `game.exe.gs_orig` and replaced by a tiny native Rust launcher (`gs_shim.exe`).
3. **Engine-Safe Launch:** When you double-click `game.exe` (from Desktop, Explorer, or GameShrink), GameShrink unpacks the files in parallel, temporarily restores the original `.exe` filename so engines like Unity (`*_Data`) and Unreal run natively, and waits for the game to close.
4. **Instant Zero-Recompress Re-Seal:** The moment you exit the game, untouched files simply delete their temporary unpacked copies—because their already-compressed archives remain in `.gameshrink_vault`—shrinking the game back down in seconds. Only files modified during gameplay are re-compressed using their saved transform recipes.

## Key Features

- **Estimate Savings for Each Option:** Benchmarks real sample files from your selected game folder before compressing and displays estimated final sizes and GB saved for all 4 modes.
- **Safe Intra-Game Duplicate Merging:** Uses parallel BLAKE3 hashing strictly inside a single game folder to merge byte-identical static files into one shared NTFS copy (zero FPS cost, completely safe on uninstall, automatically excludes writable configs and saves).
- **Live Update Watcher & Re-Compress:** Monitors compressed games in the background to detect when Steam, Epic, or GOG patches add uncompressed files—with 1-click incremental **Re-Compress** or optional automatic background re-compression.
- **Optional Bloat Cleaner:** Spots leftover redistributable installers (`_CommonRedist`, DirectX, `vcredist`), crash dumps, and `.pdb` debug symbols.
- **Safe Cancel Button:** Stop any compression or restore operation cleanly at any time without corrupting files.
- **Persistent Before & After History:** Local SQLite database tracks exact physical drive space saved (`GetCompressedFileSizeW`), category breakdowns, file-by-file actions, and 1-click lossless **Restore Original**.
- **Anti-Cheat Shield:** Automatically detects Easy Anti-Cheat, BattlEye, Riot Vanguard, FACEIT, RICOCHET, and others, locking protected multiplayer titles to hash-safe native Windows modes.

## Download & Install

1. Open the [Releases page](https://www.google.com/search?q=https://github.com/tammam2/GameShrink/releases).
2. Download the latest **`GameShrink_x.x.x_x64-setup.exe`** (or `.msi`) installer.
3. Run the installer and launch **GameShrink**.

## Build from Source (Windows 10/11)

### Prerequisites

- [Rust](https://rustup.rs/) (1.77+) with Visual Studio C++ Build Tools (`Desktop development with C++`)
- [Node.js](https://nodejs.org/) (20+)
- Microsoft Edge WebView2 Runtime (pre-installed on Windows 10/11)

### Build Commands

```powershell
git clone https://github.com/tammam2/GameShrink.git
cd GameShrink
npm install

# 1. Build the native proxy launcher (gs_shim.exe)
New-Item -ItemType Directory -Force -Path src-tauri/resources
New-Item -ItemType File -Force -Path src-tauri/resources/gs_shim.exe
cargo build --release --bin gs_shim --manifest-path src-tauri/Cargo.toml
Copy-Item src-tauri/target/release/gs_shim.exe src-tauri/resources/gs_shim.exe -Force

# 2. Build the production Windows installers (.exe & .msi)
npm run tauri build
```

Compiled installers are output to:
- `src-tauri/target/release/bundle/nsis/`
- `src-tauri/target/release/bundle/msi/`

## Tech Stack

- **Backend & Compression Engine:** Rust (`rayon`, `blake3`, `zstd`, `rusqlite`, `walkdir`, `windows-sys`)
- **Desktop Framework:** Tauri v2 + standalone native `gs_shim.exe` proxy binary
- **Frontend:** React 18, TypeScript, Tailwind CSS, Lucide Icons
- **CI/CD:** GitHub Actions automated Windows release builds (`.github/workflows/release.yml`)
