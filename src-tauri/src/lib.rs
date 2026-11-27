mod db;
mod engine;
mod models;
mod scanner;
mod win_fs;

use db::LedgerDb;
use models::{CompressRequest, GameDetailResponse, GameRecord, GlobalStats, ScanPreview};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
async fn scan_game_folder(path: String) -> Result<ScanPreview, String> {
    tauri::async_runtime::spawn_blocking(move || scanner::scan_game_preview(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn compress_game(
    app: AppHandle,
    db: State<'_, Arc<LedgerDb>>,
    request: CompressRequest,
) -> Result<GameRecord, String> {
    let db_arc = Arc::clone(&db);
    tauri::async_runtime::spawn_blocking(move || engine::run_compression(&app, &db_arc, request))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn restore_game(
    app: AppHandle,
    db: State<'_, Arc<LedgerDb>>,
    game_id: String,
    break_hardlinks: bool,
) -> Result<GameRecord, String> {
    let db_arc = Arc::clone(&db);
    tauri::async_runtime::spawn_blocking(move || {
        engine::restore_game(&app, &db_arc, &game_id, break_hardlinks)
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

/// Launches the game executable directly with its own working directory — zero wrapper overhead.
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
            let db_path = data_dir.join("aethercrush_ledger.db");
            let ledger = Arc::new(LedgerDb::open(&db_path)?);
            app.manage(ledger);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            scan_game_folder,
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
        .expect("error while running AetherCrush");
}
