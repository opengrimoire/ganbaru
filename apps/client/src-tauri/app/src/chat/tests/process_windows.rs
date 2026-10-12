use ganbaru_chat_providers::process::{ProviderProcessConfig, spawn_provider_process};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncReadExt;

const HELPER_MODE: &str = "GANBARU_CHAT_WINDOWS_HELPER_MODE";
const HELPER_MARKER: &str = "GANBARU_CHAT_WINDOWS_HELPER_MARKER";

struct FixtureDirectory(PathBuf);

impl FixtureDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ganbaru-chat-windows-process-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn config(mode: &str, working_directory: &Path) -> ProviderProcessConfig {
    let mut environment = BTreeMap::from([(HELPER_MODE.to_string(), mode.to_string())]);
    if let Some(command_shell) = std::env::var_os("ComSpec") {
        environment.insert(
            "ComSpec".to_string(),
            command_shell.to_string_lossy().into_owned(),
        );
    }
    ProviderProcessConfig {
        executable: std::env::current_exe().unwrap(),
        arguments: vec![
            "--exact".to_string(),
            "chat::tests::process_windows::provider_process_windows_helper".to_string(),
            "--nocapture".to_string(),
        ],
        working_directory: working_directory.to_path_buf(),
        environment,
        stderr_limit_bytes: 128,
    }
}

#[test]
fn provider_process_windows_helper() {
    let Ok(mode) = std::env::var(HELPER_MODE) else {
        return;
    };
    match mode.as_str() {
        "normal" => {
            std::io::stdout()
                .write_all(b"{\"status\":\"ok\"}\n")
                .unwrap();
            std::process::exit(0);
        }
        "malformed" => {
            std::io::stdout().write_all(b"{bad\xff").unwrap();
            std::process::exit(0);
        }
        "tree" | "tree-exit" => {
            let command_shell = std::env::var_os("ComSpec").unwrap();
            let child = std::process::Command::new(command_shell)
                .args(["/D", "/S", "/C", "ping -t 127.0.0.1 >NUL"])
                .spawn()
                .unwrap();
            fs::write(
                std::env::var_os(HELPER_MARKER).unwrap(),
                child.id().to_string(),
            )
            .unwrap();
            if mode == "tree-exit" {
                std::process::exit(0);
            }
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        _ => std::process::exit(2),
    }
}

#[test]
fn windows_job_handles_normal_and_malformed_provider_exit() {
    run_async(async {
        let fixture = FixtureDirectory::new();
        for (mode, expected) in [
            ("normal", b"{\"status\":\"ok\"}\n".as_slice()),
            ("malformed", b"{bad\xff".as_slice()),
        ] {
            let mut process = spawn_provider_process(config(mode, &fixture.0)).unwrap();
            let mut stdout = process.take_stdout().unwrap();
            let mut output = Vec::new();
            stdout.read_to_end(&mut output).await.unwrap();
            process
                .stop(Duration::from_secs(1), Duration::from_secs(1))
                .await
                .unwrap();
            assert_eq!(output, expected);
        }
    });
}

#[test]
fn windows_suspended_assignment_prevents_descendant_escape() {
    run_async(async {
        let fixture = FixtureDirectory::new();
        let marker = fixture.0.join("descendant.pid");
        let mut request = config("tree", &fixture.0);
        request.environment.insert(
            HELPER_MARKER.to_string(),
            marker.to_string_lossy().into_owned(),
        );
        let mut process = spawn_provider_process(request).unwrap();
        let marker_deadline = Instant::now() + Duration::from_secs(5);
        while !marker.exists() && Instant::now() < marker_deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let descendant: u32 = fs::read_to_string(&marker).unwrap().parse().unwrap();
        process
            .stop(Duration::from_millis(50), Duration::from_secs(1))
            .await
            .unwrap();
        process
            .stop(Duration::from_millis(1), Duration::from_millis(1))
            .await
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while process_is_running(descendant) && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            !process_is_running(descendant),
            "a descendant started at provider entry must remain inside the owned job"
        );
    });
}

#[test]
fn windows_job_cleans_descendants_after_normal_provider_exit() {
    run_async(async {
        let fixture = FixtureDirectory::new();
        let marker = fixture.0.join("graceful-descendant.pid");
        let mut request = config("tree-exit", &fixture.0);
        request.environment.insert(
            HELPER_MARKER.to_string(),
            marker.to_string_lossy().into_owned(),
        );
        let mut process = spawn_provider_process(request).unwrap();
        let marker_deadline = Instant::now() + Duration::from_secs(5);
        while !marker.exists() && Instant::now() < marker_deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let descendant: u32 = fs::read_to_string(&marker).unwrap().parse().unwrap();
        process
            .stop(Duration::from_secs(1), Duration::from_secs(1))
            .await
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while process_is_running(descendant) && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            !process_is_running(descendant),
            "owned descendants must not survive normal provider exit"
        );
    });
}

fn run_async(future: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(future);
}

fn process_is_running(process_id: u32) -> bool {
    use windows::Win32::Foundation::{
        ERROR_INVALID_PARAMETER, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };
    use windows::core::{HRESULT, Owned};

    // SAFETY: No pointers are passed, inheritance is disabled, and Windows
    // returns a fresh owning process handle on success.
    let process = match unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            false,
            process_id,
        )
    } {
        Ok(process) => process,
        Err(error) if error.code() == HRESULT::from_win32(ERROR_INVALID_PARAMETER.0) => {
            return false;
        }
        Err(error) => panic!("open provider process for state query: {error}"),
    };
    // SAFETY: `OpenProcess` returned a fresh owning handle above. Ownership is
    // transferred exactly once and released on every return or panic path.
    let process = unsafe { Owned::new(process) };
    // SAFETY: `process` owns a live handle with synchronization access. A zero
    // timeout only queries its current signaled state.
    let status = unsafe { WaitForSingleObject(*process, 0) };
    if status == WAIT_TIMEOUT {
        true
    } else if status == WAIT_OBJECT_0 {
        false
    } else if status == WAIT_FAILED {
        panic!(
            "query provider process state: {}",
            windows::core::Error::from_win32()
        );
    } else {
        panic!("unexpected provider process wait status: {}", status.0);
    }
}
