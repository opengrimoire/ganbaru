use super::{
    GitConfigEntry, GitConfigScope, GitFolderTrust, GitHardening, GitInheritedEnvironment,
    GitPolicy, GitRun, GitRunError, GitStdout, empty_hooks_directory, hardening_for,
    parse_config_listing, run_blocking,
};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const INTERNAL_TRUSTED: GitPolicy = GitPolicy::internal(GitFolderTrust::Trusted);
const INTERNAL_UNTRUSTED: GitPolicy = GitPolicy::internal(GitFolderTrust::Untrusted);
const USER_TRUSTED: GitPolicy = GitPolicy::user(GitFolderTrust::Trusted);
const USER_UNTRUSTED: GitPolicy = GitPolicy::user(GitFolderTrust::Untrusted);
const TEST_DEADLINE: Duration = Duration::from_secs(60);
const TEST_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

fn entry(scope: GitConfigScope, key: &str, value: Option<&str>) -> GitConfigEntry {
    GitConfigEntry {
        scope,
        key: key.to_string(),
        value: value.map(str::to_string),
    }
}

fn hooks() -> &'static Path {
    Path::new("/no-hooks")
}

fn config_value<'a>(hardening: &'a GitHardening, key: &str) -> Option<&'a str> {
    hardening
        .config
        .iter()
        .rev()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.as_str())
}

#[test]
fn parses_scoped_listing_with_valueless_keys_and_newlines_in_values() {
    let entries = parse_config_listing(
        b"global\0user.name\nTest User\0local\0core.bare\nfalse\0local\0core.fsmonitor\0\
          worktree\0alias.x\n!sh -c 'a\nb'\0",
    )
    .expect("listing should parse");
    assert_eq!(
        entries,
        vec![
            entry(GitConfigScope::Global, "user.name", Some("Test User")),
            entry(GitConfigScope::Local, "core.bare", Some("false")),
            entry(GitConfigScope::Local, "core.fsmonitor", None),
            entry(GitConfigScope::Worktree, "alias.x", Some("!sh -c 'a\nb'")),
        ]
    );
    assert_eq!(parse_config_listing(b""), Some(Vec::new()));
    assert_eq!(parse_config_listing(b"local\0"), None);
}

#[test]
fn internal_policy_disables_hooks_fsmonitor_signing_and_ext_transport() {
    let hardening = hardening_for(
        INTERNAL_UNTRUSTED,
        &[entry(
            GitConfigScope::Global,
            "core.fsmonitor",
            Some("true"),
        )],
        &GitInheritedEnvironment::default(),
        hooks(),
    );
    assert_eq!(config_value(&hardening, "core.fsmonitor"), Some(""));
    assert_eq!(
        config_value(&hardening, "core.hooksPath"),
        Some("/no-hooks")
    );
    assert_eq!(config_value(&hardening, "commit.gpgSign"), Some("false"));
    assert_eq!(config_value(&hardening, "tag.gpgSign"), Some("false"));
    assert_eq!(
        config_value(&hardening, "protocol.ext.allow"),
        Some("never")
    );
    assert!(
        hardening
            .cleared_environment
            .iter()
            .any(|name| name == "GIT_DIR")
    );
}

#[test]
fn repository_programs_fall_back_to_the_users_value_or_a_neutral_value() {
    let entries = [
        entry(
            GitConfigScope::Global,
            "diff.word.textconv",
            Some("wordconv"),
        ),
        entry(GitConfigScope::Local, "diff.word.textconv", Some("./evil")),
        entry(GitConfigScope::Local, "diff.Bin.textconv", Some("./evil")),
        entry(GitConfigScope::Local, "core.editor", Some("./evil")),
        entry(GitConfigScope::Local, "gpg.ssh.program", Some("./evil")),
        entry(
            GitConfigScope::Local,
            "remote.origin.uploadpack",
            Some("./evil"),
        ),
        entry(GitConfigScope::Local, "user.name", Some("Kept")),
    ];
    let hardening = hardening_for(
        USER_UNTRUSTED,
        &entries,
        &GitInheritedEnvironment::default(),
        hooks(),
    );
    assert_eq!(
        config_value(&hardening, "diff.word.textconv"),
        Some("wordconv")
    );
    assert_eq!(config_value(&hardening, "diff.Bin.textconv"), Some(""));
    assert_eq!(config_value(&hardening, "core.editor"), Some(":"));
    assert_eq!(
        config_value(&hardening, "gpg.ssh.program"),
        Some("ssh-keygen")
    );
    assert_eq!(
        config_value(&hardening, "remote.origin.uploadpack"),
        Some("git-upload-pack")
    );
    assert_eq!(config_value(&hardening, "user.name"), None);
}

