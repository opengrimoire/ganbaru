use super::*;

#[cfg(target_os = "linux")]
pub(super) fn read_linux_process_names(path: &Path) -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(comm) = std::fs::read_to_string(path.join("comm")) {
        if let Some(name) = normalize_process_match_name(&comm) {
            names.push(name);
        }
    }
    if let Ok(cmdline) = std::fs::read(path.join("cmdline")) {
        if let Some(first_arg) = cmdline.split(|byte| *byte == 0).next() {
            if !first_arg.is_empty() {
                let command = String::from_utf8_lossy(first_arg);
                if let Some(name) = normalize_process_match_name(&command) {
                    names.push(name);
                }
            }
        }
    }
    names
}

#[cfg(target_os = "linux")]
fn linux_process_start_time(path: &Path) -> Option<String> {
    let stat = std::fs::read_to_string(path.join("stat")).ok()?;
    let command_end = stat.rfind(')')?;
    stat.get(command_end + 1..)?
        .split_whitespace()
        .nth(19)
        .map(ToOwned::to_owned)
}

#[cfg(target_os = "linux")]
fn update_process_identity_hash(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

#[cfg(target_os = "linux")]
pub(super) fn observe_linux_process(
    path: &Path,
    process_id: u32,
) -> Option<ObservedDesktopProcess> {
    let start_time = linux_process_start_time(path)?;
    let mut match_names = read_linux_process_names(path);
    match_names.sort_by_key(|name| app_name_key(name));
    match_names.dedup_by(|left, right| app_name_key(left) == app_name_key(right));
    let process_name = match_names.first()?.clone();
    let executable = std::fs::read_link(path.join("exe"))
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut hasher = Sha256::new();
    update_process_identity_hash(&mut hasher, &process_id.to_string());
    update_process_identity_hash(&mut hasher, &start_time);
    update_process_identity_hash(&mut hasher, &executable);
    for name in &match_names {
        update_process_identity_hash(&mut hasher, &app_name_key(name));
    }
    let process_identity = format!("{:x}", hasher.finalize());
    Some(ObservedDesktopProcess {
        process_name,
        match_names,
        process_identity,
    })
}

#[cfg(target_os = "linux")]
pub(super) fn list_blocked_desktop_app_matches(
    apps: Vec<DistractionsDesktopAppRuleInput>,
    is_cancelled: impl Fn() -> bool,
) -> Vec<DistractionsRunningDesktopAppMatch> {
    let matchers = desktop_rule_matchers(apps);
    if matchers.is_empty() {
        return Vec::new();
    }
    let current_process_id = std::process::id();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut matches = Vec::new();
    let mut seen_processes = HashSet::new();
    for entry in entries.flatten() {
        if is_cancelled() {
            return Vec::new();
        }
        let file_name = entry.file_name();
        let Some(pid_text) = file_name.to_str() else {
            continue;
        };
        let Ok(process_id) = pid_text.parse::<u32>() else {
            continue;
        };
        if process_id <= 1 || process_id == current_process_id {
            continue;
        }
        let Some(observed) = observe_linux_process(&entry.path(), process_id) else {
            continue;
        };
        for process_name in &observed.match_names {
            let key = app_name_key(process_name);
            let Some(rule_matcher) = matchers.get(&key) else {
                continue;
            };
            if !seen_processes.insert(process_id) {
                break;
            }
            matches.push(DistractionsRunningDesktopAppMatch {
                app_name: rule_matcher.app_name.clone(),
                process_name: process_name.clone(),
                process_id,
                process_identity: observed.process_identity.clone(),
                rule_identity: rule_matcher.rule_identity.clone(),
            });
            break;
        }
    }
    matches.sort_by(|a, b| {
        a.app_name
            .to_lowercase()
            .cmp(&b.app_name.to_lowercase())
            .then_with(|| a.process_id.cmp(&b.process_id))
    });
    matches
}

#[cfg(not(target_os = "linux"))]
pub(super) fn list_blocked_desktop_app_matches(
    apps: Vec<DistractionsDesktopAppRuleInput>,
    _is_cancelled: impl Fn() -> bool,
) -> Vec<DistractionsRunningDesktopAppMatch> {
    for app in apps {
        drop((app.rule_identity, app.name, app.match_names));
    }
    Vec::new()
}
