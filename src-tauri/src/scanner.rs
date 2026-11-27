use crate::models::{CategoryBreakdown, ScanPreview};
use crate::win_fs;
use anyhow::{anyhow, Result};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const VAULT_DIR_NAME: &str = ".aethercrush_vault";
pub const HIGH_ENTROPY_THRESHOLD: f64 = 7.82;
pub const MIN_COMPRESS_BYTES: u64 = 8 * 1024; // Skip tiny <8KB files (NTFS cluster floor)

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub abs_path: PathBuf,
    pub rel_path: String,
    pub category: String,
    pub logical_bytes: u64,
    pub disk_bytes: u64,
    pub entropy: f64,
    pub is_writable_config: bool,
    pub is_streaming_archive: bool,
    pub is_cold_vault_candidate: bool,
}

/// Fast 3-window Shannon entropy sampler (start, middle, end — 16 KB each).
pub fn sample_entropy(path: &Path, file_size: u64) -> f64 {
    if file_size == 0 {
        return 0.0;
    }
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return 8.0,
    };

    const WINDOW: usize = 16 * 1024;
    let mut counts = [0u64; 256];
    let mut total = 0u64;
    let mut buf = vec![0u8; WINDOW];

    let offsets: Vec<u64> = if file_size <= (WINDOW as u64) * 3 {
        vec![0]
    } else {
        vec![
            0,
            (file_size / 2).saturating_sub((WINDOW / 2) as u64),
            file_size.saturating_sub(WINDOW as u64),
        ]
    };

    for off in offsets {
        if file.seek(SeekFrom::Start(off)).is_ok() {
            if let Ok(n) = file.read(&mut buf) {
                for &b in &buf[..n] {
                    counts[b as usize] += 1;
                }
                total += n as u64;
            }
        }
    }

    if total == 0 {
        return 0.0;
    }

    let mut entropy = 0.0f64;
    for &c in &counts {
        if c > 0 {
            let p = c as f64 / total as f64;
            entropy -= p * p.log2();
        }
    }
    entropy
}

