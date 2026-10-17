// Hides the extra console window in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    if let Some(result) = ganbaru_tauri_app::run_privileged_helper_if_requested() {
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    ganbaru_tauri_app::run(tauri::generate_context!())
}
