//! Process memory sampling for the benchmark harness.

static PLATFORM_LABEL: std::sync::OnceLock<String> = std::sync::OnceLock::new();

#[derive(serde::Serialize, Clone)]
struct ProcessMemory {
    name: String,
    mb: f64,
}

#[derive(serde::Serialize, Clone)]
struct MemoryMetric {
    name: String,
    slug: String,
    description: String,
}

#[derive(serde::Serialize)]
pub(crate) struct MemoryReport {
    processes: Vec<ProcessMemory>,
    total_mb: f64,
    platform: String,
    metric: MemoryMetric,
}

fn platform_label() -> String {
    PLATFORM_LABEL.get_or_init(detect_platform_label).clone()
}

fn memory_metric(name: &str, slug: &str, description: &str) -> MemoryMetric {
    MemoryMetric {
        name: name.to_string(),
        slug: slug.to_string(),
        description: description.to_string(),
    }
}

#[cfg(target_os = "linux")]
fn detect_platform_label() -> String {
    if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
                let pretty = value.trim().trim_matches('"');
                if !pretty.is_empty() {
                    return format!("Linux {pretty}");
                }
            }
        }
    }
    "Linux".to_string()
}

#[cfg(target_os = "windows")]
fn detect_platform_label() -> String {
    if let Ok(output) = std::process::Command::new("cmd")
        .args(["/C", "ver"])
        .output()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        if let Some(version) = parse_windows_version(&text) {
            let major = windows_marketing_version(&version);
            return format!("{major} ({version})");
        }
    }
    "Windows".to_string()
}