#[test]
fn repository_filters_are_relaxed_but_standard_lfs_filters_stay() {
    let entries = [
        entry(
            GitConfigScope::Local,
            "filter.evil.clean",
            Some("./evil %f"),
        ),
        entry(GitConfigScope::Local, "filter.evil.required", Some("true")),
        entry(
            GitConfigScope::Local,
            "filter.lfs.clean",
            Some("git-lfs clean -- %f"),
        ),
        entry(
            GitConfigScope::Local,
            "filter.lfs.process",
            Some("git-lfs filter-process"),
        ),
        entry(GitConfigScope::Local, "filter.lfs.smudge", Some("./evil")),
    ];
    let hardening = hardening_for(
        INTERNAL_UNTRUSTED,
        &entries,
        &GitInheritedEnvironment::default(),
        hooks(),
    );
    assert_eq!(config_value(&hardening, "filter.evil.clean"), Some(""));
    assert_eq!(
        config_value(&hardening, "filter.evil.required"),
        Some("false")
    );
    assert_eq!(config_value(&hardening, "filter.lfs.clean"), None);
    assert_eq!(config_value(&hardening, "filter.lfs.process"), None);
    assert_eq!(config_value(&hardening, "filter.lfs.smudge"), Some(""));
    assert_eq!(
        config_value(&hardening, "filter.lfs.required"),
        Some("false")
    );
}

#[test]
fn repository_credential_helpers_are_replaced_by_the_users_ordered_helpers() {
    let entries = [
        entry(
            GitConfigScope::Global,
            "credential.https://github.com.helper",
            Some(""),
        ),
        entry(
            GitConfigScope::Global,
            "credential.https://github.com.helper",
            Some("!/usr/bin/gh auth git-credential"),
        ),
        entry(GitConfigScope::Global, "credential.helper", Some("cache")),
        entry(GitConfigScope::Local, "credential.helper", Some("!./evil")),
    ];
    let hardening = hardening_for(
        USER_UNTRUSTED,
        &entries,
        &GitInheritedEnvironment::default(),
        hooks(),
    );
    let reset = hardening
        .config
        .iter()
        .position(|(key, value)| key == "credential.helper" && value.is_empty())
        .expect("helpers should reset");
    assert_eq!(
        &hardening.config[reset + 1..],
        &[
            (
                "credential.https://github.com.helper".to_string(),
                String::new()
            ),
            (
                "credential.https://github.com.helper".to_string(),
                "!/usr/bin/gh auth git-credential".to_string()
            ),
            ("credential.helper".to_string(), "cache".to_string()),
        ]
    );
    let untouched = hardening_for(
        USER_UNTRUSTED,
        &entries[..3],
        &GitInheritedEnvironment::default(),
        hooks(),
    );
    assert!(
        !untouched
            .config
            .iter()
            .any(|(key, _)| key.starts_with("credential."))
    );
}

#[test]
fn repository_ssh_askpass_and_proxy_follow_the_users_environment() {
    let entries = [
        entry(GitConfigScope::Local, "core.sshCommand", Some("./evil")),
        entry(GitConfigScope::Local, "core.askPass", Some("./evil")),
        entry(GitConfigScope::Local, "core.gitProxy", Some("./evil")),
    ];
    let from_git_ssh = hardening_for(
        USER_UNTRUSTED,
        &entries,
        &GitInheritedEnvironment {
            git_ssh: Some("/usr/bin/my ssh".into()),
            ssh_askpass: Some("/usr/bin/ask".into()),
            ..GitInheritedEnvironment::default()
        },
        hooks(),
    );
    assert_eq!(config_value(&from_git_ssh, "core.sshCommand"), None);
    assert!(from_git_ssh.environment.contains(&(
        "GIT_SSH_COMMAND".to_string(),
        OsString::from("'/usr/bin/my ssh'")
    )));
    assert_eq!(
        config_value(&from_git_ssh, "core.askPass"),
        Some("/usr/bin/ask")
    );
    assert!(
        from_git_ssh
            .environment
            .contains(&("GIT_PROXY_COMMAND".to_string(), OsString::new()))
    );

    let from_command = hardening_for(
        USER_UNTRUSTED,
        &entries,
        &GitInheritedEnvironment {
            git_ssh_command: Some("ssh -i key".into()),
            git_askpass: Some("/usr/bin/ask".into()),
            git_proxy_command: Some("proxy".into()),
            ..GitInheritedEnvironment::default()
        },
        hooks(),
    );
    assert_eq!(config_value(&from_command, "core.sshCommand"), None);
    assert_eq!(config_value(&from_command, "core.askPass"), None);
    assert!(from_command.environment.is_empty());

    let neutral = hardening_for(
        USER_UNTRUSTED,
        &entries,
        &GitInheritedEnvironment::default(),
        hooks(),
    );
    assert_eq!(config_value(&neutral, "core.sshCommand"), Some("ssh"));
    assert_eq!(config_value(&neutral, "core.askPass"), Some(""));
}

