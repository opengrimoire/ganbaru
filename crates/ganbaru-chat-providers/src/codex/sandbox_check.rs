//! Device check that the Codex command sandbox can start.
//!
//! Codex runs organizational commands inside its own Linux sandbox, which needs unprivileged
//! user namespaces. Hosts that restrict them, for example through the AppArmor
//! `kernel.apparmor_restrict_unprivileged_userns` setting, fail every sandboxed command. The
//! check runs `codex sandbox` once with the organizational command profile, without opening a
//! model thread, and the result is kept for the rest of the app run.

use super::organizational::{
    CodexOrganizationalAccess, ORGANIZATIONAL_PERMISSION_PROFILE,
    append_organizational_base_arguments,
};
use crate::process::{BoundedCommand, run_bounded_command};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const SANDBOX_CHECK_TIMEOUT: Duration = Duration::from_secs(15);
const SANDBOX_CHECK_OUTPUT_LIMIT_BYTES: usize = 64 * 1024;
const SANDBOX_CHECK_DIRECTORY_PREFIX: &str = "ganbaru-codex-sandbox-check-";

/// The Codex installation and home a check result belongs to.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct CodexSandboxKey {
    executable: String,
    shared_home: PathBuf,
    effective_home: PathBuf,
}

impl CodexSandboxKey {
    pub(super) fn new(executable: &str, shared_home: &Path, effective_home: &Path) -> Self {
        Self {
            executable: executable.trim().to_string(),
            shared_home: shared_home.to_path_buf(),
            effective_home: effective_home.to_path_buf(),
        }
    }
}

fn sandbox_results() -> &'static Mutex<HashMap<CodexSandboxKey, bool>> {
    static RESULTS: OnceLock<Mutex<HashMap<CodexSandboxKey, bool>>> = OnceLock::new();
    RESULTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The recorded check result for `key`, or `None` before a check finished.
pub(super) fn cached_sandbox_result(key: &CodexSandboxKey) -> Option<bool> {
    sandbox_results()
        .lock()
        .ok()
        .and_then(|results| results.get(key).copied())
}

pub(super) fn record_sandbox_result(key: CodexSandboxKey, starts: bool) {
    if let Ok(mut results) = sandbox_results().lock() {
        results.insert(key, starts);
    }
}

/// Arguments that run `true` under the organizational command profile in `directory`, after
/// the `prefix` a resolved executable needs.
pub(super) fn sandbox_check_arguments(prefix: &[String], directory: &Path) -> Vec<String> {
    let mut arguments = prefix.to_vec();
    arguments.extend([
        "sandbox".to_string(),
        "-P".to_string(),
        ORGANIZATIONAL_PERMISSION_PROFILE.to_string(),
    ]);
    append_organizational_base_arguments(&mut arguments, CodexOrganizationalAccess::Commands);
    arguments.extend([
        "-C".to_string(),
        directory.to_string_lossy().into_owned(),
        "--".to_string(),
        "true".to_string(),
    ]);
    arguments
}

/// Whether `executable` can run a command in its sandbox with `environment`. Any failure to
/// start, a nonzero exit, oversized output, or a timeout counts as a sandbox that cannot start.
/// The check runs on Linux only, where the Codex sandbox depends on user namespaces; other
/// platforms keep the permission profile proof of the launch.
pub(super) async fn sandbox_starts(
    executable: &Path,
    prefix: &[String],
    environment: &BTreeMap<String, String>,
) -> bool {
    if cfg!(not(target_os = "linux")) {
        return true;
    }
    run_sandbox_check(executable, prefix, environment, SANDBOX_CHECK_TIMEOUT).await
}

