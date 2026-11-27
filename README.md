# GameShrink — Smart Game Compressor (Rust + Tauri v2)

Shrink your Windows games to save gigabytes of SSD space while keeping them **playable directly from their `.exe`**—or deep-freeze games you aren't playing right now for the absolute smallest size.

## Compression Options

- **Playable Max (Recommended):** Strongest native Windows compression (LZX) + skips already-compressed files. Runs directly from `game.exe`.
- **Playable Extreme:** Forces maximum Windows compression on every single file to squeeze out every last MB. Still playable directly.
- **Playable Balanced:** Optimized for huge open-world games (high compression on static files, fast compression on streaming map packs).
- **Deep Freeze (Smallest Size):** Absolute maximum space savings for games you aren't playing right now (combines smart similarity compression, texture optimization, and level-19 archiving). Click **Unpack to Play** whenever you're ready to play again.

## Features

- **Estimate Savings for Each Option:** Tests real sample files from your game and shows estimated GB saved for all 4 modes before you commit.
- **Merge Duplicate Files (Safe):** Collapses identical files inside a game into one shared copy with zero FPS cost.
- **Safe Cancel Button:** Stop compression or restoration cleanly at any time without corrupting files.
- **Before & After History:** Local SQLite database tracks exact drive space saved per game, per category, and per file—plus 1-click Restore.
- **Anti-Cheat Aware:** Automatically detects EAC, BattlEye, Vanguard, etc., and protects multiplayer games.

## Build on Windows or GitHub Actions

```bash
npm install
npm run tauri build
```
Or open **Actions → Release GameShrink (Windows) → Run workflow** on GitHub to build `GameShrink_2.1.0_x64-setup.exe` in the cloud.
