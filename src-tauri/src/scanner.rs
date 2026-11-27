use crate::hypercodec;
use crate::models::{BenchmarkReport, CategoryBreakdown, ModeEstimate, ScanPreview};
use crate::win_fs;
use anyhow::{anyhow, Result};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const VAULT_DIR_NAME: &str = ".gameshrink_vault";
pub const SHIM_CONFIG_NAME: &str = ".gameshrink_shim.json";
pub const HIGH_ENTROPY_THRESHOLD: f64 = 7.82;
pub const MIN_COMPRESS_BYTES: u64 = 8 * 1024;

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
    pub is_bloat_redist: bool,
}

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
            "Textures & Game Packs".to_string()
        }
        "wav" | "ogg" | "wem" | "bnk" | "pck" | "fsb" | "mp3" | "flac" | "xwb" | "xsb" => {
            "Audio & Voice".to_string()
        }
        "bik" | "bk2" | "mp4" | "webm" | "mkv" | "usm" | "mov" | "avi" => {
            "Video & Cutscenes".to_string()
        }
        "exe" | "dll" | "so" | "sys" | "pdb" | "lib" => "Programs & DLLs".to_string(),
        "json" | "xml" | "ini" | "cfg" | "yaml" | "yml" | "csv" | "txt" | "lua" | "py" | "js"
        | "locres" => "Text & Config".to_string(),
        _ => "Other Files".to_string(),
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

pub fn is_bloat_redist(rel_path: &str) -> bool {
    let lower = rel_path.to_lowercase();
    lower.contains("_commonredist/")
        || lower.contains("redist/")
        || lower.contains("directx/")
        || lower.contains("vcredist")
        || lower.contains("ue4prereqsetup")
        || lower.contains("ueprereqsetup")
        || lower.ends_with(".pdb")
        || lower.ends_with(".dmp")
        || lower.ends_with(".mdmp")
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
                .map(|n| n != VAULT_DIR_NAME && n != SHIM_CONFIG_NAME && !n.ends_with(".gs_orig"))
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
                is_bloat_redist: is_bloat_redist(&rel_path),
            })
        })
        .collect();

    Ok(scanned)
}

pub fn count_uncompressed_updates(root: &Path) -> (u64, u64) {
    if !root.exists() || !root.is_dir() {
        return (0, 0);
    }
    let mut count = 0u64;
    let mut bytes = 0u64;
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            e.file_name()
                .to_str()
                .map(|n| n != VAULT_DIR_NAME && n != SHIM_CONFIG_NAME && !n.ends_with(".gs_orig"))
                .unwrap_or(true)
        })
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if let Ok(meta) = fs::metadata(path) {
            let logical = meta.len();
            if logical < 1024 * 1024 {
                continue;
            }
            let disk = win_fs::get_disk_size(path);
            if disk >= logical {
                let ent = sample_entropy(path, logical);
                if ent <= HIGH_ENTROPY_THRESHOLD {
                    count += 1;
                    bytes += logical;
                }
            }
        }
    }
    (count, bytes)
}

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
    let mut bloat_files = 0u64;
    let mut bloat_bytes = 0u64;
    let mut compressible_bytes = 0u64;
    let mut high_entropy_bytes = 0u64;
    let mut estimated_saved_bytes = 0u64;

    let mut cat_map: HashMap<String, CategoryBreakdown> = HashMap::new();

    for f in &files {
        total_logical_bytes += f.logical_bytes;
        current_disk_bytes += f.disk_bytes;

        if f.is_bloat_redist {
            bloat_files += 1;
            bloat_bytes += f.disk_bytes;
        }

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
            let est_ratio = ((8.0 - f.entropy) / 8.0).clamp(0.08, 0.72);
            est_file_saved = (f.disk_bytes as f64 * est_ratio) as u64;
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
        bloat_files,
        bloat_bytes,
        compressible_bytes,
        high_entropy_bytes,
        estimated_after_bytes,
        estimated_saved_bytes,
        categories,
    })
}