#[cfg(target_os = "windows")]
fn parse_windows_version(text: &str) -> Option<String> {
    let start = text.find("Version ")? + "Version ".len();
    let rest = &text[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    let version = rest[..end].trim();
    if version.is_empty() {
        None
    } else {
        Some(version.to_string())
    }
}

#[cfg(target_os = "windows")]
fn windows_marketing_version(version: &str) -> &'static str {
    let build = version
        .split('.')
        .nth(2)
        .and_then(|part| part.parse::<u32>().ok())
        .unwrap_or(0);
    if build >= 22_000 {
        "Windows 11"
    } else {
        "Windows 10"
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn detect_platform_label() -> String {
    std::env::consts::OS.to_string()
}

#[tauri::command]
pub(crate) fn memory_report() -> MemoryReport {
    #[cfg(target_os = "linux")]
    {
        struct LinuxMemoryReading {
            kb: f64,
            is_pss: bool,
        }

        fn read_linux_memory_kb(pid: &str) -> LinuxMemoryReading {
            // smaps_rollup gives PSS (Proportional Set Size): private memory
            // plus a fair share of shared memory. Avoids double-counting shared
            // libraries across processes.
            let path = format!("/proc/{pid}/smaps_rollup");
            if let Ok(content) = std::fs::read_to_string(&path) {
                for line in content.lines() {
                    if let Some(value) = line.strip_prefix("Pss:") {
                        return LinuxMemoryReading {
                            kb: value
                                .trim()
                                .trim_end_matches(" kB")
                                .trim()
                                .parse::<f64>()
                                .unwrap_or(0.0),
                            is_pss: true,
                        };
                    }
                }
            }
            // Fallback to VmRSS if smaps_rollup is unavailable.
            let path = format!("/proc/{pid}/status");
            if let Ok(content) = std::fs::read_to_string(path) {
                for line in content.lines() {
                    if let Some(value) = line.strip_prefix("VmRSS:") {
                        return LinuxMemoryReading {
                            kb: value
                                .trim()
                                .trim_end_matches(" kB")
                                .trim()
                                .parse::<f64>()
                                .unwrap_or(0.0),
                            is_pss: false,
                        };
                    }
                }
            }
            LinuxMemoryReading {
                kb: 0.0,
                is_pss: false,
            }
        }

        fn process_label(pid: &str) -> String {
            let comm_path = format!("/proc/{pid}/comm");
            if let Ok(comm) = std::fs::read_to_string(&comm_path) {
                let comm = comm.trim();
                if comm.contains("Network") {
                    return "Network".to_string();
                }
                if comm.contains("WebKit") {
                    return "Frontend".to_string();
                }
                return comm.to_string();
            }
            format!("PID {pid}")
        }

        fn process_memory(name: String, pid: &str, all_pss: &mut bool) -> ProcessMemory {
            let reading = read_linux_memory_kb(pid);
            if !reading.is_pss {
                *all_pss = false;
            }
            ProcessMemory {
                name,
                mb: reading.kb / 1024.0,
            }
        }

        let my_pid = std::process::id();
        let my_pid_str = my_pid.to_string();
        let mut all_pss = true;
        let mut processes = vec![process_memory(
            "Backend".to_string(),
            &my_pid_str,
            &mut all_pss,
        )];

        // Walk /proc to find child processes (WebKitWebProcess, WebKitNetworkProcess, etc.)
        if let Ok(entries) = std::fs::read_dir("/proc") {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let entry_pid = file_name.to_string_lossy();
                if !entry_pid.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                if *entry_pid == *my_pid_str {
                    continue;
                }
                let stat_path = format!("/proc/{entry_pid}/stat");
                if let Ok(stat) = std::fs::read_to_string(&stat_path) {
                    // Format: pid (comm) state ppid ...
                    // Find closing ')' to skip comm which may contain spaces
                    if let Some(after_comm) = stat.rfind(')') {
                        let fields: Vec<&str> = stat[after_comm + 2..].split_whitespace().collect();
                        // fields[0] = state, fields[1] = ppid
                        if let Some(ppid) = fields.get(1) {
                            if *ppid == my_pid_str {
                                processes.push(process_memory(
                                    process_label(&entry_pid),
                                    &entry_pid,
                                    &mut all_pss,
                                ));
                            }
                        }
                    }
                }
            }
        }

        let total_mb = processes.iter().map(|p| p.mb).sum();
        processes[1..].sort_by(|a, b| b.mb.partial_cmp(&a.mb).unwrap_or(std::cmp::Ordering::Equal));

        MemoryReport {
            processes,
            total_mb,
            platform: platform_label(),
            metric: if all_pss {
                memory_metric(
                    "PSS",
                    "pss",
                    "Proportional Set Size, private memory plus a fair share of shared memory.",
                )
            } else {
                memory_metric(
                    "PSS/RSS",
                    "pss_rss",
                    "PSS where available, with RSS fallback for processes that cannot report PSS.",
                )
            },
        }
    }
    #[cfg(target_os = "windows")]
    {
        use std::mem::size_of;
        use windows::Win32::Foundation::ERROR_NO_MORE_FILES;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        };
        use windows::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
        };
        use windows::core::{HRESULT, Owned};

        let my_pid = std::process::id();

        let snapshot = {
            // SAFETY: The flags request a system-wide process snapshot and do
            // not require any caller-provided pointers. Windows returns a fresh
            // owning handle on success.
            let snapshot = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
                Ok(snapshot) => snapshot,
                Err(_) => {
                    return MemoryReport {
                        processes: vec![],
                        total_mb: 0.0,
                        platform: platform_label(),
                        metric: memory_metric(
                            "Working Set",
                            "working_set",
                            "Resident pages currently in physical memory. Shared pages may be counted per process.",
                        ),
                    };
                }
            };
            // SAFETY: The successful call above returned a fresh owning handle.
            // Ownership is transferred exactly once and released on drop.
            unsafe { Owned::new(snapshot) }
        };

        let mut process_entries: Vec<(u32, u32, String)> = Vec::new();
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        // SAFETY: `entry` is a valid PROCESSENTRY32W with dwSize initialized as
        // required by Process32FirstW and Process32NextW. `snapshot` owns a live
        // process snapshot for the duration of enumeration.
        match unsafe { Process32FirstW(*snapshot, &mut entry) } {
            Ok(()) => loop {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let exe = String::from_utf16_lossy(&entry.szExeFile[..end]);
                process_entries.push((entry.th32ProcessID, entry.th32ParentProcessID, exe));

                // SAFETY: The snapshot handle is still open and `entry` remains a
                // valid output buffer for the next process entry.
                entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
                match unsafe { Process32NextW(*snapshot, &mut entry) } {
                    Ok(()) => {}
                    Err(error) if error.code() == HRESULT::from_win32(ERROR_NO_MORE_FILES.0) => {
                        break;
                    }
                    Err(error) => {
                        eprintln!("Windows process snapshot enumeration failed: {error}");
                        break;
                    }
                }
            },
            Err(error) if error.code() == HRESULT::from_win32(ERROR_NO_MORE_FILES.0) => {}
            Err(error) => eprintln!("Windows process snapshot initialization failed: {error}"),
        }

        // Recursively collect all descendant PIDs (handles WebView2 grandchildren)
        let mut pids = vec![my_pid];
        let mut i = 0;
        while i < pids.len() {
            let parent = pids[i];
            for &(pid, ppid, _) in &process_entries {
                if ppid == parent && !pids.contains(&pid) {
                    pids.push(pid);
                }
            }
            i += 1;
        }

        let mut processes = Vec::new();
        let mut webview_index = 0u32;
        for &pid in &pids {
            // SAFETY: The process ID comes from the current process or the Windows
            // process snapshot. No pointers are passed, handle inheritance is
            // disabled, and Windows returns a fresh owning handle on success.
            if let Ok(handle) =
                unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) }
            {
                // SAFETY: `OpenProcess` returned a fresh owning handle above.
                // Ownership is transferred exactly once and released on drop.
                let handle = unsafe { Owned::new(handle) };
                let mut counters = PROCESS_MEMORY_COUNTERS {
                    cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                    ..Default::default()
                };
                // SAFETY: `handle` is an open process handle and `counters` is a valid
                // writable output buffer whose cb field matches its struct size.
                // Windows does not retain the handle or output pointer.
                if unsafe { GetProcessMemoryInfo(*handle, &mut counters, counters.cb) }.is_ok() {
                    let mb = counters.WorkingSetSize as f64 / (1024.0 * 1024.0);
                    let name = if pid == my_pid {
                        "Backend".to_string()
                    } else {
                        let exe = process_entries
                            .iter()
                            .find(|(p, _, _)| *p == pid)
                            .map(|(_, _, e)| e.as_str())
                            .unwrap_or("unknown");
                        if exe.contains("msedgewebview2") || exe.contains("WebView") {
                            webview_index += 1;
                            format!("WebView2 #{webview_index}")
                        } else {
                            exe.to_string()
                        }
                    };
                    processes.push(ProcessMemory { name, mb });
                }
            }
        }

        let total_mb = processes.iter().map(|p| p.mb).sum();
        if processes.len() > 1 {
            processes[1..]
                .sort_by(|a, b| b.mb.partial_cmp(&a.mb).unwrap_or(std::cmp::Ordering::Equal));
        }

        MemoryReport {
            processes,
            total_mb,
            platform: platform_label(),
            metric: memory_metric(
                "Working Set",
                "working_set",
                "Resident pages currently in physical memory. Shared pages may be counted per process.",
            ),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        MemoryReport {
            processes: vec![],
            total_mb: 0.0,
            platform: platform_label(),
            metric: memory_metric(
                "Unavailable",
                "unavailable",
                "Memory reporting is not implemented for this platform yet.",
            ),
        }
    }
}
