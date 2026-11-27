mod db;
mod engine;
mod hypercodec;
mod models;
mod scanner;
mod win_fs;

use db::LedgerDb;
use models::{
    BenchmarkReport, CompressRequest, GameDetailResponse, GameRecord, GlobalStats, ScanPreview,
};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

struct CancelFlag(Arc<AtomicBool>);

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
    request: CompressRequest,
) -> Result<GameRecord, String> {
    cancel.0.store(false, Ordering::Relaxed);
    let db_arc = Arc::clone(&db);
    let cancel_arc = Arc::clone(&cancel.0);
    tauri::async_runtime::spawn_blocking(move || {
        engine::run_compression(&app, &db_arc, &cancel_arc, request)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn restore_game(
    app: AppHandle,
    db: State<'_, Arc<LedgerDb>>,
    cancel: State<'_, CancelFlag>,
    game_id: String,
    break_hardlinks: bool,
) -> Result<GameRecord, String> {
    cancel.0.store(false, Ordering::Relaxed);
    let db_arc = Arc::clone(&db);
    let cancel_arc = Arc::clone(&cancel.0);
    tauri::async_runtime::spawn_blocking(move || {
        engine::restore_game(&app, &db_arc, &cancel_arc, &game_id, break_hardlinks)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
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
            app.manage(ledger);
            app.manage(CancelFlag(Arc::new(AtomicBool::new(false))));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            cancel_operation,
            scan_game_folder,
            estimate_all_modes,
            compress_game,
            restore_game,
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