#[test]
fn untrusted_policy_keeps_only_an_absolute_user_hooks_path() {
    let absolute = hardening_for(
        USER_UNTRUSTED,
        &[entry(
            GitConfigScope::Global,
            "core.hooksPath",
            Some("/home/u/hooks"),
        )],
        &GitInheritedEnvironment::default(),
        hooks(),
    );
    assert_eq!(
        config_value(&absolute, "core.hooksPath"),
        Some("/home/u/hooks")
    );
    let relative = hardening_for(
        USER_UNTRUSTED,
        &[
            entry(GitConfigScope::Global, "core.hooksPath", Some(".husky")),
            entry(GitConfigScope::Local, "core.hooksPath", Some(".evil")),
        ],
        &GitInheritedEnvironment::default(),
        hooks(),
    );
    assert_eq!(config_value(&relative, "core.hooksPath"), Some("/no-hooks"));
    assert_eq!(config_value(&relative, "core.fsmonitor"), None);
    assert_eq!(config_value(&relative, "commit.gpgSign"), None);
}

#[test]
fn trusted_policy_keeps_the_full_configuration() {
    let hardening = hardening_for(
        USER_TRUSTED,
        &[entry(
            GitConfigScope::Local,
            "core.fsmonitor",
            Some("./evil"),
        )],
        &GitInheritedEnvironment::default(),
        hooks(),
    );
    assert_eq!(
        hardening.config,
        vec![("protocol.ext.allow".to_string(), "never".to_string())]
    );
    assert!(hardening.cleared_environment.is_empty());
}

#[test]
fn internal_runs_in_trusted_folders_keep_repository_programs_but_not_hooks() {
    let hardening = hardening_for(
        INTERNAL_TRUSTED,
        &[
            entry(
                GitConfigScope::Local,
                "filter.crypt.clean",
                Some("crypt clean"),
            ),
            entry(GitConfigScope::Local, "core.hooksPath", Some(".hooks")),
        ],
        &GitInheritedEnvironment::default(),
        hooks(),
    );
    assert_eq!(config_value(&hardening, "filter.crypt.clean"), None);
    assert_eq!(
        config_value(&hardening, "core.hooksPath"),
        Some("/no-hooks")
    );
    assert_eq!(config_value(&hardening, "core.fsmonitor"), Some(""));
    assert_eq!(config_value(&hardening, "commit.gpgSign"), Some("false"));
    assert!(
        hardening
            .cleared_environment
            .iter()
            .any(|name| name == "GIT_SSH_COMMAND")
    );
}

#[test]
fn keys_with_equals_signs_move_every_pair_to_the_environment_in_order() {
    let hardening = GitHardening {
        config: vec![
            ("credential.helper".to_string(), String::new()),
            (
                "credential.https://x/?a=b.helper".to_string(),
                "store".to_string(),
            ),
        ],
        ..GitHardening::default()
    };
    let mut command = Command::new("git");
    hardening.apply(&mut command);
    assert_eq!(command.get_args().count(), 0);
    let environment = command
        .get_envs()
        .filter_map(|(name, value)| Some((name.to_str()?.to_string(), value?.to_owned())))
        .collect::<Vec<_>>();
    let value = |name: &str| {
        environment
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value.clone())
    };
    assert_eq!(value("GIT_CONFIG_COUNT"), Some("2".into()));
    assert_eq!(value("GIT_CONFIG_KEY_0"), Some("credential.helper".into()));
    assert_eq!(
        value("GIT_CONFIG_KEY_1"),
        Some("credential.https://x/?a=b.helper".into())
    );
    let child = hardening.child_environment(3);
    assert!(child.contains(&("GIT_CONFIG_COUNT".to_string(), "5".into())));
    assert!(child.contains(&("GIT_CONFIG_KEY_3".to_string(), "credential.helper".into())));
}