pub fn estimate_all_modes(path_str: &str) -> Result<BenchmarkReport> {
    let root = PathBuf::from(path_str);
    let files = collect_game_files(&root)?;
    let duplicates = find_intra_game_duplicates(&files);

    let total_game_bytes: u64 = files.iter().map(|f| f.disk_bytes).sum();
    let duplicate_saved_bytes: u64 = files
        .iter()
        .filter(|f| duplicates.contains_key(&f.rel_path))
        .map(|f| f.disk_bytes)
        .sum();
    let bloat_saved_bytes: u64 = files
        .iter()
        .filter(|f| f.is_bloat_redist)
        .map(|f| f.disk_bytes)
        .sum();

    let non_dup: Vec<&ScannedFile> = files
        .iter()
        .filter(|f| !duplicates.contains_key(&f.rel_path) && f.logical_bytes >= MIN_COMPRESS_BYTES)
        .collect();

    let compressible_total: u64 = non_dup
        .iter()
        .filter(|f| f.entropy <= HIGH_ENTROPY_THRESHOLD)
        .map(|f| f.disk_bytes)
        .sum();
    let streaming_compressible: u64 = non_dup
        .iter()
        .filter(|f| f.entropy <= HIGH_ENTROPY_THRESHOLD && f.is_streaming_archive)
        .map(|f| f.disk_bytes)
        .sum();
    let static_compressible = compressible_total.saturating_sub(streaming_compressible);
    let high_entropy_total: u64 = non_dup
        .iter()
        .filter(|f| f.entropy > HIGH_ENTROPY_THRESHOLD)
        .map(|f| f.disk_bytes)
        .sum();

    let sample_pool: Vec<&ScannedFile> = non_dup
        .iter()
        .copied()
        .filter(|f| f.logical_bytes >= 64 * 1024 && f.logical_bytes <= 12 * 1024 * 1024)
        .collect();

    let sim_input: Vec<(String, PathBuf, u64)> = sample_pool
        .iter()
        .take(1000)
        .map(|f| (f.rel_path.clone(), f.abs_path.clone(), f.logical_bytes))
        .collect();
    let sim_pairs = hypercodec::find_similarity_pairs(&sim_input);

    let mut picked: Vec<&ScannedFile> = Vec::new();
    for f in &sample_pool {
        if picked.len() >= 10 {
            break;
        }
        if f.entropy <= HIGH_ENTROPY_THRESHOLD {
            picked.push(f);
        }
    }
    for f in &sample_pool {
        if picked.len() >= 14 {
            break;
        }
        if f.entropy > HIGH_ENTROPY_THRESHOLD {
            picked.push(f);
        }
    }

    let temp_dir = std::env::temp_dir().join("gameshrink_bench_v4");
    let _ = fs::create_dir_all(&temp_dir);

    struct SampleMeasure {
        is_high_entropy: bool,
        raw: u64,
        fast_out: u64,
        strong_out: u64,
        ultra_out: u64,
    }

    let measures: Vec<SampleMeasure> = picked
        .par_iter()
        .filter_map(|f| {
            let raw = fs::read(&f.abs_path).ok()?;
            let raw_len = raw.len() as u64;
            let fast_out = zstd::bulk::compress(&raw, 1).ok()?.len() as u64;
            let strong_out = zstd::bulk::compress(&raw, 9).ok()?.len() as u64;

            let ref_abs = sim_pairs.get(&f.rel_path).map(|r| root.join(r));
            let tmp_out = temp_dir.join(format!(
                "{}.gsh",
                blake3::hash(f.rel_path.as_bytes()).to_hex()
            ));
            let ultra_out = hypercodec::encode_ultra_file(
                &f.abs_path,
                &tmp_out,
                ref_abs.as_deref(),
                None,
                19,
            )
            .map(|r| r.compressed_size)
            .unwrap_or(strong_out)
            .min(strong_out);
            let _ = fs::remove_file(&tmp_out);

            Some(SampleMeasure {
                is_high_entropy: f.entropy > HIGH_ENTROPY_THRESHOLD,
                raw: raw_len,
                fast_out: fast_out.min(raw_len),
                strong_out: strong_out.min(raw_len),
                ultra_out: ultra_out.min(raw_len),
            })
        })
        .collect();

    let _ = fs::remove_dir_all(&temp_dir);

    let (comp_raw, comp_fast, comp_strong, comp_ultra) = measures
        .iter()
        .filter(|m| !m.is_high_entropy)
        .fold((0u64, 0u64, 0u64, 0u64), |acc, m| {
            (
                acc.0 + m.raw,
                acc.1 + m.fast_out,
                acc.2 + m.strong_out,
                acc.3 + m.ultra_out,
            )
        });

    let (hi_raw, hi_strong, hi_ultra) = measures
        .iter()
        .filter(|m| m.is_high_entropy)
        .fold((0u64, 0u64, 0u64), |acc, m| {
            (acc.0 + m.raw, acc.1 + m.strong_out, acc.2 + m.ultra_out)
        });

    let lzx_ratio_comp = if comp_raw > 0 {
        ((comp_raw.saturating_sub(comp_strong)) as f64 / comp_raw as f64) * 0.92
    } else {
        0.30
    };
    let xpress_ratio_comp = if comp_raw > 0 {
        ((comp_raw.saturating_sub(comp_fast)) as f64 / comp_raw as f64) * 0.78
    } else {
        0.22
    };
    let ultra_ratio_comp = if comp_raw > 0 {
        (comp_raw.saturating_sub(comp_ultra)) as f64 / comp_raw as f64
    } else {
        0.42
    };

    let lzx_ratio_hi = if hi_raw > 0 {
        ((hi_raw.saturating_sub(hi_strong)) as f64 / hi_raw as f64) * 0.85
    } else {
        0.02
    };
    let ultra_ratio_hi = if hi_raw > 0 {
        (hi_raw.saturating_sub(hi_ultra)) as f64 / hi_raw as f64
    } else {
        0.05
    };

    let make_est = |mode: &str, label: &str, saved: u64| {
        let clamped_saved = saved.min(total_game_bytes);
        let after = total_game_bytes.saturating_sub(clamped_saved);
        let pct = if total_game_bytes > 0 {
            (clamped_saved as f64 / total_game_bytes as f64) * 100.0
        } else {
            0.0
        };
        ModeEstimate {
            mode: mode.to_string(),
            label: label.to_string(),
            estimated_after_bytes: after,
            estimated_saved_bytes: clamped_saved,
            estimated_saved_pct: pct,
        }
    };

    let saved_balanced = duplicate_saved_bytes
        + (static_compressible as f64 * lzx_ratio_comp) as u64
        + (streaming_compressible as f64 * xpress_ratio_comp) as u64;

    let saved_max = duplicate_saved_bytes + (compressible_total as f64 * lzx_ratio_comp) as u64;

    let saved_extreme = duplicate_saved_bytes
        + (compressible_total as f64 * lzx_ratio_comp) as u64
        + (high_entropy_total as f64 * lzx_ratio_hi) as u64;

    let saved_ultra = duplicate_saved_bytes
        + (compressible_total as f64 * ultra_ratio_comp) as u64
        + (high_entropy_total as f64 * ultra_ratio_hi) as u64;

    let estimates = vec![
        make_est("adaptive_thermal", "Playable Balanced", saved_balanced),
        make_est("max_crush", "Playable Max", saved_max),
        make_est("brute_max", "Playable Extreme", saved_extreme),
        make_est("ultra_auto_launch", "Ultra Pack (Auto-Launch)", saved_ultra),
    ];

    Ok(BenchmarkReport {
        sample_count: measures.len() as u64,
        total_game_bytes,
        duplicate_saved_bytes,
        bloat_saved_bytes,
        estimates,
    })
}
