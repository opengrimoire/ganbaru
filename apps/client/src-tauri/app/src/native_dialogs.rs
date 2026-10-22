//! Runs blocking native file dialogs off the async runtime.

/// Waits for a native dialog on a blocking thread, so an open dialog never holds an async
/// runtime worker that other commands need.
pub(crate) async fn on_dialog_thread<T: Send + 'static>(
    dialog: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(dialog)
        .await
        .map_err(|error| format!("native dialog failed: {error}"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Calls that wait for the user to close a native dialog.
    const BLOCKING_DIALOG_CALLS: &[&str] = &[
        ".blocking_pick_file()",
        ".blocking_pick_files()",
        ".blocking_pick_folder()",
        ".blocking_pick_folders()",
        ".blocking_save_file()",
    ];
    /// Calls that move their closure onto a blocking thread.
    const BLOCKING_THREAD_CALLS: &[&str] = &["spawn_blocking(", "on_dialog_thread(", "blocking("];

    fn rust_sources(directory: &Path, sources: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).expect("read source directory") {
            let path = entry.expect("read source entry").path();
            if path.is_dir() {
                rust_sources(&path, sources);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(path);
            }
        }
    }

    /// Lists blocking dialog calls whose nearest enclosing function is `async` without a hop to
    /// a blocking thread in between.
    fn dialogs_on_async_workers(source: &str) -> Vec<usize> {
        let lines: Vec<&str> = source.lines().collect();
        let mut findings = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if !BLOCKING_DIALOG_CALLS.iter().any(|call| line.contains(call)) {
                continue;
            }
            let Some(start) = (0..=index).rev().find(|&row| {
                let trimmed = lines[row].trim_start();
                trimmed.contains("fn ") && !trimmed.starts_with("//")
            }) else {
                continue;
            };
            let header = lines[start];
            let hops = lines[start..=index]
                .iter()
                .any(|line| BLOCKING_THREAD_CALLS.iter().any(|call| line.contains(call)));
            let is_ios = start > 0
                && lines[start.saturating_sub(4)..start]
                    .iter()
                    .any(|line| line.contains("target_os = \"ios\""));
            if header.contains("async fn") && !hops && !is_ios {
                findings.push(index + 1);
            }
        }
        findings
    }

    #[test]
    fn the_scan_flags_a_dialog_awaited_on_an_async_worker() {
        let blocking = "pub async fn pick(app: AppHandle) {\n    app.dialog().file().blocking_pick_file();\n}\n";
        let moved = "pub async fn pick(app: AppHandle) {\n    on_dialog_thread(move || app.dialog().file().blocking_pick_file()).await;\n}\n";
        let synchronous =
            "fn pick(app: &AppHandle) {\n    app.dialog().file().blocking_save_file();\n}\n";

        assert_eq!(dialogs_on_async_workers(blocking), vec![2]);
        assert!(dialogs_on_async_workers(moved).is_empty());
        assert!(dialogs_on_async_workers(synchronous).is_empty());
    }

    #[test]
    fn no_async_command_waits_for_a_native_dialog_on_a_runtime_worker() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let scanner = root.join("native_dialogs.rs");
        let mut sources = Vec::new();
        rust_sources(&root, &mut sources);
        assert!(sources.contains(&scanner), "the scan reads the app sources");
        let mut findings = Vec::new();
        for path in sources.into_iter().filter(|path| *path != scanner) {
            let source = fs::read_to_string(&path).expect("read Rust source");
            for line in dialogs_on_async_workers(&source) {
                findings.push(format!("{}:{line}", path.display()));
            }
        }
        assert!(findings.is_empty(), "{findings:#?}");
    }
}