pub fn hash_file_blake3(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

pub fn classify_category(rel_path: &str) -> String {
    let lower = rel_path.to_lowercase();
    let ext = Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match ext {
        "pak" | "utoc" | "ucas" | "vpk" | "ba2" | "bsa" | "rpf" | "arc" | "forge" | "assets"
        | "resource" | "bundle" | "dds" | "ktx" | "ktx2" | "tga" | "png" | "bmp" | "hdr" => {
            "Textures & Archives".to_string()
        }
        "wav" | "ogg" | "wem" | "bnk" | "pck" | "fsb" | "mp3" | "flac" | "xwb" | "xsb" => {
            "Audio & Voice".to_string()
        }
        "bik" | "bk2" | "mp4" | "webm" | "mkv" | "usm" | "mov" | "avi" => {
            "Video & Cutscenes".to_string()
        }
        "exe" | "dll" | "so" | "sys" | "pdb" | "lib" => "Binaries & DLLs".to_string(),
        "json" | "xml" | "ini" | "cfg" | "yaml" | "yml" | "csv" | "txt" | "lua" | "py" | "js"
        | "locres" => "Data & Config".to_string(),
        _ => "Other Assets".to_string(),
    }
}

pub fn is_writable_config(rel_path: &str) -> bool {
    let lower = rel_path.to_lowercase();
    let ext = Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    matches!(
        ext,
        "ini" | "cfg" | "json" | "xml" | "sav" | "log" | "db" | "sqlite" | "txt" | "vdf" | "toml"
    )
}

pub fn is_streaming_archive(rel_path: &str) -> bool {
    let lower = rel_path.to_lowercase();
    let ext = Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    matches!(
        ext,
        "pak" | "ucas" | "vpk" | "ba2" | "bsa" | "rpf" | "forge" | "assets" | "bundle"
    )
}

/// Identifies non-English foreign voice/localization packs eligible for optional single-player Zstd-19 Cold Vaulting.
pub fn is_cold_vault_candidate(rel_path: &str, size: u64) -> bool {
    if size < 5 * 1024 * 1024 {
        return false;
    }
    let lower = rel_path.to_lowercase();
    let is_media_or_pack = lower.ends_with(".bnk")
        || lower.ends_with(".pck")
        || lower.ends_with(".wem")
        || lower.ends_with(".fsb")
        || lower.ends_with(".bik")
        || lower.ends_with(".bk2")
        || lower.ends_with(".usm")
        || lower.ends_with(".pak");

    if !is_media_or_pack {
        return false;
    }

    let foreign_markers = [
        "french", "german", "spanish", "russian", "japanese", "chinese", "korean", "italian",
        "polish", "portuguese", "brazilian", "_fr_", "_de_", "_es_", "_ru_", "_jp_", "_ja_",
        "_zh_", "_ko_", "_it_", "_pl_", "_pt_", "(fr)", "(de)", "(es)", "(ru)", "(jp)",
    ];
    foreign_markers.iter().any(|m| lower.contains(m))
}

pub fn detect_anti_cheat(files: &[ScannedFile]) -> (bool, Option<String>) {
    let signatures = [
        ("easyanticheat", "Easy Anti-Cheat (EAC)"),
        ("battleye", "BattlEye"),
        ("beclient", "BattlEye"),
        ("vanguard", "Riot Vanguard"),
        ("vgk.sys", "Riot Vanguard"),
        ("faceit", "FACEIT Anti-Cheat"),
        ("gameguard", "nProtect GameGuard"),
        ("xigncode", "XIGNCODE3"),
        ("ricochet", "RICOCHET Anti-Cheat"),
        ("punkbuster", "PunkBuster"),
    ];

    for f in files {
        let lower = f.rel_path.to_lowercase();
        for (needle, label) in &signatures {
            if lower.contains(needle) {
                return (true, Some((*label).to_string()));
            }
        }
    }
    (false, None)
}

pub fn detect_main_exe(root: &Path, files: &[ScannedFile]) -> Option<String> {
    let ignore_words = [
        "unins",
        "crash",
        "redist",
        "vcredist",
        "dxsetup",
        "dotnet",
        "setup",
        "helper",
        "updater",
        "launcher_helper",
        "easyanticheat",
        "battleye",
        "unitycrashhandler",
        "ue4prereq",
        "ueprereq",
    ];

    let mut candidates: Vec<&ScannedFile> = files
        .iter()
        .filter(|f| {
            let lower = f.rel_path.to_lowercase();
            lower.ends_with(".exe") && !ignore_words.iter().any(|w| lower.contains(w))
        })
        .collect();

    // Prefer shallow depth, then largest executable size
    candidates.sort_by(|a, b| {
        let depth_a = a.rel_path.matches('/').count() + a.rel_path.matches('\\').count();
        let depth_b = b.rel_path.matches('/').count() + b.rel_path.matches('\\').count();
        depth_a
            .cmp(&depth_b)
            .then_with(|| b.logical_bytes.cmp(&a.logical_bytes))
    });

    candidates
        .first()
        .map(|f| root.join(&f.rel_path).to_string_lossy().to_string())
}

pub fn collect_game_files(root: &Path) -> Result<Vec<ScannedFile>> {
    if !root.exists() || !root.is_dir() {
        return Err(anyhow!("Folder does not exist: {}", root.display()));
    }

    let raw_paths: Vec<PathBuf> = WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            e.file_name()
                .to_str()
                .map(|n| n != VAULT_DIR_NAME)
                .unwrap_or(true)
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();

    let scanned: Vec<ScannedFile> = raw_paths
        .par_iter()
        .filter_map(|abs| {
            let meta = std::fs::metadata(abs).ok()?;
            let logical_bytes = meta.len();
            let disk_bytes = win_fs::get_disk_size(abs);
            let rel_path = abs
                .strip_prefix(root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            let entropy = if logical_bytes >= MIN_COMPRESS_BYTES {
                sample_entropy(abs, logical_bytes)
            } else {
                0.0
            };

            Some(ScannedFile {
                abs_path: abs.clone(),
                rel_path: rel_path.clone(),
                category: classify_category(&rel_path),
                logical_bytes,
                disk_bytes,
                entropy,
                is_writable_config: is_writable_config(&rel_path),
                is_streaming_archive: is_streaming_archive(&rel_path),
                is_cold_vault_candidate: is_cold_vault_candidate(&rel_path, logical_bytes),
            })
        })
        .collect();

    Ok(scanned)
}

/// Finds exact byte-identical static files strictly INSIDE this single game folder.
/// Returns a map of `duplicate_rel_path -> (canonical_rel_path, blake3_hash)`.
pub fn find_intra_game_duplicates(files: &[ScannedFile]) -> HashMap<String, (String, String)> {
    let mut by_size: HashMap<u64, Vec<&ScannedFile>> = HashMap::new();
    for f in files {
        if f.logical_bytes >= MIN_COMPRESS_BYTES && !f.is_writable_config {
            by_size.entry(f.logical_bytes).or_default().push(f);
        }
    }

    let collision_files: Vec<&ScannedFile> = by_size
        .into_values()
        .filter(|group| group.len() > 1)
        .flatten()
        .collect();

    let hashed: Vec<(&ScannedFile, String)> = collision_files
        .par_iter()
        .filter_map(|f| hash_file_blake3(&f.abs_path).ok().map(|h| (*f, h)))
        .collect();

    let mut canonical_by_hash: HashMap<String, String> = HashMap::new();
    let mut duplicates: HashMap<String, (String, String)> = HashMap::new();

    for (f, h) in hashed {
        if let Some(canonical_rel) = canonical_by_hash.get(&h) {
            if canonical_rel != &f.rel_path {
                duplicates.insert(f.rel_path.clone(), (canonical_rel.clone(), h));
            }
        } else {
            canonical_by_hash.insert(h, f.rel_path.clone());
        }
    }

    duplicates
}

pub fn scan_game_preview(path_str: &str) -> Result<ScanPreview> {
    let root = PathBuf::from(path_str);
    let files = collect_game_files(&root)?;
    let (anti_cheat_detected, anti_cheat_name) = detect_anti_cheat(&files);
    let detected_exe = detect_main_exe(&root, &files);
    let duplicates = find_intra_game_duplicates(&files);

    let suggested_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Game")
        .to_string();

    let mut total_logical_bytes = 0u64;
    let mut current_disk_bytes = 0u64;
    let mut duplicate_reclaimable_bytes = 0u64;
    let mut compressible_bytes = 0u64;
    let mut high_entropy_bytes = 0u64;
    let mut cold_vault_candidates = 0u64;
    let mut cold_vault_bytes = 0u64;
    let mut estimated_saved_bytes = 0u64;

    let mut cat_map: HashMap<String, CategoryBreakdown> = HashMap::new();

    for f in &files {
        total_logical_bytes += f.logical_bytes;
        current_disk_bytes += f.disk_bytes;

        let is_dup = duplicates.contains_key(&f.rel_path);
        let mut est_file_saved = 0u64;

        if is_dup {
            duplicate_reclaimable_bytes += f.disk_bytes;
            est_file_saved = f.disk_bytes;
        } else if f.entropy > HIGH_ENTROPY_THRESHOLD {
            high_entropy_bytes += f.disk_bytes;
            est_file_saved = 0;
        } else if f.logical_bytes >= MIN_COMPRESS_BYTES {
            compressible_bytes += f.disk_bytes;
            // Conservative LZX gain estimate based on Shannon entropy
            let est_ratio = ((8.0 - f.entropy) / 8.0).clamp(0.08, 0.72);
            est_file_saved = (f.disk_bytes as f64 * est_ratio) as u64;
        }

        if f.is_cold_vault_candidate {
            cold_vault_candidates += 1;
            cold_vault_bytes += f.disk_bytes;
        }

        estimated_saved_bytes += est_file_saved;

        let entry = cat_map
            .entry(f.category.clone())
            .or_insert_with(|| CategoryBreakdown {
                category: f.category.clone(),
                file_count: 0,
                before_bytes: 0,
                after_bytes: 0,
                saved_bytes: 0,
            });
        entry.file_count += 1;
        entry.before_bytes += f.disk_bytes;
        entry.saved_bytes += est_file_saved;
        entry.after_bytes += f.disk_bytes.saturating_sub(est_file_saved);
    }

    let mut categories: Vec<CategoryBreakdown> = cat_map.into_values().collect();
    categories.sort_by(|a, b| b.before_bytes.cmp(&a.before_bytes));

    let estimated_after_bytes = current_disk_bytes.saturating_sub(estimated_saved_bytes);

    Ok(ScanPreview {
        path: path_str.to_string(),
        suggested_name,
        detected_exe,
        anti_cheat_detected,
        anti_cheat_name,
        total_files: files.len() as u64,
        total_logical_bytes,
        current_disk_bytes,
        duplicate_files: duplicates.len() as u64,
        duplicate_reclaimable_bytes,
        compressible_bytes,
        high_entropy_bytes,
        cold_vault_candidates,
        cold_vault_bytes,
        estimated_after_bytes,
        estimated_saved_bytes,
        categories,
    })
}
