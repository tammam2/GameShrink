use crate::db::LedgerDb;
use crate::hypercodec;
use crate::models::{
    CompressRequest, CompressionMode, FileAuditRecord, GameRecord, ProgressEvent,
};
use crate::scanner::{self, HIGH_ENTROPY_THRESHOLD, MIN_COMPRESS_BYTES, VAULT_DIR_NAME};
use crate::win_fs::{self, WofAlgo};
use anyhow::{anyhow, Result};
use chrono::Utc;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

const HYPER_IN_MEMORY_LIMIT: u64 = 64 * 1024 * 1024;

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

fn friendly_hyper_label(raw_label: &str) -> String {
    if raw_label.contains("ChronoDelta") {
        "Deep Freeze (Smart Delta)".to_string()
    } else if raw_label.contains("CanonPack") {
        "Deep Freeze (Texture)".to_string()
    } else if raw_label.contains("AutoSIMD") {
        "Deep Freeze (Smart Pack)".to_string()
    } else {
        "Deep Freeze".to_string()
    }
}

fn vault_file_hyper(
    src: &Path,
    vault_dest: &Path,
    reference_path: Option<&Path>,
) -> Result<(u64, String)> {
    let meta_len = fs::metadata(src).map(|m| m.len()).unwrap_or(0);
    if meta_len <= HYPER_IN_MEMORY_LIMIT {
        let res = hypercodec::encode_hyper_file(src, vault_dest, reference_path, 19)?;
        fs::remove_file(src)?;
        Ok((res.compressed_size, friendly_hyper_label(&res.codec_label)))
    } else {
        if let Some(parent) = vault_dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let input = BufReader::new(File::open(src)?);
        let output = BufWriter::new(File::create(vault_dest)?);
        zstd::stream::copy_encode(input, output, 19)?;
        let vaulted_size = win_fs::get_disk_size(vault_dest);
        fs::remove_file(src)?;
        Ok((vaulted_size, "Deep Freeze".to_string()))
    }
}

fn restore_vaulted_file(
    vault_src: &Path,
    original_dest: &Path,
    reference_path: Option<&Path>,
) -> Result<u64> {
    if !vault_src.exists() {
        return Err(anyhow!("Archive file missing: {}", vault_src.display()));
    }
    let sz = hypercodec::decode_hyper_file(vault_src, original_dest, reference_path)?;
    let _ = fs::remove_file(vault_src);
    Ok(sz)
}

