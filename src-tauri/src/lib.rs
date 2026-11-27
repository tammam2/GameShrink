mod db;
mod engine;
mod hypercodec;
mod models;
mod scanner;
mod win_fs;

use db::LedgerDb;
use models::{
    BenchmarkReport, CompressRequest, CompressionMode, GameDetailResponse, GameRecord,
    GameUpdateNotice, GlobalStats, ScanPreview, WatcherStatus,
};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

struct CancelFlag(Arc<AtomicBool>);

struct WatcherState {
    enabled: AtomicBool,
    auto_recompress: AtomicBool,
    busy: AtomicBool,
    pending: Mutex<Vec<GameUpdateNotice>>,
}

/// Computes the exact same SQLite ledger path used by Tauri's `app_local_data_dir()` on Windows.
fn resolve_cli_db_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local)
                .join("com.gameshrink.app")
                .join("gameshrink_history.db");
        }
    }
    PathBuf::from("gameshrink_history.db")
}

/// Entrypoint invoked silently when the user double-clicks a shimmed `game.exe`!
pub fn run_ultra_play_session_cli(game_id: &str, extra_args: &[String]) -> anyhow::Result<()> {
    let db_path = resolve_cli_db_path();
    let ledger = LedgerDb::open(&db_path)?;
    engine::run_ultra_play_session(&ledger, game_id, extra_args)
}

#[tauri::command]
fn cancel_operation(cancel: State<'_, CancelFlag>) {
    cancel.0.store(true, Ordering::Relaxed);
}

