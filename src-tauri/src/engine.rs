use crate::db::LedgerDb;
use crate::models::{
    CompressRequest, CompressionMode, FileAuditRecord, GameRecord, ProgressEvent,
};
use crate::scanner::{
    self, HIGH_ENTROPY_THRESHOLD, MIN_COMPRESS_BYTES, VAULT_DIR_NAME,
};
use crate::win_fs::{self, WofAlgo};
use anyhow::{anyhow, Result};
use chrono::Utc;
use rayon::prelude::*;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

fn emit_progress(
    app: &AppHandle,
    phase: &str,
    current_file: &str,
    processed: u64,
    total: u64,
    saved_so_far: u64,
) {
    let pct = if total > 0 {
        ((processed as f64 / total as f64) * 100.0).min(100.0)
    } else {
        100.0
    };
    let _ = app.emit(
        "compression-progress",
        ProgressEvent {
            phase: phase.to_string(),
            current_file: current_file.to_string(),
            processed_files: processed,
            total_files: total,
            saved_so_far_bytes: saved_so_far,
            pct,
        },
    );
}

fn vault_file_zstd19(src: &Path, vault_dest: &Path) -> Result<u64> {
    if let Some(parent) = vault_dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let input = BufReader::new(File::open(src)?);
    let output = BufWriter::new(File::create(vault_dest)?);
    zstd::stream::copy_encode(input, output, 19)?;
    let vaulted_size = win_fs::get_disk_size(vault_dest);
    fs::remove_file(src)?;
    Ok(vaulted_size)
}

fn restore_vaulted_file(vault_src: &Path, original_dest: &Path) -> Result<u64> {
    if !vault_src.exists() {
        return Err(anyhow!("Vault file missing: {}", vault_src.display()));
    }
    if let Some(parent) = original_dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let input = BufReader::new(File::open(vault_src)?);
    let output = BufWriter::new(File::create(original_dest)?);
    zstd::stream::copy_decode(input, output)?;
    let _ = fs::remove_file(vault_src);
    Ok(win_fs::get_disk_size(original_dest))
}