pub fn run_compression(
    app: &AppHandle,
    db: &LedgerDb,
    cancel: &Arc<AtomicBool>,
    req: CompressRequest,
) -> Result<GameRecord> {
    let start = Instant::now();
    let root = PathBuf::from(&req.path);
    if !root.exists() || !root.is_dir() {
        return Err(anyhow!("Selected folder does not exist: {}", req.path));
    }

    emit_progress(app, "Scanning Game Folder", "Reading file list...", 0, 100, 0);

    let files = scanner::collect_game_files(&root)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(anyhow!("Compression cancelled by user."));
    }

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

    let duplicates = if req.enable_hardlink_dedup {
        emit_progress(
            app,
            "Finding Duplicate Files",
            "Checking for identical files inside this game...",
            0,
            total_files,
            0,
        );
        scanner::find_intra_game_duplicates(&files)
    } else {
        HashMap::new()
    };

    if cancel.load(Ordering::Relaxed) {
        return Err(anyhow!("Compression cancelled by user."));
    }

    let vault_root = root.join(VAULT_DIR_NAME);
    let processed = AtomicU64::new(0);
    let saved_running = AtomicU64::new(0);

    let audits: Vec<FileAuditRecord> = if req.mode == CompressionMode::HyperArchive {
        emit_progress(
            app,
            "Finding Similar Assets",
            "Grouping similar files for extra compression...",
            0,
            total_files,
            0,
        );

        let chrono_input: Vec<(String, PathBuf, u64)> = files
            .iter()
            .filter(|f| {
                !duplicates.contains_key(&f.rel_path)
                    && f.logical_bytes >= MIN_COMPRESS_BYTES
                    && f.logical_bytes <= HYPER_IN_MEMORY_LIMIT
            })
            .map(|f| (f.rel_path.clone(), f.abs_path.clone(), f.logical_bytes))
            .collect();
        let chrono_pairs = hypercodec::find_chrono_delta_pairs(&chrono_input);

        let mut phase_a_audits: Vec<FileAuditRecord> = files
            .par_iter()
            .filter(|f| chrono_pairs.contains_key(&f.rel_path))
            .map(|f| {
                if cancel.load(Ordering::Relaxed) {
                    return FileAuditRecord {
                        id: 0,
                        game_id: game_id.clone(),
                        rel_path: f.rel_path.clone(),
                        category: f.category.clone(),
                        before_bytes: f.disk_bytes,
                        after_bytes: f.disk_bytes,
                        saved_bytes: 0,
                        action: "Cancelled".to_string(),
                        entropy: (f.entropy * 100.0).round() / 100.0,
                        blake3_hash: None,
                        hardlink_target: None,
                        vault_path: None,
                    };
                }

                let ref_rel = chrono_pairs.get(&f.rel_path).unwrap();
                let ref_abs = root.join(ref_rel);
                let rel_vault = format!("{}.gsh", f.rel_path);
                let dest = vault_root.join(&rel_vault);

                let (after, action, vp, ht) =
                    match vault_file_hyper(&f.abs_path, &dest, Some(&ref_abs)) {
                        Ok((sz, label)) => (sz, label, Some(rel_vault), Some(ref_rel.clone())),
                        Err(_) => (f.disk_bytes, "Skipped".to_string(), None, None),
                    };

                let saved = f.disk_bytes.saturating_sub(after);
                let done = processed.fetch_add(1, Ordering::Relaxed) + 1;
                let total_saved_now = saved_running.fetch_add(saved, Ordering::Relaxed) + saved;
                if done == total_files || done % 15 == 0 {
                    emit_progress(
                        app,
                        "Deep Freezing Similar Files",
                        &f.rel_path,
                        done,
                        total_files,
                        total_saved_now,
                    );
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
                    blake3_hash: None,
                    hardlink_target: ht,
                    vault_path: vp,
                }
            })
            .collect();

        let phase_b_audits: Vec<FileAuditRecord> = files
            .par_iter()
            .filter(|f| !chrono_pairs.contains_key(&f.rel_path))
            .map(|f| {
                if cancel.load(Ordering::Relaxed) {
                    return FileAuditRecord {
                        id: 0,
                        game_id: game_id.clone(),
                        rel_path: f.rel_path.clone(),
                        category: f.category.clone(),
                        before_bytes: f.disk_bytes,
                        after_bytes: f.disk_bytes,
                        saved_bytes: 0,
                        action: "Cancelled".to_string(),
                        entropy: (f.entropy * 100.0).round() / 100.0,
                        blake3_hash: None,
                        hardlink_target: None,
                        vault_path: None,
                    };
                }

                let (after, action, blake3_hash, hardlink_target, vault_path) =
                    if let Some((canonical_rel, hash)) = duplicates.get(&f.rel_path) {
                        let _ = fs::remove_file(&f.abs_path);
                        (
                            0,
                            "Duplicate Merged".to_string(),
                            Some(hash.clone()),
                            Some(canonical_rel.clone()),
                            None,
                        )
                    } else if f.logical_bytes < MIN_COMPRESS_BYTES {
                        (f.disk_bytes, "Skipped (Tiny)".to_string(), None, None, None)
                    } else {
                        let rel_vault = format!("{}.gsh", f.rel_path);
                        let dest = vault_root.join(&rel_vault);
                        match vault_file_hyper(&f.abs_path, &dest, None) {
                            Ok((sz, label)) => (sz, label, None, None, Some(rel_vault)),
                            Err(_) => (f.disk_bytes, "Skipped".to_string(), None, None, None),
                        }
                    };

                let saved = f.disk_bytes.saturating_sub(after);
                let done = processed.fetch_add(1, Ordering::Relaxed) + 1;
                let total_saved_now = saved_running.fetch_add(saved, Ordering::Relaxed) + saved;
                if done == total_files || done % 15 == 0 {
                    emit_progress(
                        app,
                        "Deep Freezing Game Files",
                        &f.rel_path,
                        done,
                        total_files,
                        total_saved_now,
                    );
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

        phase_a_audits.extend(phase_b_audits);
        phase_a_audits
    } else {
        let allow_cold_vault = req.enable_cold_vault && !anti_cheat_detected;

        files
            .par_iter()
            .map(|f| {
                if cancel.load(Ordering::Relaxed) {
                    return FileAuditRecord {
                        id: 0,
                        game_id: game_id.clone(),
                        rel_path: f.rel_path.clone(),
                        category: f.category.clone(),
                        before_bytes: f.disk_bytes,
                        after_bytes: f.disk_bytes,
                        saved_bytes: 0,
                        action: "Cancelled".to_string(),
                        entropy: (f.entropy * 100.0).round() / 100.0,
                        blake3_hash: None,
                        hardlink_target: None,
                        vault_path: None,
                    };
                }

                let (after, action, blake3_hash, hardlink_target, vault_path) =
                    if let Some((canonical_rel, hash)) = duplicates.get(&f.rel_path) {
                        let canonical_abs = root.join(canonical_rel);
                        if win_fs::replace_with_hardlink(&f.abs_path, &canonical_abs).is_ok() {
                            (
                                0,
                                "Duplicate Merged".to_string(),
                                Some(hash.clone()),
                                Some(canonical_rel.clone()),
                                None,
                            )
                        } else {
                            (f.disk_bytes, "Skipped".to_string(), None, None, None)
                        }
                    } else if allow_cold_vault && f.is_cold_vault_candidate {
                        let rel_vault = format!("{}.gsh", f.rel_path);
                        let dest = vault_root.join(&rel_vault);
                        match vault_file_hyper(&f.abs_path, &dest, None) {
                            Ok((vsize, _)) => (
                                vsize,
                                "Archived Language".to_string(),
                                None,
                                None,
                                Some(rel_vault),
                            ),
                            Err(_) => (f.disk_bytes, "Skipped".to_string(), None, None, None),
                        }
                    } else if f.logical_bytes < MIN_COMPRESS_BYTES {
                        (f.disk_bytes, "Skipped (Tiny)".to_string(), None, None, None)
                    } else if req.mode != CompressionMode::BruteMax
                        && f.entropy > HIGH_ENTROPY_THRESHOLD
                    {
                        (
                            f.disk_bytes,
                            "Already Compressed".to_string(),
                            None,
                            None,
                            None,
                        )
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
                            CompressionMode::HyperArchive => WofAlgo::Lzx,
                        };

                        let friendly = if algo == WofAlgo::Lzx {
                            "Max Compressed"
                        } else {
                            "Fast Compressed"
                        };

                        match win_fs::compress_file_wof(&f.abs_path, algo) {
                            Ok(new_disk_size) => (
                                new_disk_size.min(f.disk_bytes),
                                friendly.to_string(),
                                None,
                                None,
                                None,
                            ),
                            Err(_) => (f.disk_bytes, "Skipped".to_string(), None, None, None),
                        }
                    };

                let saved = f.disk_bytes.saturating_sub(after);
                let done = processed.fetch_add(1, Ordering::Relaxed) + 1;
                let total_saved_now = saved_running.fetch_add(saved, Ordering::Relaxed) + saved;

                if done == total_files || done % 25 == 0 {
                    let phase = match action.as_str() {
                        "Duplicate Merged" => "Merging Duplicate Files",
                        "Archived Language" => "Archiving Unused Voice Packs",
                        _ => "Compressing Game Files",
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
            .collect()
    };

    let was_cancelled = cancel.load(Ordering::Relaxed);

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
        if a.action == "Duplicate Merged" {
            files_hardlinked += 1;
            dedup_saved_bytes += a.saved_bytes;
        } else if a.action == "Max Compressed" || a.action == "Fast Compressed" {
            files_compressed += 1;
            lzx_saved_bytes += a.saved_bytes;
        } else if a.vault_path.is_some() {
            files_vaulted += 1;
            vault_saved_bytes += a.saved_bytes;
        } else {
            files_skipped += 1;
        }
    }

    let saved_bytes = before_bytes.saturating_sub(after_bytes);
    let ratio_pct = if before_bytes > 0 {
        (saved_bytes as f64 / before_bytes as f64) * 100.0
    } else {
        0.0
    };

    let status = if was_cancelled {
        "cancelled".to_string()
    } else if req.mode == CompressionMode::HyperArchive {
        "archived".to_string()
    } else {
        "compressed".to_string()
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
        status,
    };

    db.upsert_game(&record, &audits)?;
    if was_cancelled {
        emit_progress(
            app,
            "Cancelled",
            "Stopped safely — already-processed files saved to history",
            processed.load(Ordering::Relaxed),
            total_files,
            saved_bytes,
        );
    } else {
        emit_progress(
            app,
            "Complete",
            "Saved compression history",
            total_files,
            total_files,
            saved_bytes,
        );
    }

    Ok(record)
}

pub fn restore_game(
    app: &AppHandle,
    db: &LedgerDb,
    cancel: &Arc<AtomicBool>,
    game_id: &str,
    break_hardlinks: bool,
) -> Result<GameRecord> {
    let start = Instant::now();
    let detail = db.get_game_details(game_id)?;
    let all_audits = db.get_all_audits(game_id)?;
    let root = PathBuf::from(&detail.game.path);
    let vault_root = if root.join(VAULT_DIR_NAME).exists() {
        root.join(VAULT_DIR_NAME)
    } else {
        root.join(".aethercrush_vault")
    };

    let total = all_audits.len() as u64;
    let processed = AtomicU64::new(0);

    let phase1_audits: Vec<FileAuditRecord> = all_audits
        .par_iter()
        .filter(|a| {
            !(a.vault_path.is_some() && a.hardlink_target.is_some())
                && a.action != "Duplicate Merged"
                && a.action != "Hardlinked"
        })
        .map(|a| {
            if cancel.load(Ordering::Relaxed) {
                return a.clone();
            }
            let abs = root.join(&a.rel_path);
            let mut restored_size = a.before_bytes;

            if let Some(vp) = &a.vault_path {
                let vault_file = vault_root.join(vp);
                if let Ok(sz) = restore_vaulted_file(&vault_file, &abs, None) {
                    restored_size = sz;
                }
            } else if a.action == "Max Compressed"
                || a.action == "Fast Compressed"
                || a.action == "LZX"
                || a.action == "XPRESS8K"
            {
                if abs.exists() {
                    if let Ok(sz) = win_fs::decompress_file_wof(&abs) {
                        restored_size = sz;
                    }
                }
            } else if abs.exists() {
                restored_size = win_fs::get_disk_size(&abs);
            }

            let done = processed.fetch_add(1, Ordering::Relaxed) + 1;
            if done == total || done % 25 == 0 {
                emit_progress(app, "Restoring Files", &a.rel_path, done, total, 0);
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

    let phase2_audits: Vec<FileAuditRecord> = all_audits
        .par_iter()
        .filter(|a| {
            (a.vault_path.is_some() && a.hardlink_target.is_some())
                || a.action == "Duplicate Merged"
                || a.action == "Hardlinked"
        })
        .map(|a| {
            if cancel.load(Ordering::Relaxed) {
                return a.clone();
            }
            let abs = root.join(&a.rel_path);
            let mut restored_size = a.before_bytes;

            if let (Some(vp), Some(ref_rel)) = (&a.vault_path, &a.hardlink_target) {
                let vault_file = vault_root.join(vp);
                let ref_abs = root.join(ref_rel);
                if let Ok(sz) = restore_vaulted_file(&vault_file, &abs, Some(&ref_abs)) {
                    restored_size = sz;
                }
            } else if a.action == "Duplicate Merged" || a.action == "Hardlinked" {
                if !abs.exists() {
                    if let Some(canon_rel) = &a.hardlink_target {
                        let canon_abs = root.join(canon_rel);
                        if canon_abs.exists() {
                            if let Some(parent) = abs.parent() {
                                let _ = fs::create_dir_all(parent);
                            }
                            let _ = fs::copy(&canon_abs, &abs);
                            restored_size = win_fs::get_disk_size(&abs);
                        }
                    }
                } else if break_hardlinks {
                    let _ = win_fs::break_hardlink(&abs);
                    restored_size = win_fs::get_disk_size(&abs);
                }
            }

            let done = processed.fetch_add(1, Ordering::Relaxed) + 1;
            if done == total || done % 25 == 0 {
                emit_progress(app, "Restoring Merged Files", &a.rel_path, done, total, 0);
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

    let mut updated_audits = phase1_audits;
    updated_audits.extend(phase2_audits);

    let was_cancelled = cancel.load(Ordering::Relaxed);
    if !was_cancelled && vault_root.exists() {
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
    updated_game.status = if was_cancelled {
        "cancelled".to_string()
    } else {
        "restored".to_string()
    };

    db.upsert_game(&updated_game, &updated_audits)?;
    emit_progress(
        app,
        if was_cancelled { "Cancelled" } else { "Restored" },
        if was_cancelled {
            "Restore stopped by user"
        } else {
            "All files restored cleanly"
        },
        total,
        total,
        0,
    );

    Ok(updated_game)
}
