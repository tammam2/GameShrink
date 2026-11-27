use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompressionMode {
    /// Strongest playable mode: Intra-game hardlink dedup + NTFS LZX (game runs directly from .exe)
    MaxCrush,
    /// Forces NTFS LZX on every eligible file regardless of entropy + Intra-game hardlinks
    BruteMax,
    /// Zero-FPS adaptive: LZX on cold/load-once assets + XPRESS8K on streaming archives
    AdaptiveThermal,
    /// Beyond-SOTA Deep Freeze: Stacks Intra-game Dedup + ChronoDelta LSH Residuals + CanonPack BCn + Auto-SIMD Bytecode + Zstd-19
    HyperArchive,
}

impl CompressionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            CompressionMode::MaxCrush => "max_crush",
            CompressionMode::BruteMax => "brute_max",
            CompressionMode::AdaptiveThermal => "adaptive_thermal",
            CompressionMode::HyperArchive => "hyper_archive",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressRequest {
    pub path: String,
    pub name: Option<String>,
    pub mode: CompressionMode,
    pub enable_hardlink_dedup: bool,
    pub enable_cold_vault: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameRecord {
    pub id: String,
    pub name: String,
    pub path: String,
    pub exe_path: Option<String>,
    pub anti_cheat_detected: bool,
    pub anti_cheat_name: Option<String>,
    pub mode: String,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub saved_bytes: u64,
    pub ratio_pct: f64,
    pub dedup_saved_bytes: u64,
    pub lzx_saved_bytes: u64,
    pub vault_saved_bytes: u64,
    pub files_total: u64,
    pub files_compressed: u64,
    pub files_hardlinked: u64,
    pub files_skipped: u64,
    pub files_vaulted: u64,
    pub duration_secs: f64,
    pub last_compressed_at: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAuditRecord {
    pub id: i64,
    pub game_id: String,
    pub rel_path: String,
    pub category: String,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub saved_bytes: u64,
    pub action: String,
    pub entropy: f64,
    pub blake3_hash: Option<String>,
    pub hardlink_target: Option<String>,
    pub vault_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryBreakdown {
    pub category: String,
    pub file_count: u64,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub saved_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionRun {
    pub id: i64,
    pub game_id: String,
    pub mode: String,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub saved_bytes: u64,
    pub duration_secs: f64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanPreview {
    pub path: String,
    pub suggested_name: String,
    pub detected_exe: Option<String>,
    pub anti_cheat_detected: bool,
    pub anti_cheat_name: Option<String>,
    pub total_files: u64,
    pub total_logical_bytes: u64,
    pub current_disk_bytes: u64,
    pub duplicate_files: u64,
    pub duplicate_reclaimable_bytes: u64,
    pub chrono_delta_pairs: u64,
    pub compressible_bytes: u64,
    pub high_entropy_bytes: u64,
    pub cold_vault_candidates: u64,
    pub cold_vault_bytes: u64,
    pub estimated_after_bytes: u64,
    pub estimated_saved_bytes: u64,
    pub categories: Vec<CategoryBreakdown>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSampleRow {
    pub rel_path: String,
    pub raw_bytes: u64,
    pub zstd19_bytes: u64,
    pub hyper_bytes: u64,
    pub winning_codec: String,
    pub extra_gain_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub sample_count: u64,
    pub total_raw_bytes: u64,
    pub total_zstd19_bytes: u64,
    pub total_hyper_bytes: u64,
    pub chrono_delta_pairs_found: u64,
    pub extra_saved_over_zstd_bytes: u64,
    pub rows: Vec<BenchmarkSampleRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameDetailResponse {
    pub game: GameRecord,
    pub categories: Vec<CategoryBreakdown>,
    pub audits: Vec<FileAuditRecord>,
    pub runs: Vec<CompressionRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalStats {
    pub total_games: u64,
    pub total_before_bytes: u64,
    pub total_after_bytes: u64,
    pub total_saved_bytes: u64,
    pub avg_ratio_pct: f64,
    pub total_files_hardlinked: u64,
    pub total_dedup_saved_bytes: u64,
    pub total_lzx_saved_bytes: u64,
    pub total_vault_saved_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressEvent {
    pub phase: String,
    pub current_file: String,
    pub processed_files: u64,
    pub total_files: u64,
    pub saved_so_far_bytes: u64,
    pub pct: f64,
}