pub fn run_compression(
    app: &AppHandle,
    db: &LedgerDb,
    req: CompressRequest,
) -> Result<GameRecord> {
    let start = Instant::now();
    let root = PathBuf::from(&req.path);
    if !root.exists() || !root.is_dir() {
        return Err(anyhow!("Selected folder does not exist: {}", req.path));
    }

    emit_progress(app, "Scanning & Entropy X-Ray", "Analyzing game files...", 0, 100, 0);

    let files = scanner::collect_game_files(&root)?;
    let total_files = files.len() as u64;
    let (anti_cheat_detected, anti_cheat_name) = scanner::detect_anti_cheat(&files);
    let exe_path = scanner::detect_main_exe(&root, &files);

    let game_id = db
        .find_game_id_by_path(&req.path)?
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let game_name = req
        .name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| {
            root.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Game")
                .to_string()
        });

    let before_bytes: u64 = files.iter().map(|f| f.disk_bytes).sum();

    // Pass 1: Find intra-game duplicates strictly inside this single game folder
    let duplicates = if req.enable_hardlink_dedup {
        emit_progress(
            app,
            "Intra-Game Hardlink Deduplication",
            "Hashing duplicate candidates with BLAKE3...",
            0,
            total_files,
            0,
        );
        scanner::find_intra_game_duplicates(&files)
    } else {
        std::collections::HashMap::new()
    };

    let processed = AtomicU64::new(0);
    let saved_running = AtomicU64::new(0);

    // Cold Vault is automatically locked OFF when anti-cheat is detected for safety
    let allow_cold_vault = req.enable_cold_vault && !anti_cheat_detected;
    let vault_root = root.join(VAULT_DIR_NAME);

    // Process files in parallel via Rayon (hardlinks first on duplicates, then WOF LZX/XPRESS8K or Vault)
    let audits: Vec<FileAuditRecord> = files
        .par_iter()
        .map(|f| {
            let mut action = "Unchanged".to_string();
            let mut after = f.disk_bytes;
            let mut blake3_hash: Option<String> = None;
            let mut hardlink_target: Option<String> = None;
            let mut vault_path: Option<String> = None;

            if let Some((canonical_rel, hash)) = duplicates.get(&f.rel_path) {
                let canonical_abs = root.join(canonical_rel);
                if win_fs::replace_with_hardlink(&f.abs_path, &canonical_abs).is_ok() {
                    action = "Hardlinked".to_string();
                    after = 0; // Physical storage reclaimed because inode is shared
                    blake3_hash = Some(hash.clone());
                    hardlink_target = Some(canonical_rel.clone());
                } else {
                    action = "Skipped-Error".to_string();
                }
            } else if allow_cold_vault && f.is_cold_vault_candidate {
                let rel_zst = format!("{}.zst", f.rel_path);
                let dest = vault_root.join(&rel_zst);
                if let Ok(vsize) = vault_file_zstd19(&f.abs_path, &dest) {
                    action = "Vaulted-Zstd19".to_string();
                    after = vsize;
                    vault_path = Some(rel_zst);
                } else {
                    action = "Skipped-Error".to_string();
                }
            } else if f.logical_bytes < MIN_COMPRESS_BYTES {
                action = "Skipped-TooSmall".to_string();
            } else if req.mode != CompressionMode::BruteMax && f.entropy > HIGH_ENTROPY_THRESHOLD {
                action = "Skipped-HighEntropy".to_string();
            } else {
                let algo = match req.mode {
                    CompressionMode::MaxCrush | CompressionMode::BruteMax => WofAlgo::Lzx,
                    CompressionMode::AdaptiveThermal => {
                        if f.is_streaming_archive {
                            WofAlgo::Xpress8k
                        } else {
                            WofAlgo::Lzx
                        }
                    }
                };

                match win_fs::compress_file_wof(&f.abs_path, algo) {
                    Ok(new_disk_size) => {
                        after = new_disk_size.min(f.disk_bytes);
                        action = algo.label().to_string();
                    }
                    Err(_) => {
                        action = "Skipped-Error".to_string();
                    }
                }
            }

            let saved = f.disk_bytes.saturating_sub(after);
            let done = processed.fetch_add(1, Ordering::Relaxed) + 1;
            let total_saved_now = saved_running.fetch_add(saved, Ordering::Relaxed) + saved;

            if done == total_files || done % 25 == 0 {
                let phase = match action.as_str() {
                    "Hardlinked" => "Intra-Game Hardlink Dedup",
                    "Vaulted-Zstd19" => "Cold Vaulting (Zstd-19)",
                    _ => "Compressing (NTFS LZX Engine)",
                };
                emit_progress(app, phase, &f.rel_path, done, total_files, total_saved_now);
            }

            FileAuditRecord {
                id: 0,
                game_id: game_id.clone(),
                rel_path: f.rel_path.clone(),
                category: f.category.clone(),
                before_bytes: f.disk_bytes,
                after_bytes: after,
                saved_bytes: saved,
                action,
                entropy: (f.entropy * 100.0).round() / 100.0,
                blake3_hash,
                hardlink_target,
                vault_path,
            }
        })
        .collect();

    let mut after_bytes = 0u64;
    let mut dedup_saved_bytes = 0u64;
    let mut lzx_saved_bytes = 0u64;
    let mut vault_saved_bytes = 0u64;
    let mut files_compressed = 0u64;
    let mut files_hardlinked = 0u64;
    let mut files_skipped = 0u64;
    let mut files_vaulted = 0u64;

    for a in &audits {
        after_bytes += a.after_bytes;
        match a.action.as_str() {
            "Hardlinked" => {
                files_hardlinked += 1;
                dedup_saved_bytes += a.saved_bytes;
            }
            "LZX" | "XPRESS8K" => {
                files_compressed += 1;
                lzx_saved_bytes += a.saved_bytes;
            }
            "Vaulted-Zstd19" => {
                files_vaulted += 1;
                vault_saved_bytes += a.saved_bytes;
            }
            _ => {
                files_skipped += 1;
            }
        }
    }

    let saved_bytes = before_bytes.saturating_sub(after_bytes);
    let ratio_pct = if before_bytes > 0 {
        (saved_bytes as f64 / before_bytes as f64) * 100.0
    } else {
        0.0
    };

    let record = GameRecord {
        id: game_id,
        name: game_name,
        path: req.path,
        exe_path,
        anti_cheat_detected,
        anti_cheat_name,
        mode: req.mode.as_str().to_string(),
        before_bytes,
        after_bytes,
        saved_bytes,
        ratio_pct,
        dedup_saved_bytes,
        lzx_saved_bytes,
        vault_saved_bytes,
        files_total: total_files,
        files_compressed,
        files_hardlinked,
        files_skipped,
        files_vaulted,
        duration_secs: start.elapsed().as_secs_f64(),
        last_compressed_at: Utc::now().to_rfc3339(),
        status: "compressed".to_string(),
    };

    db.upsert_game(&record, &audits)?;
    emit_progress(app, "Complete", "Ledger saved to SQLite", total_files, total_files, saved_bytes);

    Ok(record)
}