async fn run_sandbox_check(
    executable: &Path,
    prefix: &[String],
    environment: &BTreeMap<String, String>,
    timeout: Duration,
) -> bool {
    let Ok(directory) =
        crate::probe_directory::ProbeDirectory::create_with_prefix(SANDBOX_CHECK_DIRECTORY_PREFIX)
    else {
        return false;
    };
    let arguments = sandbox_check_arguments(prefix, &directory);
    let arguments = arguments.iter().map(String::as_str).collect::<Vec<_>>();
    run_bounded_command(BoundedCommand {
        executable,
        arguments: &arguments,
        working_directory: &directory,
        environment,
        output_limit_bytes: SANDBOX_CHECK_OUTPUT_LIMIT_BYTES,
        timeout,
    })
    .await
    .is_ok_and(|output| output.successful)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_arguments_run_true_under_the_command_profile_in_the_directory() {
        let arguments =
            sandbox_check_arguments(&["/opt/codex.js".to_string()], Path::new("/tmp/check"));
        assert_eq!(
            arguments[..4],
            [
                "/opt/codex.js",
                "sandbox",
                "-P",
                ORGANIZATIONAL_PERMISSION_PROFILE
            ]
        );
        assert_eq!(
            arguments[arguments.len() - 4..],
            ["-C", "/tmp/check", "--", "true"]
        );
        let mut base = Vec::new();
        append_organizational_base_arguments(&mut base, CodexOrganizationalAccess::Commands);
        assert_eq!(arguments[4..arguments.len() - 4], base[..]);
        assert!(base.iter().any(|argument| {
            argument.starts_with(&format!("permissions.{ORGANIZATIONAL_PERMISSION_PROFILE}="))
        }));
    }

    #[test]
    fn results_are_kept_per_installation_and_home() {
        let first = CodexSandboxKey::new(" codex ", Path::new("/home/a"), Path::new("/home/a"));
        let other_home = CodexSandboxKey::new("codex", Path::new("/home/b"), Path::new("/home/b"));
        assert_eq!(cached_sandbox_result(&first), None);
        record_sandbox_result(first.clone(), false);
        assert_eq!(
            cached_sandbox_result(&CodexSandboxKey::new(
                "codex",
                Path::new("/home/a"),
                Path::new("/home/a")
            )),
            Some(false)
        );
        assert_eq!(cached_sandbox_result(&other_home), None);
    }

    #[cfg(unix)]
    mod scripted {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        fn script(directory: &Path, body: &str) -> PathBuf {
            let path = directory.join("codex");
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("script");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .expect("permissions");
            path
        }

        fn environment() -> BTreeMap<String, String> {
            BTreeMap::from([("PATH".to_string(), "/usr/bin:/bin".to_string())])
        }

        #[tokio::test]
        async fn a_successful_sandboxed_command_passes() {
            let directory = crate::probe_directory::ProbeDirectory::create().expect("directory");
            let executable = script(
                &directory,
                r#"[ "$1" = sandbox ] && [ "$2" = -P ] || exit 3"#,
            );
            assert!(
                run_sandbox_check(&executable, &[], &environment(), SANDBOX_CHECK_TIMEOUT).await
            );
        }

        #[tokio::test]
        async fn a_sandbox_that_cannot_start_fails() {
            let directory = crate::probe_directory::ProbeDirectory::create().expect("directory");
            let executable = script(
                &directory,
                "echo 'bwrap: setting up uid map: Permission denied' >&2; exit 1",
            );
            assert!(
                !run_sandbox_check(&executable, &[], &environment(), SANDBOX_CHECK_TIMEOUT).await
            );
        }

        #[tokio::test]
        async fn a_check_past_its_timeout_fails() {
            let directory = crate::probe_directory::ProbeDirectory::create().expect("directory");
            let executable = script(&directory, "sleep 5");
            assert!(
                !run_sandbox_check(&executable, &[], &environment(), Duration::from_millis(200))
                    .await
            );
        }

        #[tokio::test]
        async fn a_missing_executable_fails() {
            let directory = crate::probe_directory::ProbeDirectory::create().expect("directory");
            assert!(
                !run_sandbox_check(
                    &directory.join("missing"),
                    &[],
                    &environment(),
                    SANDBOX_CHECK_TIMEOUT
                )
                .await
            );
        }
    }
}