#[tauri::command]
async fn scan_game_folder(path: String) -> Result<ScanPreview, String> {
    tauri::async_runtime::spawn_blocking(move || scanner::scan_game_preview(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn estimate_all_modes(path: String) -> Result<BenchmarkReport, String> {
    tauri::async_runtime::spawn_blocking(move || scanner::estimate_all_modes(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn compress_game(
    app: AppHandle,
    db: State<'_, Arc<LedgerDb>>,
    cancel: State<'_, CancelFlag>,
    watcher: State<'_, Arc<WatcherState>>,
    request: CompressRequest,
) -> Result<GameRecord, String> {
    cancel.0.store(false, Ordering::Relaxed);
    watcher.busy.store(true, Ordering::Relaxed);
    let db_arc = Arc::clone(&db);
    let cancel_arc = Arc::clone(&cancel.0);
    let watcher_arc = Arc::clone(&watcher);

    let res = tauri::async_runtime::spawn_blocking(move || {
        let out = engine::run_compression(&app, &db_arc, &cancel_arc, request);
        watcher_arc.busy.store(false, Ordering::Relaxed);
        out
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    if let Ok(mut list) = watcher.pending.lock() {
        list.retain(|u| u.game_id != res.id);
    }
    Ok(res)
}

#[tauri::command]
async fn recompress_game(
    app: AppHandle,
    db: State<'_, Arc<LedgerDb>>,
    cancel: State<'_, CancelFlag>,
    watcher: State<'_, Arc<WatcherState>>,
    game_id: String,
    mode: Option<CompressionMode>,
    only_uncompressed: bool,
) -> Result<GameRecord, String> {
    let detail = db.get_game_details(&game_id).map_err(|e| e.to_string())?;
    let target_mode = mode.unwrap_or_else(|| CompressionMode::from_str(&detail.game.mode));

    let req = CompressRequest {
        path: detail.game.path,
        name: Some(detail.game.name),
        mode: target_mode,
        enable_hardlink_dedup: true,
        clean_redist_bloat: false,
        only_uncompressed,
    };

    cancel.0.store(false, Ordering::Relaxed);
    watcher.busy.store(true, Ordering::Relaxed);
    let db_arc = Arc::clone(&db);
    let cancel_arc = Arc::clone(&cancel.0);
    let watcher_arc = Arc::clone(&watcher);

    let res = tauri::async_runtime::spawn_blocking(move || {
        let out = engine::run_compression(&app, &db_arc, &cancel_arc, req);
        watcher_arc.busy.store(false, Ordering::Relaxed);
        out
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    if let Ok(mut list) = watcher.pending.lock() {
        list.retain(|u| u.game_id != res.id);
    }
    Ok(res)
}

#[tauri::command]
async fn restore_game(
    app: AppHandle,
    db: State<'_, Arc<LedgerDb>>,
    cancel: State<'_, CancelFlag>,
    watcher: State<'_, Arc<WatcherState>>,
    game_id: String,
    break_hardlinks: bool,
) -> Result<GameRecord, String> {
    cancel.0.store(false, Ordering::Relaxed);
    watcher.busy.store(true, Ordering::Relaxed);
    let db_arc = Arc::clone(&db);
    let cancel_arc = Arc::clone(&cancel.0);
    let watcher_arc = Arc::clone(&watcher);

    tauri::async_runtime::spawn_blocking(move || {
        let out = engine::restore_game(&app, &db_arc, &cancel_arc, &game_id, break_hardlinks);
        watcher_arc.busy.store(false, Ordering::Relaxed);
        out
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn play_ultra_game(
    db: State<'_, Arc<LedgerDb>>,
    game_id: String,
) -> Result<(), String> {
    let db_arc = Arc::clone(&db);
    tauri::async_runtime::spawn_blocking(move || {
        engine::run_ultra_play_session(&db_arc, &game_id, &[])
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_watcher_status(watcher: State<'_, Arc<WatcherState>>) -> WatcherStatus {
    let pending = watcher
        .pending
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default();
    WatcherStatus {
        enabled: watcher.enabled.load(Ordering::Relaxed),
        auto_recompress: watcher.auto_recompress.load(Ordering::Relaxed),
        pending_updates: pending,
    }
}

#[tauri::command]
fn set_watcher_settings(
    watcher: State<'_, Arc<WatcherState>>,
    enabled: bool,
    auto_recompress: bool,
) -> WatcherStatus {
    watcher.enabled.store(enabled, Ordering::Relaxed);
    watcher
        .auto_recompress
        .store(auto_recompress, Ordering::Relaxed);
    get_watcher_status(watcher)
}

#[tauri::command]
async fn check_updates_now(
    app: AppHandle,
    db: State<'_, Arc<LedgerDb>>,
    watcher: State<'_, Arc<WatcherState>>,
) -> Result<WatcherStatus, String> {
    let db_arc = Arc::clone(&db);
    let watcher_arc = Arc::clone(&watcher);
    tauri::async_runtime::spawn_blocking(move || {
        let notices = scan_tracked_updates(&db_arc);
        if let Ok(mut p) = watcher_arc.pending.lock() {
            *p = notices.clone();
        }
        let status = WatcherStatus {
            enabled: watcher_arc.enabled.load(Ordering::Relaxed),
            auto_recompress: watcher_arc.auto_recompress.load(Ordering::Relaxed),
            pending_updates: notices,
        };
        let _ = app.emit("watcher-update", &status);
        status
    })
    .await
    .map_err(|e| e.to_string())
}

fn scan_tracked_updates(db: &LedgerDb) -> Vec<GameUpdateNotice> {
    let games = match db.list_games() {
        Ok(g) => g,
        Err(_) => return Vec::new(),
    };
    let mut notices = Vec::new();
    for g in games {
        if g.status != "compressed" {
            continue;
        }
        let (cnt, bytes) = scanner::count_uncompressed_updates(&PathBuf::from(&g.path));
        if cnt > 0 {
            notices.push(GameUpdateNotice {
                game_id: g.id,
                game_name: g.name,
                uncompressed_files: cnt,
                uncompressed_bytes: bytes,
            });
        }
    }
    notices
}

#[tauri::command]
fn list_games(db: State<'_, Arc<LedgerDb>>) -> Result<Vec<GameRecord>, String> {
    db.list_games().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_game_details(
    db: State<'_, Arc<LedgerDb>>,
    game_id: String,
) -> Result<GameDetailResponse, String> {
    db.get_game_details(&game_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_global_stats(db: State<'_, Arc<LedgerDb>>) -> Result<GlobalStats, String> {
    db.get_global_stats().map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_game_record(db: State<'_, Arc<LedgerDb>>, game_id: String) -> Result<(), String> {
    db.delete_game_record(&game_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn launch_game_exe(exe_path: String) -> Result<(), String> {
    let path = PathBuf::from(&exe_path);
    if !path.exists() {
        return Err(format!("Executable not found: {}", exe_path));
    }
    let work_dir = path
        .parent()
        .ok_or_else(|| "Cannot determine executable directory".to_string())?;
    Command::new(&path)
        .current_dir(work_dir)
        .spawn()
        .map_err(|e| format!("Failed to launch {}: {}", exe_path, e))?;
    Ok(())
}

#[tauri::command]
fn open_folder(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_local_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            let db_path = data_dir.join("gameshrink_history.db");
            let ledger = Arc::new(LedgerDb::open(&db_path)?);
            let cancel_flag = Arc::new(AtomicBool::new(false));
            let watcher = Arc::new(WatcherState {
                enabled: AtomicBool::new(true),
                auto_recompress: AtomicBool::new(false),
                busy: AtomicBool::new(false),
                pending: Mutex::new(Vec::new()),
            });

            app.manage(Arc::clone(&ledger));
            app.manage(CancelFlag(Arc::clone(&cancel_flag)));
            app.manage(Arc::clone(&watcher));

            let app_handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(45));
                if !watcher.enabled.load(Ordering::Relaxed)
                    || watcher.busy.load(Ordering::Relaxed)
                {
                    continue;
                }

                let notices = scan_tracked_updates(&ledger);
                let auto = watcher.auto_recompress.load(Ordering::Relaxed);

                if auto && !notices.is_empty() && !watcher.busy.load(Ordering::Relaxed) {
                    watcher.busy.store(true, Ordering::Relaxed);
                    cancel_flag.store(false, Ordering::Relaxed);
                    for n in &notices {
                        if let Ok(detail) = ledger.get_game_details(&n.game_id) {
                            let req = CompressRequest {
                                path: detail.game.path,
                                name: Some(detail.game.name),
                                mode: CompressionMode::from_str(&detail.game.mode),
                                enable_hardlink_dedup: true,
                                clean_redist_bloat: false,
                                only_uncompressed: true,
                            };
                            let _ = engine::run_compression(
                                &app_handle,
                                &ledger,
                                &cancel_flag,
                                req,
                            );
                        }
                    }
                    watcher.busy.store(false, Ordering::Relaxed);
                    let remaining = scan_tracked_updates(&ledger);
                    if let Ok(mut p) = watcher.pending.lock() {
                        *p = remaining.clone();
                    }
                    let _ = app_handle.emit(
                        "watcher-update",
                        WatcherStatus {
                            enabled: watcher.enabled.load(Ordering::Relaxed),
                            auto_recompress: auto,
                            pending_updates: remaining,
                        },
                    );
                } else {
                    if let Ok(mut p) = watcher.pending.lock() {
                        *p = notices.clone();
                    }
                    let _ = app_handle.emit(
                        "watcher-update",
                        WatcherStatus {
                            enabled: watcher.enabled.load(Ordering::Relaxed),
                            auto_recompress: auto,
                            pending_updates: notices,
                        },
                    );
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            cancel_operation,
            scan_game_folder,
            estimate_all_modes,
            compress_game,
            recompress_game,
            restore_game,
            play_ultra_game,
            get_watcher_status,
            set_watcher_settings,
            check_updates_now,
            list_games,
            get_game_details,
            get_global_stats,
            delete_game_record,
            launch_game_exe,
            open_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running GameShrink");
}
