// Lightweight native Windows proxy launcher that replaces a game's .exe in Ultra Pack mode.
// When double-clicked, it wakes GameShrink to unpack, launch the hidden original .exe,
// wait for the game to close, and instantly re-seal without recompressing unchanged files.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Deserialize)]
struct ShimConfig {
    gameshrink_exe: String,
    game_id: String,
    orig_exe: String,
}

fn main() {
    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return,
    };
    let dir = match exe_path.parent() {
        Some(d) => d.to_path_buf(),
        None => return,
    };

    let cfg_path = dir.join(".gameshrink_shim.json");
    let raw = match fs::read_to_string(&cfg_path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let cfg: ShimConfig = match serde_json::from_str(&raw) {
        Ok(c) => c,
        Err(_) => return,
    };

    let passthrough_args: Vec<String> = std::env::args().skip(1).collect();
    let gs_path = PathBuf::from(&cfg.gameshrink_exe);

    if gs_path.exists() {
        let mut cmd = Command::new(&gs_path);
        cmd.arg("--play-ultra").arg(&cfg.game_id);
        if !passthrough_args.is_empty() {
            cmd.arg("--");
            cmd.args(&passthrough_args);
        }
        let _ = cmd.spawn();
    } else {
        // Fallback: if GameShrink was uninstalled, try launching original exe directly
        let orig = dir.join(&cfg.orig_exe);
        if orig.exists() {
            let _ = Command::new(&orig)
                .current_dir(&dir)
                .args(&passthrough_args)
                .spawn();
        }
    }
}