struct TestRepository {
    root: PathBuf,
    global: PathBuf,
}

impl TestRepository {
    fn new() -> Self {
        let unique = format!(
            "ganbaru-git-hardened-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should follow Unix epoch")
                .as_nanos()
        );
        let base = std::env::temp_dir().join(unique);
        let root = base.join("repo");
        fs::create_dir_all(&root).expect("repository directory should be created");
        let global = base.join("global.gitconfig");
        fs::write(
            &global,
            "[user]\n\tname = Ganbaru test\n\temail = test@example.invalid\n\
             [commit]\n\tgpgSign = false\n[init]\n\tdefaultBranch = main\n",
        )
        .expect("global config should write");
        let repository = Self { root, global };
        repository.git(&["init", "--quiet"]);
        repository
    }

    fn base(&self) -> &Path {
        self.root.parent().expect("repository should have a parent")
    }

    fn environment(&self) -> Vec<(&'static str, &OsStr)> {
        vec![
            ("GIT_CONFIG_GLOBAL", self.global.as_os_str()),
            ("GIT_CONFIG_NOSYSTEM", OsStr::new("1")),
        ]
    }

    /// Runs Git the way a user's terminal would, with no hardening.
    fn git(&self, arguments: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(arguments)
            .env("GIT_CONFIG_GLOBAL", &self.global)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .expect("git should start");
        assert!(status.success(), "git {arguments:?} should succeed");
    }

    fn run(
        &self,
        policy: GitPolicy,
        arguments: &[&str],
        input: Option<&[u8]>,
        deadline: Duration,
    ) -> Result<super::GitRunOutput, GitRunError> {
        let arguments = arguments.iter().map(OsStr::new).collect::<Vec<_>>();
        let environment = self.environment();
        run_blocking(GitRun {
            root: &self.root,
            arguments: &arguments,
            policy,
            deadline,
            input,
            stdout: GitStdout::Memory(TEST_OUTPUT_BYTES),
            environment: &environment,
            cancel: None,
        })
    }

    fn marker(&self) -> PathBuf {
        self.base().join("marker")
    }

    #[cfg(unix)]
    fn write_script(&self, relative: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("script parent should be created");
        }
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("script should write");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("script should be executable");
        path
    }
}

impl Drop for TestRepository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.base());
    }
}

