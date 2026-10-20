//! Desktop benchmark harness support: isolated database lifecycle, cross-restart state,
//! process memory sampling, and dense dataset seeding.

use crate::db;
use std::path::PathBuf;
use tauri::Manager;

pub(crate) mod memory;
pub(crate) mod seed;

/// Path to the persisted benchmark state file. It lives in `app_config_dir`,
/// not the vault, so `reset_database` (which deletes only the SQLite files)
/// cannot remove it mid-run and vault backups never include it. The in-app
/// benchmark harness uses it to carry state from Phase A across the restart
/// into Phase B.
fn benchmark_state_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut path = app.path().app_config_dir().map_err(|e| e.to_string())?;
    path.push("benchmark-state.json");
    Ok(path)
}

/// Path to the isolated SQLite file the benchmark harness uses for both
/// phases. Lives in app config and is never opened during normal app
/// operation. The harness deletes it before each run and after the summary
/// is closed.
fn benchmark_db_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut path = app.path().app_config_dir().map_err(|e| e.to_string())?;
    path.push(db::BENCHMARK_SQLITE_FILE);
    Ok(path)
}

/// Delete the benchmark DB file together with its WAL and SHM sidecars.
/// SQLite on Linux unlinks open files cleanly. The benchmark commands close
/// the managed pool before deleting files so Windows can release handles too.
fn delete_benchmark_db_files(app: &tauri::AppHandle) -> Result<(), String> {
    let base = benchmark_db_path(app)?;
    for suffix in &["", "-wal", "-shm"] {
        let mut path = base.clone();
        let name = format!("{}{}", path.file_name().unwrap().to_string_lossy(), suffix);
        path.set_file_name(name);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Idempotent cleanup of any prior benchmark DB before a new run begins.
/// Called from the runner when the user confirms a benchmark, so a crashed
/// previous run does not feed stale data into Phase A.
#[tauri::command]
pub(crate) async fn prepare_benchmark_db(app: tauri::AppHandle) -> Result<(), String> {
    db::close_sqlite_pool(&app, &db::DatabaseTarget::Benchmark.url()).await?;
    delete_benchmark_db_files(&app)
}

/// Same operation as `prepare_benchmark_db`, exposed separately so callers
/// distinguish run-finished cleanup from run-starting cleanup.
#[tauri::command]
pub(crate) async fn teardown_benchmark_db(app: tauri::AppHandle) -> Result<(), String> {
    db::close_sqlite_pool(&app, &db::DatabaseTarget::Benchmark.url()).await?;
    delete_benchmark_db_files(&app)
}

#[tauri::command]
pub(crate) fn read_benchmark_state(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let path = benchmark_state_path(&app)?;
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(Some(contents))
}

#[tauri::command]
pub(crate) fn write_benchmark_state(app: tauri::AppHandle, json: String) -> Result<(), String> {
    let path = benchmark_state_path(&app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub(crate) fn clear_benchmark_state(app: tauri::AppHandle) -> Result<(), String> {
    let path = benchmark_state_path(&app)?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Benchmark state fields Rust checks before selecting the benchmark database.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BenchmarkRelaunchProbe {
    vault_mode: Option<String>,
    stage: Option<String>,
}

/// Stages written immediately before an intentional benchmark relaunch.
const BENCHMARK_PENDING_STAGES: &[&str] = &["phase-a-pending", "phase-b-pending"];

/// Whether persisted benchmark state names a benchmark relaunch that is waiting to run.
/// Version and freshness checks stay with the harness, which may still decline the
/// benchmark; this only bounds when the webview may select it.
fn state_names_pending_benchmark(json: &str) -> bool {
    serde_json::from_str::<BenchmarkRelaunchProbe>(json).is_ok_and(|probe| {
        probe.vault_mode.as_deref() == Some("benchmark")
            && probe
                .stage
                .as_deref()
                .is_some_and(|stage| BENCHMARK_PENDING_STAGES.contains(&stage))
    })
}

fn benchmark_relaunch_pending(app: &tauri::AppHandle) -> bool {
    let path = match benchmark_state_path(app) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("benchmark state path unavailable: {error}");
            return false;
        }
    };
    match std::fs::read_to_string(&path) {
        Ok(json) => state_names_pending_benchmark(&json),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            eprintln!("benchmark state unreadable: {error}");
            false
        }
    }
}

/// Selects the database every webview of this process uses and returns its URL. The first
/// call decides for the process lifetime; the benchmark database is chosen only when
/// requested and the persisted state names a pending benchmark relaunch.
#[tauri::command]
pub(crate) fn select_database_target(app: tauri::AppHandle, benchmark: bool) -> String {
    db::select_database_target(benchmark, || benchmark_relaunch_pending(&app)).url()
}

#[cfg(test)]
mod tests {
    use super::state_names_pending_benchmark;

    #[test]
    fn only_pending_benchmark_state_selects_the_benchmark_database() {
        for stage in ["phase-a-pending", "phase-b-pending"] {
            let json = format!(r#"{{"vaultMode":"benchmark","stage":"{stage}","extra":1}}"#);
            assert!(state_names_pending_benchmark(&json), "{stage}");
        }
        for json in [
            r#"{"vaultMode":"benchmark","stage":"phase-a-running"}"#,
            r#"{"vaultMode":"benchmark","stage":"phase-b-running"}"#,
            r#"{"vaultMode":"user","stage":"phase-a-pending"}"#,
            r#"{"stage":"phase-a-pending"}"#,
            r#"{"vaultMode":"benchmark"}"#,
            r#"{"vaultMode":"benchmark","stage":7}"#,
            "not json",
            "",
        ] {
            assert!(!state_names_pending_benchmark(json), "{json}");
        }
    }
}
