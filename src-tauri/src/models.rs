use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompressionMode {
    /// Playable Max: Strongest native Windows compression (LZX) + skips already-compressed files.
    MaxCrush,
    /// Playable Extreme: Forces strongest Windows compression (LZX) on every single file.
    BruteMax,
    /// Playable Balanced: Fast load times for open-world games (LZX on static + XPRESS8K on streaming).
    AdaptiveThermal,
    /// Ultra Pack (Auto-Launch): Custom Smart-Delta + Texture-Planar + Auto-Transform + Zstd-19 with auto-unpacking .exe proxy & Instant Re-Seal on exit!
    UltraAutoLaunch,
}

impl CompressionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            CompressionMode::MaxCrush => "max_crush",
            CompressionMode::BruteMax => "brute_max",
            CompressionMode::AdaptiveThermal => "adaptive_thermal",
            CompressionMode::UltraAutoLaunch => "ultra_auto_launch",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "brute_max" => CompressionMode::BruteMax,
            "adaptive_thermal" => CompressionMode::AdaptiveThermal,
            "ultra_auto_launch" => CompressionMode::UltraAutoLaunch,
            _ => CompressionMode::MaxCrush,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressRequest {
    pub path: String,
    pub name: Option<String>,
    pub mode: CompressionMode,
    pub enable_hardlink_dedup: bool,
    pub clean_redist_bloat: bool,
    pub only_uncompressed: bool,
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
    /// Stores BLAKE3 hash for Hardlinked files, or saved Transform Recipe for Ultra Pack files!
    pub blake3_hash: Option<String>,
    /// Stores canonical target for Hardlinked files, or Reference I-Asset path for Smart Delta files!
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
    pub bloat_files: u64,
    pub bloat_bytes: u64,
    pub compressible_bytes: u64,
    pub high_entropy_bytes: u64,
    pub estimated_after_bytes: u64,
    pub estimated_saved_bytes: u64,
    pub categories: Vec<CategoryBreakdown>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeEstimate {
    pub mode: String,
    pub label: String,
    pub estimated_after_bytes: u64,
    pub estimated_saved_bytes: u64,
    pub estimated_saved_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub sample_count: u64,
    pub total_game_bytes: u64,
    pub duplicate_saved_bytes: u64,
    pub bloat_saved_bytes: u64,
    pub estimates: Vec<ModeEstimate>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherStatus {
    pub enabled: bool,
    pub auto_recompress: bool,
    pub pending_updates: Vec<GameUpdateNotice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameUpdateNotice {
    pub game_id: String,
    pub game_name: String,
    pub uncompressed_files: u64,
    pub uncompressed_bytes: u64,
}