#[cfg(unix)]
#[test]
fn internal_runs_do_not_execute_repository_programs() {
    let repository = TestRepository::new();
    let marker = repository.marker();
    let record = format!("echo ran >> '{}'", marker.display());
    let fsmonitor = repository.write_script("tools/fsmonitor", &record);
    let filter = repository.write_script("tools/filter", &format!("{record}\ncat"));
    repository.write_script("tools/hooks/pre-commit", &record);
    repository.write_script("tools/hooks/post-commit", &record);
    repository.git(&["config", "core.fsmonitor", &fsmonitor.to_string_lossy()]);
    repository.git(&["config", "filter.evil.clean", &filter.to_string_lossy()]);
    repository.git(&["config", "filter.evil.smudge", &filter.to_string_lossy()]);
    repository.git(&["config", "filter.evil.required", "true"]);
    repository.git(&["config", "core.hooksPath", "tools/hooks"]);
    fs::write(
        repository.root.join(".gitattributes"),
        "*.txt filter=evil\n",
    )
    .expect("attributes should write");
    fs::write(repository.root.join("file.txt"), "content\n").expect("file should write");
    fs::write(repository.root.join(".gitignore"), "ignored/\n").expect("ignore should write");

    for arguments in [
        &["status", "--porcelain=v2", "-z"][..],
        &["add", "-A"],
        &["commit", "-q", "-m", "test: hardened"],
        &["ls-files", "-co", "--exclude-standard", "-z"],
        &["diff", "HEAD"],
    ] {
        let output = repository
            .run(INTERNAL_UNTRUSTED, arguments, None, TEST_DEADLINE)
            .unwrap_or_else(|error| panic!("git {arguments:?} should run: {error:?}"));
        assert!(
            output.status.success(),
            "git {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fs::remove_file(repository.root.join("file.txt")).expect("file should be removed");
    let output = repository
        .run(
            INTERNAL_UNTRUSTED,
            &["restore", "--", "file.txt"],
            None,
            TEST_DEADLINE,
        )
        .expect("restore should run");
    assert!(output.status.success());
    let output = repository
        .run(
            INTERNAL_UNTRUSTED,
            &["check-ignore", "--stdin", "-z"],
            Some(b"ignored/a\0file.txt\0"),
            TEST_DEADLINE,
        )
        .expect("check-ignore should run");
    assert_eq!(output.stdout, b"ignored/a\0");
    assert!(!marker.exists(), "a repository program ran");

    repository.git(&["status", "--porcelain"]);
    assert!(
        marker.exists(),
        "the fixture should run programs outside Ganbaru"
    );
}

#[cfg(unix)]
#[test]
fn internal_runs_keep_the_users_global_configuration() {
    let repository = TestRepository::new();
    let excludes = repository.base().join("excludes");
    fs::write(&excludes, "*.secret\n").expect("excludes should write");
    let mut global = fs::read_to_string(&repository.global).expect("global should read");
    global.push_str(&format!(
        "[core]\n\texcludesFile = {}\n\tautocrlf = input\n\
         [filter \"upper\"]\n\tclean = tr a-z A-Z\n\tsmudge = cat\n",
        excludes.display()
    ));
    fs::write(&repository.global, global).expect("global should write");
    fs::write(
        repository.root.join(".gitattributes"),
        "*.up filter=upper\n",
    )
    .expect("attributes should write");
    fs::write(repository.root.join("a.secret"), "x\n").expect("file should write");
    fs::write(repository.root.join("b.up"), "lower\n").expect("file should write");
    fs::write(repository.root.join("c.txt"), "line\r\n").expect("file should write");

    let listed = repository
        .run(
            INTERNAL_UNTRUSTED,
            &["ls-files", "-co", "--exclude-standard", "-z"],
            None,
            TEST_DEADLINE,
        )
        .expect("ls-files should run");
    assert_eq!(listed.stdout, b".gitattributes\0b.up\0c.txt\0");
    for arguments in [&["add", "-A"][..], &["commit", "-q", "-m", "test: global"]] {
        let output = repository
            .run(INTERNAL_UNTRUSTED, arguments, None, TEST_DEADLINE)
            .expect("git should run");
        assert!(output.status.success());
    }
    let upper = repository
        .run(
            INTERNAL_UNTRUSTED,
            &["show", "HEAD:b.up"],
            None,
            TEST_DEADLINE,
        )
        .expect("show should run");
    assert_eq!(upper.stdout, b"LOWER\n");
    let normalized = repository
        .run(
            INTERNAL_UNTRUSTED,
            &["show", "HEAD:c.txt"],
            None,
            TEST_DEADLINE,
        )
        .expect("show should run");
    assert_eq!(normalized.stdout, b"line\n");
}

#[cfg(unix)]
#[test]
fn trusted_runs_execute_hooks_and_untrusted_runs_do_not() {
    let repository = TestRepository::new();
    let marker = repository.marker();
    repository.write_script(
        ".git/hooks/pre-commit",
        &format!("echo ran >> '{}'", marker.display()),
    );
    fs::write(repository.root.join("file.txt"), "content\n").expect("file should write");
    repository.git(&["add", "file.txt"]);
    let untrusted = repository
        .run(
            USER_UNTRUSTED,
            &["commit", "-q", "-m", "test: untrusted"],
            None,
            TEST_DEADLINE,
        )
        .expect("commit should run");
    assert!(untrusted.status.success());
    assert!(!marker.exists());
    fs::write(repository.root.join("file.txt"), "changed\n").expect("file should write");
    repository.git(&["add", "file.txt"]);
    let trusted = repository
        .run(
            USER_TRUSTED,
            &["commit", "-q", "-m", "test: trusted"],
            None,
            TEST_DEADLINE,
        )
        .expect("commit should run");
    assert!(trusted.status.success());
    assert!(marker.exists());
}

#[cfg(unix)]
#[test]
fn deadline_stops_the_whole_process_tree() {
    let repository = TestRepository::new();
    let pid_file = repository.base().join("grandchild.pid");
    repository.write_script(
        ".git/hooks/pre-commit",
        &format!("sleep 30 &\necho $! > '{}'\nsleep 30", pid_file.display()),
    );
    fs::write(repository.root.join("file.txt"), "content\n").expect("file should write");
    repository.git(&["add", "file.txt"]);
    let started = std::time::Instant::now();
    let result = repository.run(
        USER_TRUSTED,
        &["commit", "-q", "-m", "test: slow hook"],
        None,
        Duration::from_secs(2),
    );
    assert!(matches!(result, Err(GitRunError::TimedOut)), "{result:?}");
    assert!(started.elapsed() < Duration::from_secs(20));
    let pid = fs::read_to_string(&pid_file)
        .expect("hook should record its child")
        .trim()
        .to_string();
    let mut alive = true;
    for _ in 0..100 {
        alive = Command::new("kill")
            .args(["-0", &pid])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if !alive {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!alive, "the hook's background child survived the deadline");
}

#[test]
fn check_ignore_streams_large_inputs_without_deadlock() {
    let repository = TestRepository::new();
    fs::write(repository.root.join(".gitignore"), "*.log\n").expect("ignore should write");
    let segment = "d".repeat(200);
    let mut input = Vec::new();
    for index in 0..10_000 {
        input.extend_from_slice(format!("{segment}/{index}.log\0").as_bytes());
    }
    let output = repository
        .run(
            INTERNAL_UNTRUSTED,
            &["check-ignore", "--stdin", "-z"],
            Some(&input),
            TEST_DEADLINE,
        )
        .expect("check-ignore should complete");
    assert_eq!(output.stdout.len(), input.len());
}

#[test]
fn output_bound_stops_the_run() {
    let repository = TestRepository::new();
    fs::write(repository.root.join("big.txt"), vec![b'x'; 64 * 1024]).expect("file should write");
    let arguments = ["hash-object", "--stdin"].map(OsStr::new);
    let environment = repository.environment();
    let input = vec![b'y'; 1024];
    repository.git(&["add", "big.txt"]);
    let result = run_blocking(GitRun {
        root: &repository.root,
        arguments: &[OsStr::new("show"), OsStr::new(":big.txt")],
        policy: INTERNAL_UNTRUSTED,
        deadline: TEST_DEADLINE,
        input: None,
        stdout: GitStdout::Memory(16),
        environment: &environment,
        cancel: None,
    });
    assert!(
        matches!(result, Err(GitRunError::OutputTooLarge)),
        "{result:?}"
    );
    let hashed = run_blocking(GitRun {
        root: &repository.root,
        arguments: &arguments,
        policy: INTERNAL_UNTRUSTED,
        deadline: TEST_DEADLINE,
        input: Some(&input),
        stdout: GitStdout::Memory(TEST_OUTPUT_BYTES),
        environment: &environment,
        cancel: None,
    })
    .expect("hash-object should run");
    assert!(hashed.status.success());
}

#[cfg(unix)]
#[test]
fn empty_hooks_directory_never_contains_hooks() {
    let directory = empty_hooks_directory();
    assert!(fs::read_dir(directory).is_err());
}

#[cfg(unix)]
#[test]
fn internal_runs_in_trusted_folders_match_the_terminal_without_hooks() {
    let repository = TestRepository::new();
    let marker = repository.marker();
    repository.write_script(
        ".git/hooks/pre-commit",
        &format!("echo ran >> '{}'", marker.display()),
    );
    repository.git(&["config", "filter.upper.clean", "tr a-z A-Z"]);
    repository.git(&["config", "filter.upper.smudge", "cat"]);
    fs::write(
        repository.root.join(".gitattributes"),
        "*.up filter=upper\n",
    )
    .expect("attributes should write");
    fs::write(repository.root.join("b.up"), "lower\n").expect("file should write");
    for arguments in [&["add", "-A"][..], &["commit", "-q", "-m", "test: trusted"]] {
        let output = repository
            .run(INTERNAL_TRUSTED, arguments, None, TEST_DEADLINE)
            .expect("git should run");
        assert!(output.status.success());
    }
    let upper = repository
        .run(
            INTERNAL_TRUSTED,
            &["show", "HEAD:b.up"],
            None,
            TEST_DEADLINE,
        )
        .expect("show should run");
    assert_eq!(upper.stdout, b"LOWER\n");
    assert!(!marker.exists());
}
