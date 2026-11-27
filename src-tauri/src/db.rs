use crate::models::{
    CategoryBreakdown, CompressionRun, FileAuditRecord, GameDetailResponse, GameRecord, GlobalStats,
};
use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

pub struct LedgerDb {
    conn: Mutex<Connection>,
}

impl LedgerDb {
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS games (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL UNIQUE,
                exe_path TEXT,
                anti_cheat_detected INTEGER NOT NULL DEFAULT 0,
                anti_cheat_name TEXT,
                mode TEXT NOT NULL,
                before_bytes INTEGER NOT NULL,
                after_bytes INTEGER NOT NULL,
                saved_bytes INTEGER NOT NULL,
                ratio_pct REAL NOT NULL,
                dedup_saved_bytes INTEGER NOT NULL DEFAULT 0,
                lzx_saved_bytes INTEGER NOT NULL DEFAULT 0,
                vault_saved_bytes INTEGER NOT NULL DEFAULT 0,
                files_total INTEGER NOT NULL DEFAULT 0,
                files_compressed INTEGER NOT NULL DEFAULT 0,
                files_hardlinked INTEGER NOT NULL DEFAULT 0,
                files_skipped INTEGER NOT NULL DEFAULT 0,
                files_vaulted INTEGER NOT NULL DEFAULT 0,
                duration_secs REAL NOT NULL DEFAULT 0.0,
                last_compressed_at TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'compressed'
            );

            CREATE TABLE IF NOT EXISTS file_audits (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                game_id TEXT NOT NULL,
                rel_path TEXT NOT NULL,
                category TEXT NOT NULL,
                before_bytes INTEGER NOT NULL,
                after_bytes INTEGER NOT NULL,
                saved_bytes INTEGER NOT NULL,
                action TEXT NOT NULL,
                entropy REAL NOT NULL,
                blake3_hash TEXT,
                hardlink_target TEXT,
                vault_path TEXT,
                FOREIGN KEY(game_id) REFERENCES games(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_file_audits_game ON file_audits(game_id);
            CREATE INDEX IF NOT EXISTS idx_file_audits_action ON file_audits(game_id, action);

            CREATE TABLE IF NOT EXISTS compression_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                game_id TEXT NOT NULL,
                mode TEXT NOT NULL,
                before_bytes INTEGER NOT NULL,
                after_bytes INTEGER NOT NULL,
                saved_bytes INTEGER NOT NULL,
                duration_secs REAL NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY(game_id) REFERENCES games(id) ON DELETE CASCADE
            );
            ",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn find_game_id_by_path(&self, path: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id FROM games WHERE path = ?1")?;
        let mut rows = stmt.query(params![path])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn upsert_game(&self, game: &GameRecord, audits: &[FileAuditRecord]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        tx.execute(
            "
            INSERT INTO games (
                id, name, path, exe_path, anti_cheat_detected, anti_cheat_name, mode,
                before_bytes, after_bytes, saved_bytes, ratio_pct,
                dedup_saved_bytes, lzx_saved_bytes, vault_saved_bytes,
                files_total, files_compressed, files_hardlinked, files_skipped, files_vaulted,
                duration_secs, last_compressed_at, status
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7,
                ?8, ?9, ?10, ?11,
                ?12, ?13, ?14,
                ?15, ?16, ?17, ?18, ?19,
                ?20, ?21, ?22
            )
            ON CONFLICT(path) DO UPDATE SET
                name = excluded.name,
                exe_path = excluded.exe_path,
                anti_cheat_detected = excluded.anti_cheat_detected,
                anti_cheat_name = excluded.anti_cheat_name,
                mode = excluded.mode,
                before_bytes = excluded.before_bytes,
                after_bytes = excluded.after_bytes,
                saved_bytes = excluded.saved_bytes,
                ratio_pct = excluded.ratio_pct,
                dedup_saved_bytes = excluded.dedup_saved_bytes,
                lzx_saved_bytes = excluded.lzx_saved_bytes,
                vault_saved_bytes = excluded.vault_saved_bytes,
                files_total = excluded.files_total,
                files_compressed = excluded.files_compressed,
                files_hardlinked = excluded.files_hardlinked,
                files_skipped = excluded.files_skipped,
                files_vaulted = excluded.files_vaulted,
                duration_secs = excluded.duration_secs,
                last_compressed_at = excluded.last_compressed_at,
                status = excluded.status
            ",
            params![
                game.id,
                game.name,
                game.path,
                game.exe_path,
                if game.anti_cheat_detected { 1 } else { 0 },
                game.anti_cheat_name,
                game.mode,
                game.before_bytes as i64,
                game.after_bytes as i64,
                game.saved_bytes as i64,
                game.ratio_pct,
                game.dedup_saved_bytes as i64,
                game.lzx_saved_bytes as i64,
                game.vault_saved_bytes as i64,
                game.files_total as i64,
                game.files_compressed as i64,
                game.files_hardlinked as i64,
                game.files_skipped as i64,
                game.files_vaulted as i64,
                game.duration_secs,
                game.last_compressed_at,
                game.status,
            ],
        )?;

        tx.execute("DELETE FROM file_audits WHERE game_id = ?1", params![game.id])?;

        {
            let mut stmt = tx.prepare(
                "
                INSERT INTO file_audits (
                    game_id, rel_path, category, before_bytes, after_bytes, saved_bytes,
                    action, entropy, blake3_hash, hardlink_target, vault_path
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                ",
            )?;

            for a in audits {
                stmt.execute(params![
                    game.id,
                    a.rel_path,
                    a.category,
                    a.before_bytes as i64,
                    a.after_bytes as i64,
                    a.saved_bytes as i64,
                    a.action,
                    a.entropy,
                    a.blake3_hash,
                    a.hardlink_target,
                    a.vault_path,
                ])?;
            }
        }

        tx.execute(
            "
            INSERT INTO compression_runs (
                game_id, mode, before_bytes, after_bytes, saved_bytes, duration_secs, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            params![
                game.id,
                game.mode,
                game.before_bytes as i64,
                game.after_bytes as i64,
                game.saved_bytes as i64,
                game.duration_secs,
                game.last_compressed_at,
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    pub fn list_games(&self) -> Result<Vec<GameRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "
            SELECT
                id, name, path, exe_path, anti_cheat_detected, anti_cheat_name, mode,
                before_bytes, after_bytes, saved_bytes, ratio_pct,
                dedup_saved_bytes, lzx_saved_bytes, vault_saved_bytes,
                files_total, files_compressed, files_hardlinked, files_skipped, files_vaulted,
                duration_secs, last_compressed_at, status
            FROM games
            ORDER BY last_compressed_at DESC
            ",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(GameRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                exe_path: row.get(3)?,
                anti_cheat_detected: row.get::<_, i64>(4)? != 0,
                anti_cheat_name: row.get(5)?,
                mode: row.get(6)?,
                before_bytes: row.get::<_, i64>(7)? as u64,
                after_bytes: row.get::<_, i64>(8)? as u64,
                saved_bytes: row.get::<_, i64>(9)? as u64,
                ratio_pct: row.get(10)?,
                dedup_saved_bytes: row.get::<_, i64>(11)? as u64,
                lzx_saved_bytes: row.get::<_, i64>(12)? as u64,
                vault_saved_bytes: row.get::<_, i64>(13)? as u64,
                files_total: row.get::<_, i64>(14)? as u64,
                files_compressed: row.get::<_, i64>(15)? as u64,
                files_hardlinked: row.get::<_, i64>(16)? as u64,
                files_skipped: row.get::<_, i64>(17)? as u64,
                files_vaulted: row.get::<_, i64>(18)? as u64,
                duration_secs: row.get(19)?,
                last_compressed_at: row.get(20)?,
                status: row.get(21)?,
            })
        })?;

        let mut games = Vec::new();
        for g in rows {
            games.push(g?);
        }
        Ok(games)
    }

