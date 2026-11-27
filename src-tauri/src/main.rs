// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "--play-ultra" {
        let game_id = args[2].clone();
        let extra_args: Vec<String> = if let Some(pos) = args.iter().position(|a| a == "--") {
            args[pos + 1..].to_vec()
        } else {
            Vec::new()
        };
        let _ = gameshrink_lib::run_ultra_play_session_cli(&game_id, &extra_args);
        return;
    }

    gameshrink_lib::run()
}
