#![cfg(unix)]

use ganbaru_chat_providers::process::{ProviderProcessConfig, spawn_provider_process};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncReadExt;

struct FixtureDirectory(PathBuf);

impl FixtureDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ganbaru-chat-process-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn script(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, source).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
}

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Runs a fixture script through `/bin/sh` instead of executing it directly.
///
/// Concurrent tests that fork can briefly inherit the descriptor that wrote the
/// script, and executing a file that is still open for writing fails with
/// `ETXTBSY`. Reading the script as an interpreter argument avoids that race.
fn config(script: &Path, working_directory: &Path) -> ProviderProcessConfig {
    ProviderProcessConfig {
        executable: PathBuf::from("/bin/sh"),
        arguments: vec![script.to_string_lossy().into_owned()],
        working_directory: working_directory.to_path_buf(),
        environment: BTreeMap::new(),
        stderr_limit_bytes: 128,
    }
}

#[test]
fn provider_process_fixture_covers_normal_exit_and_malformed_protocol_bytes() {
    tauri::async_runtime::block_on(async {
        let fixture = FixtureDirectory::new();
        let normal = fixture.script("normal", "#!/bin/sh\nprintf '{\"status\":\"ok\"}\\n'\n");
        let mut process = spawn_provider_process(config(&normal, &fixture.0)).unwrap();
        let mut stdout = process.take_stdout().unwrap();
        let mut output = Vec::new();
        stdout.read_to_end(&mut output).await.unwrap();
        process
            .stop(Duration::from_secs(1), Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(output, b"{\"status\":\"ok\"}\n");

        let malformed = fixture.script("malformed", "#!/bin/sh\nprintf '{bad\\377'\n");
        let mut process = spawn_provider_process(config(&malformed, &fixture.0)).unwrap();
        let mut stdout = process.take_stdout().unwrap();
        let mut output = Vec::new();
        stdout.read_to_end(&mut output).await.unwrap();
        process
            .stop(Duration::from_secs(1), Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(output, b"{bad\xff");
    });
}

#[test]
fn provider_process_passes_literal_tokens_and_uses_only_explicit_environment() {
    tauri::async_runtime::block_on(async {
        let fixture = FixtureDirectory::new();
        let marker = fixture.0.join("must-not-exist");
        let script = fixture.script(
            "arguments",
            "#!/bin/sh\n[ -z \"${HOME+x}\" ] || exit 91\nprintf '%s\\n' \"${SAFE_VALUE:-missing}\" \"$1\"\n",
        );
        let mut request = config(&script, &fixture.0);
        request
            .environment
            .insert("SAFE_VALUE".to_string(), "present".to_string());
        request
            .arguments
            .push(format!("$(touch {})", marker.display()));
        let mut process = spawn_provider_process(request).unwrap();
        let mut stdout = process.take_stdout().unwrap();
        let mut output = String::new();
        stdout.read_to_string(&mut output).await.unwrap();
        process
            .stop(Duration::from_secs(1), Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(output, format!("present\n$(touch {})\n", marker.display()));
        assert!(!marker.exists());
    });
}

#[test]
fn provider_process_bounds_stderr_and_replaces_invalid_utf8() {
    tauri::async_runtime::block_on(async {
        let fixture = FixtureDirectory::new();
        let script = fixture.script(
            "stderr",
            "#!/bin/sh\ni=0\nwhile [ \"$i\" -lt 300 ]; do printf x >&2; i=$((i + 1)); done\nprintf '\\377tail' >&2\n",
        );
        let mut process = spawn_provider_process(config(&script, &fixture.0)).unwrap();
        process
            .stop(Duration::from_secs(1), Duration::from_secs(1))
            .await
            .unwrap();
        let diagnostic = process.diagnostic().unwrap();
        assert!(diagnostic.truncated);
        assert!(diagnostic.text.len() <= 130);
        assert!(diagnostic.text.ends_with("�tail"));
    });
}

#[test]
fn provider_process_force_stop_terminates_its_process_group_and_is_idempotent() {
    tauri::async_runtime::block_on(async {
        let fixture = FixtureDirectory::new();
        let script = fixture.script(
            "tree",
            "#!/bin/sh\ntrap '' TERM\n(trap '' TERM; while :; do sleep 1; done) &\necho $!\nwhile :; do sleep 1; done\n",
        );
        let mut process = spawn_provider_process(config(&script, &fixture.0)).unwrap();
        let mut stdout = process.take_stdout().unwrap();
        let mut line = Vec::new();
        loop {
            let mut byte = [0_u8; 1];
            stdout.read_exact(&mut byte).await.unwrap();
            if byte[0] == b'\n' {
                break;
            }
            line.push(byte[0]);
        }
        let descendant: i32 = String::from_utf8(line).unwrap().parse().unwrap();
        process
            .stop(Duration::from_millis(50), Duration::from_millis(100))
            .await
            .unwrap();
        process
            .stop(Duration::from_millis(1), Duration::from_millis(1))
            .await
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while process_is_running(descendant) && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            !process_is_running(descendant),
            "owned descendant must not survive forced group cleanup"
        );
    });
}

#[test]
fn provider_process_cleans_descendants_after_the_direct_child_exits_normally() {
    tauri::async_runtime::block_on(async {
        let fixture = FixtureDirectory::new();
        let script = fixture.script(
            "graceful-tree",
            "#!/bin/sh\n(trap '' TERM; while :; do sleep 1; done) &\necho $!\n",
        );
        let mut process = spawn_provider_process(config(&script, &fixture.0)).unwrap();
        let mut stdout = process.take_stdout().unwrap();
        let mut line = Vec::new();
        loop {
            let mut byte = [0_u8; 1];
            stdout.read_exact(&mut byte).await.unwrap();
            if byte[0] == b'\n' {
                break;
            }
            line.push(byte[0]);
        }
        let descendant: i32 = String::from_utf8(line).unwrap().parse().unwrap();
        process
            .stop(Duration::from_secs(1), Duration::from_secs(1))
            .await
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while process_is_running(descendant) && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            !process_is_running(descendant),
            "owned descendants must not survive normal provider exit"
        );
    });
}

#[test]
fn provider_process_drop_force_cleans_a_hung_descendant_tree() {
    tauri::async_runtime::block_on(async {
        let fixture = FixtureDirectory::new();
        let script = fixture.script(
            "drop-tree",
            "#!/bin/sh\ntrap '' TERM\n(trap '' TERM; while :; do sleep 1; done) &\necho $!\nwhile :; do sleep 1; done\n",
        );
        let mut process = spawn_provider_process(config(&script, &fixture.0)).unwrap();
        let mut stdout = process.take_stdout().unwrap();
        let mut line = Vec::new();
        loop {
            let mut byte = [0_u8; 1];
            stdout.read_exact(&mut byte).await.unwrap();
            if byte[0] == b'\n' {
                break;
            }
            line.push(byte[0]);
        }
        let descendant: i32 = String::from_utf8(line).unwrap().parse().unwrap();
        drop(process);
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while process_is_running(descendant) && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            !process_is_running(descendant),
            "owned descendant must not survive process-handle drop"
        );
    });
}