    pub fn get_game_details(&self, game_id: &str) -> Result<GameDetailResponse> {
        let conn = self.conn.lock().unwrap();

        let game = conn.query_row(
            "
            SELECT
                id, name, path, exe_path, anti_cheat_detected, anti_cheat_name, mode,
                before_bytes, after_bytes, saved_bytes, ratio_pct,
                dedup_saved_bytes, lzx_saved_bytes, vault_saved_bytes,
                files_total, files_compressed, files_hardlinked, files_skipped, files_vaulted,
                duration_secs, last_compressed_at, status
            FROM games WHERE id = ?1
            ",
            params![game_id],
            |row| {
                Ok(GameRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    exe_path: row.get(3)?,
                    anti_cheat_detected: row.get::<_, i64>(4)? != 0,
                    anti_cheat_name: row.get(5)?,
                    mode: row.get(6)?,
                    before_bytes: row.get::<_, i64>(7)? as u64,
                    after_bytes: row.get::<_, i64>(8)? as u64,
                    saved_bytes: row.get::<_, i64>(9)? as u64,
                    ratio_pct: row.get(10)?,
                    dedup_saved_bytes: row.get::<_, i64>(11)? as u64,
                    lzx_saved_bytes: row.get::<_, i64>(12)? as u64,
                    vault_saved_bytes: row.get::<_, i64>(13)? as u64,
                    files_total: row.get::<_, i64>(14)? as u64,
                    files_compressed: row.get::<_, i64>(15)? as u64,
                    files_hardlinked: row.get::<_, i64>(16)? as u64,
                    files_skipped: row.get::<_, i64>(17)? as u64,
                    files_vaulted: row.get::<_, i64>(18)? as u64,
                    duration_secs: row.get(19)?,
                    last_compressed_at: row.get(20)?,
                    status: row.get(21)?,
                })
            },
        )?;

