// Hides the extra console window in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    ganbaru_tauri_app::run(tauri::generate_context!())
}