#[cfg(target_os = "linux")]
fn process_is_running(process_id: i32) -> bool {
    let Ok(stat) = fs::read_to_string(format!("/proc/{process_id}/stat")) else {
        return false;
    };
    stat.rsplit_once(") ")
        .and_then(|(_, suffix)| suffix.chars().next())
        .is_some_and(|state| state != 'Z')
}

#[cfg(not(target_os = "linux"))]
fn process_is_running(process_id: i32) -> bool {
    if process_id <= 0 {
        return false;
    }
    // SAFETY: A positive process ID addresses one process, and signal zero only
    // probes its existence and permissions without delivering a signal.
    let result = unsafe { libc::kill(process_id, 0) };
    let error = (result != 0)
        .then(std::io::Error::last_os_error)
        .and_then(|error| error.raw_os_error());
    process_probe_indicates_running(result, error)
}

#[cfg(any(not(target_os = "linux"), test))]
fn process_probe_indicates_running(result: i32, error: Option<i32>) -> bool {
    result == 0 || (result == -1 && error == Some(libc::EPERM))
}

#[cfg(test)]
mod process_probe_tests {
    use super::process_probe_indicates_running;

    #[test]
    fn permission_denied_still_means_the_process_exists() {
        assert!(process_probe_indicates_running(0, None));
        assert!(process_probe_indicates_running(-1, Some(libc::EPERM)));
        assert!(!process_probe_indicates_running(-1, Some(libc::ESRCH)));
    }
}