        let mut cat_stmt = conn.prepare(
            "
            SELECT category, COUNT(*), SUM(before_bytes), SUM(after_bytes), SUM(saved_bytes)
            FROM file_audits
            WHERE game_id = ?1
            GROUP BY category
            ORDER BY SUM(saved_bytes) DESC
            ",
        )?;
        let categories = cat_stmt
            .query_map(params![game_id], |row| {
                Ok(CategoryBreakdown {
                    category: row.get(0)?,
                    file_count: row.get::<_, i64>(1)? as u64,
                    before_bytes: row.get::<_, i64>(2)? as u64,
                    after_bytes: row.get::<_, i64>(3)? as u64,
                    saved_bytes: row.get::<_, i64>(4)? as u64,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut audit_stmt = conn.prepare(
            "
            SELECT id, game_id, rel_path, category, before_bytes, after_bytes, saved_bytes,
                   action, entropy, blake3_hash, hardlink_target, vault_path
            FROM file_audits
            WHERE game_id = ?1
            ORDER BY saved_bytes DESC, before_bytes DESC
            LIMIT 2500
            ",
        )?;
        let audits = audit_stmt
            .query_map(params![game_id], |row| {
                Ok(FileAuditRecord {
                    id: row.get(0)?,
                    game_id: row.get(1)?,
                    rel_path: row.get(2)?,
                    category: row.get(3)?,
                    before_bytes: row.get::<_, i64>(4)? as u64,
                    after_bytes: row.get::<_, i64>(5)? as u64,
                    saved_bytes: row.get::<_, i64>(6)? as u64,
                    action: row.get(7)?,
                    entropy: row.get(8)?,
                    blake3_hash: row.get(9)?,
                    hardlink_target: row.get(10)?,
                    vault_path: row.get(11)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut run_stmt = conn.prepare(
            "
            SELECT id, game_id, mode, before_bytes, after_bytes, saved_bytes, duration_secs, created_at
            FROM compression_runs
            WHERE game_id = ?1
            ORDER BY id DESC
            LIMIT 25
            ",
        )?;
        let runs = run_stmt
            .query_map(params![game_id], |row| {
                Ok(CompressionRun {
                    id: row.get(0)?,
                    game_id: row.get(1)?,
                    mode: row.get(2)?,
                    before_bytes: row.get::<_, i64>(3)? as u64,
                    after_bytes: row.get::<_, i64>(4)? as u64,
                    saved_bytes: row.get::<_, i64>(5)? as u64,
                    duration_secs: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(GameDetailResponse {
            game,
            categories,
            audits,
            runs,
        })
    }

    pub fn get_all_audits(&self, game_id: &str) -> Result<Vec<FileAuditRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "
            SELECT id, game_id, rel_path, category, before_bytes, after_bytes, saved_bytes,
                   action, entropy, blake3_hash, hardlink_target, vault_path
            FROM file_audits
            WHERE game_id = ?1
            ",
        )?;
        let audits = stmt
            .query_map(params![game_id], |row| {
                Ok(FileAuditRecord {
                    id: row.get(0)?,
                    game_id: row.get(1)?,
                    rel_path: row.get(2)?,
                    category: row.get(3)?,
                    before_bytes: row.get::<_, i64>(4)? as u64,
                    after_bytes: row.get::<_, i64>(5)? as u64,
                    saved_bytes: row.get::<_, i64>(6)? as u64,
                    action: row.get(7)?,
                    entropy: row.get(8)?,
                    blake3_hash: row.get(9)?,
                    hardlink_target: row.get(10)?,
                    vault_path: row.get(11)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(audits)
    }

    pub fn get_global_stats(&self) -> Result<GlobalStats> {
        let conn = self.conn.lock().unwrap();
        let stats = conn.query_row(
            "
            SELECT
                COUNT(*),
                COALESCE(SUM(before_bytes), 0),
                COALESCE(SUM(after_bytes), 0),
                COALESCE(SUM(saved_bytes), 0),
                COALESCE(SUM(files_hardlinked), 0),
                COALESCE(SUM(dedup_saved_bytes), 0),
                COALESCE(SUM(lzx_saved_bytes), 0),
                COALESCE(SUM(vault_saved_bytes), 0)
            FROM games
            WHERE status IN ('compressed', 'archived')
            ",
            [],
            |row| {
                let before = row.get::<_, i64>(1)? as u64;
                let saved = row.get::<_, i64>(3)? as u64;
                let avg_ratio = if before > 0 {
                    (saved as f64 / before as f64) * 100.0
                } else {
                    0.0
                };
                Ok(GlobalStats {
                    total_games: row.get::<_, i64>(0)? as u64,
                    total_before_bytes: before,
                    total_after_bytes: row.get::<_, i64>(2)? as u64,
                    total_saved_bytes: saved,
                    avg_ratio_pct: avg_ratio,
                    total_files_hardlinked: row.get::<_, i64>(4)? as u64,
                    total_dedup_saved_bytes: row.get::<_, i64>(5)? as u64,
                    total_lzx_saved_bytes: row.get::<_, i64>(6)? as u64,
                    total_vault_saved_bytes: row.get::<_, i64>(7)? as u64,
                })
            },
        )?;
        Ok(stats)
    }

    pub fn delete_game_record(&self, game_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM games WHERE id = ?1", params![game_id])?;
        Ok(())
    }
}
