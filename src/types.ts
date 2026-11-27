export type CompressionMode =
  | "max_crush"
  | "brute_max"
  | "adaptive_thermal"
  | "hyper_archive";

export interface CompressRequest {
  path: string;
  name?: string;
  mode: CompressionMode;
  enable_hardlink_dedup: boolean;
  enable_cold_vault: boolean;
}

export interface GameRecord {
  id: string;
  name: string;
  path: string;
  exe_path: string | null;
  anti_cheat_detected: boolean;
  anti_cheat_name: string | null;
  mode: string;
  before_bytes: number;
  after_bytes: number;
  saved_bytes: number;
  ratio_pct: number;
  dedup_saved_bytes: number;
  lzx_saved_bytes: number;
  vault_saved_bytes: number;
  files_total: number;
  files_compressed: number;
  files_hardlinked: number;
  files_skipped: number;
  files_vaulted: number;
  duration_secs: number;
  last_compressed_at: string;
  status: string;
}

export interface FileAuditRecord {
  id: number;
  game_id: string;
  rel_path: string;
  category: string;
  before_bytes: number;
  after_bytes: number;
  saved_bytes: number;
  action: string;
  entropy: number;
  blake3_hash: string | null;
  hardlink_target: string | null;
  vault_path: string | null;
}

export interface CategoryBreakdown {
  category: string;
  file_count: number;
  before_bytes: number;
  after_bytes: number;
  saved_bytes: number;
}

export interface CompressionRun {
  id: number;
  game_id: string;
  mode: string;
  before_bytes: number;
  after_bytes: number;
  saved_bytes: number;
  duration_secs: number;
  created_at: string;
}

export interface ScanPreview {
  path: string;
  suggested_name: string;
  detected_exe: string | null;
  anti_cheat_detected: boolean;
  anti_cheat_name: string | null;
  total_files: number;
  total_logical_bytes: number;
  current_disk_bytes: number;
  duplicate_files: number;
  duplicate_reclaimable_bytes: number;
  chrono_delta_pairs: number;
  compressible_bytes: number;
  high_entropy_bytes: number;
  cold_vault_candidates: number;
  cold_vault_bytes: number;
  estimated_after_bytes: number;
  estimated_saved_bytes: number;
  categories: CategoryBreakdown[];
}

export interface ModeEstimate {
  mode: string;
  label: string;
  playable_directly: boolean;
  estimated_after_bytes: number;
  estimated_saved_bytes: number;
  estimated_saved_pct: number;
}

export interface BenchmarkReport {
  sample_count: number;
  total_game_bytes: number;
  duplicate_saved_bytes: number;
  estimates: ModeEstimate[];
}

export interface GameDetailResponse {
  game: GameRecord;
  categories: CategoryBreakdown[];
  audits: FileAuditRecord[];
  runs: CompressionRun[];
}

export interface GlobalStats {
  total_games: number;
  total_before_bytes: number;
  total_after_bytes: number;
  total_saved_bytes: number;
  avg_ratio_pct: number;
  total_files_hardlinked: number;
  total_dedup_saved_bytes: number;
  total_lzx_saved_bytes: number;
  total_vault_saved_bytes: number;
}

export interface ProgressEvent {
  phase: string;
  current_file: string;
  processed_files: number;
  total_files: number;
  saved_so_far_bytes: number;
  pct: number;
}
