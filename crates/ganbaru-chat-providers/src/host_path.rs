//! The `PATH` that provider discovery searches and that provider programs inherit.
//!
//! A running process keeps the environment it started with, while installers on Windows add
//! their folders to the user or machine `PATH` setting in the registry for programs started
//! later. On Windows this module therefore reads those settings again on every call and
//! appends the folders they gained after Ganbaru started, so a provider installed while the
//! app runs, or a tool it needs such as Node.js, is found without a restart. Folders from the
//! launch `PATH` keep their order and precedence. Other platforms use the launch `PATH` as is,
//! because their only equivalent is running the person's login shell and its startup files.

use std::ffi::OsString;

/// The `PATH` to search for provider programs and to hand to them.
pub fn search_path() -> Option<OsString> {
    #[cfg(windows)]
    {
        let launch = std::env::var("PATH").unwrap_or_default();
        let merged = append_new_entries(&launch, &settings_path_entries());
        (!merged.is_empty()).then(|| OsString::from(merged))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("PATH")
    }
}

/// A host environment variable to copy into a provider's environment, with `PATH` from
/// [`search_path`] and every other name read from this process as is.
pub fn host_variable(name: &str) -> Option<String> {
    if name.eq_ignore_ascii_case("PATH") {
        return search_path().and_then(|value| value.into_string().ok());
    }
    std::env::var(name).ok()
}

/// The absolute folders of the machine and then the user `PATH` settings, in the order a newly
/// started Windows program sees them. A setting that is missing or unreadable adds nothing, which
/// leaves the launch `PATH` as the whole search.
#[cfg(windows)]
fn settings_path_entries() -> Vec<String> {
    const MACHINE_ENVIRONMENT: &str =
        r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";
    let machine = windows_registry::LOCAL_MACHINE
        .open(MACHINE_ENVIRONMENT)
        .and_then(|key| key.get_string("Path"));
    let user = windows_registry::CURRENT_USER
        .open("Environment")
        .and_then(|key| key.get_string("Path"));
    [machine, user]
        .into_iter()
        .filter_map(Result::ok)
        .flat_map(|value| path_setting_entries(&value, |name| std::env::var(name).ok()))
        .collect()
}

/// The absolute folders of one `PATH` setting, with `%NAME%` references expanded.
#[cfg(any(windows, test))]
fn path_setting_entries(value: &str, lookup: impl Fn(&str) -> Option<String>) -> Vec<String> {
    value
        .split(';')
        .map(|entry| expand_references(entry.trim(), &lookup))
        .filter(|entry| is_absolute_windows_path(entry))
        .collect()
}

/// Expands `%NAME%` references the way Windows expands an expandable registry string: a name
/// that `lookup` does not know stays as written.
#[cfg(any(windows, test))]
fn expand_references(value: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut expanded = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        expanded.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            expanded.push_str(&rest[start..]);
            return expanded;
        };
        let name = &after[..end];
        match (!name.is_empty()).then(|| lookup(name)).flatten() {
            Some(replacement) => expanded.push_str(&replacement),
            None => expanded.push_str(&rest[start..start + end + 2]),
        }
        rest = &after[end + 1..];
    }
    expanded.push_str(rest);
    expanded
}

/// Whether `entry` names a drive or network folder rather than one relative to the current
/// directory.
#[cfg(any(windows, test))]
fn is_absolute_windows_path(entry: &str) -> bool {
    let bytes = entry.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    drive || entry.starts_with(r"\\")
}

/// `launch` followed by each entry of `added` that it does not already name, compared the way
/// Windows compares folders: without case and without a trailing separator.
#[cfg(any(windows, test))]
fn append_new_entries(launch: &str, added: &[String]) -> String {
    let key = |entry: &str| entry.trim().trim_end_matches(['\\', '/']).to_lowercase();
    let mut seen = launch
        .split(';')
        .map(key)
        .filter(|entry| !entry.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    let mut merged = launch.trim_end_matches(';').to_string();
    for entry in added {
        if !seen.insert(key(entry)) {
            continue;
        }
        if !merged.is_empty() {
            merged.push(';');
        }
        merged.push_str(entry);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(name: &str) -> Option<String> {
        match name.to_ascii_uppercase().as_str() {
            "USERPROFILE" => Some(r"C:\Users\Person".to_string()),
            "SYSTEMROOT" => Some(r"C:\Windows".to_string()),
            _ => None,
        }
    }

    #[test]
    fn references_expand_and_unknown_names_stay_as_written() {
        assert_eq!(
            expand_references(r"%USERPROFILE%\.grok\bin", lookup),
            r"C:\Users\Person\.grok\bin"
        );
        assert_eq!(
            expand_references(r"%SystemRoot%\system32", lookup),
            r"C:\Windows\system32"
        );
        assert_eq!(
            expand_references(r"%MISSING%\bin", lookup),
            r"%MISSING%\bin"
        );
        assert_eq!(expand_references(r"C:\100%\bin", lookup), r"C:\100%\bin");
        assert_eq!(expand_references("%%", lookup), "%%");
        assert_eq!(
            expand_references(r"%MISSING%%USERPROFILE%", lookup),
            r"%MISSING%C:\Users\Person"
        );
    }

    #[test]
    fn setting_entries_keep_only_absolute_folders() {
        let entries = path_setting_entries(
            r"%SystemRoot%\system32; ;relative\bin;\\server\tools;%MISSING%\bin;C:/Node",
            lookup,
        );
        assert_eq!(
            entries,
            [r"C:\Windows\system32", r"\\server\tools", "C:/Node"]
        );
    }

    #[test]
    fn new_folders_follow_the_launch_path_once_each() {
        let added = [
            r"c:\windows\system32\".to_string(),
            r"C:\Program Files\nodejs".to_string(),
            r"C:\Users\Person\.grok\bin".to_string(),
            r"C:\PROGRAM FILES\NODEJS".to_string(),
        ];
        assert_eq!(
            append_new_entries(r"C:\Windows\system32;C:\Tools;", &added),
            r"C:\Windows\system32;C:\Tools;C:\Program Files\nodejs;C:\Users\Person\.grok\bin"
        );
        assert_eq!(
            append_new_entries("", &added[1..2]),
            r"C:\Program Files\nodejs"
        );
        assert_eq!(append_new_entries(r"C:\Tools", &[]), r"C:\Tools");
    }

    #[test]
    fn host_variables_other_than_path_come_from_this_process() {
        assert_eq!(host_variable("GANBARU_HOST_PATH_UNSET_VARIABLE"), None);
    }
}