pub fn restore_game(
    app: &AppHandle,
    db: &LedgerDb,
    game_id: &str,
    break_hardlinks: bool,
) -> Result<GameRecord> {
    let start = Instant::now();
    let detail = db.get_game_details(game_id)?;
    let all_audits = db.get_all_audits(game_id)?;
    let root = PathBuf::from(&detail.game.path);
    let vault_root = root.join(VAULT_DIR_NAME);

    let total = all_audits.len() as u64;
    let processed = AtomicU64::new(0);

    let updated_audits: Vec<FileAuditRecord> = all_audits
        .par_iter()
        .map(|a| {
            let abs = root.join(&a.rel_path);
            let mut restored_size = a.before_bytes;

            match a.action.as_str() {
                "Vaulted-Zstd19" => {
                    if let Some(vp) = &a.vault_path {
                        let vault_file = vault_root.join(vp);
                        if let Ok(sz) = restore_vaulted_file(&vault_file, &abs) {
                            restored_size = sz;
                        }
                    }
                }
                "Hardlinked" => {
                    if break_hardlinks && abs.exists() {
                        let _ = win_fs::break_hardlink(&abs);
                        restored_size = win_fs::get_disk_size(&abs);
                    }
                }
                "LZX" | "XPRESS8K" => {
                    if abs.exists() {
                        if let Ok(sz) = win_fs::decompress_file_wof(&abs) {
                            restored_size = sz;
                        }
                    }
                }
                _ => {
                    if abs.exists() {
                        restored_size = win_fs::get_disk_size(&abs);
                    }
                }
            }

            let done = processed.fetch_add(1, Ordering::Relaxed) + 1;
            if done == total || done % 25 == 0 {
                emit_progress(app, "Restoring Original Files", &a.rel_path, done, total, 0);
            }

            FileAuditRecord {
                id: 0,
                game_id: a.game_id.clone(),
                rel_path: a.rel_path.clone(),
                category: a.category.clone(),
                before_bytes: restored_size,
                after_bytes: restored_size,
                saved_bytes: 0,
                action: "Restored".to_string(),
                entropy: a.entropy,
                blake3_hash: a.blake3_hash.clone(),
                hardlink_target: None,
                vault_path: None,
            }
        })
        .collect();

    // Clean empty vault folder if present
    if vault_root.exists() {
        let _ = fs::remove_dir_all(&vault_root);
    }

    let restored_total: u64 = updated_audits.iter().map(|a| a.after_bytes).sum();
    let mut updated_game = detail.game;
    updated_game.before_bytes = restored_total;
    updated_game.after_bytes = restored_total;
    updated_game.saved_bytes = 0;
    updated_game.ratio_pct = 0.0;
    updated_game.dedup_saved_bytes = 0;
    updated_game.lzx_saved_bytes = 0;
    updated_game.vault_saved_bytes = 0;
    updated_game.files_compressed = 0;
    updated_game.files_hardlinked = 0;
    updated_game.files_vaulted = 0;
    updated_game.duration_secs = start.elapsed().as_secs_f64();
    updated_game.last_compressed_at = Utc::now().to_rfc3339();
    updated_game.status = "restored".to_string();

    db.upsert_game(&updated_game, &updated_audits)?;
    emit_progress(app, "Restored", "All files restored losslessly", total, total, 0);

    Ok(updated_game)
}
