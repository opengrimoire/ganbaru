use super::super::{GitCall, GitFolder, commit, initialize, run, stage};
use super::{
    BranchTracking, GitPushTarget, GitPushTargetError, is_local_push_url, is_object_id,
    is_supported_push_url, push_object, push_target,
};
use std::fs;
use std::path::{Path, PathBuf};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let unique = format!(
            "ganbaru-git-push-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should follow Unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir(&path).expect("temporary directory should be created");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn text(&self) -> &str {
        self.0.to_str().expect("temporary path should be UTF-8")
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A repository on `main` with one commit and a bare remote named `origin`.
struct PushFixture {
    work: TestDirectory,
    remote: TestDirectory,
}

impl PushFixture {
    async fn new() -> Self {
        let fixture = Self {
            work: TestDirectory::new("work"),
            remote: TestDirectory::new("remote"),
        };
        run(
            GitFolder::untrusted(fixture.remote.path()),
            &["init", "--bare", "--quiet"],
            GitCall::User,
        )
        .await
        .expect("remote should initialize");
        let folder = fixture.folder();
        initialize(folder, Some("main"))
            .await
            .expect("repository should initialize");
        for (key, value) in [
            ("user.name", "Ganbaru test"),
            ("user.email", "test@example.invalid"),
            ("commit.gpgsign", "false"),
        ] {
            fixture.git(&["config", key, value]).await;
        }
        fixture
            .git(&["remote", "add", "origin", fixture.remote.text()])
            .await;
        fixture.commit_file("one.txt", "test: first").await;
        fixture
    }

    fn folder(&self) -> GitFolder<'_> {
        GitFolder::untrusted(self.work.path())
    }

    async fn git(&self, arguments: &[&str]) -> String {
        let output = run(self.folder(), arguments, GitCall::User)
            .await
            .unwrap_or_else(|error| panic!("git {arguments:?} should succeed: {error:?}"));
        String::from_utf8(output.stdout).expect("output should be UTF-8")
    }

    async fn commit_file(&self, name: &str, message: &str) {
        fs::write(self.work.path().join(name), message).expect("file should write");
        stage(self.folder(), &[name.to_string()])
            .await
            .expect("file should stage");
        commit(self.folder(), message)
            .await
            .expect("commit should succeed");
    }

    async fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"]).await.trim().to_string()
    }

    async fn remote_branch(&self, branch: &str) -> Option<String> {
        let reference = format!("refs/heads/{branch}");
        run(
            GitFolder::untrusted(self.remote.path()),
            &["rev-parse", "--verify", "--quiet", &reference],
            GitCall::Internal,
        )
        .await
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

#[test]
fn object_ids_are_full_lowercase_hexadecimal() {
    assert!(is_object_id(&"a".repeat(40)));
    assert!(is_object_id(&"0".repeat(64)));
    assert!(!is_object_id(&"a".repeat(39)));
    assert!(!is_object_id(&"A".repeat(40)));
    assert!(!is_object_id(&format!("{}g", "a".repeat(39))));
    assert!(!is_object_id("HEAD"));
}

#[test]
fn push_urls_with_a_password_or_control_characters_are_unsupported() {
    for url in [
        "https://github.com/org/repo.git",
        "ssh://git@github.com/org/repo.git",
        "git@github.com:org/repo.git",
        "https://user@example.com/repo.git",
        "/srv/git/repo.git",
        "https://example.com/a@b/repo.git",
    ] {
        assert!(is_supported_push_url(url), "{url}");
    }
    for url in [
        "https://user:token@github.com/org/repo.git",
        "https://:token@github.com/org/repo.git",
        "ssh://git:secret@host/repo.git",
        "https://example.com/repo .git",
        "https://example.com/\nrepo.git",
        "",
    ] {
        assert!(!is_supported_push_url(url), "{url:?}");
    }
    assert!(!is_supported_push_url(&format!(
        "https://example.com/{}",
        "a".repeat(2_048)
    )));
}

#[test]
fn branch_tracking_prefers_the_push_remote_then_the_upstream() {
    let both = BranchTracking::parse("fork\0refs/heads/topic\0origin\0refs/heads/main\n")
        .expect("four fields should parse");
    assert_eq!(
        both.default_destination(),
        Some(("fork".to_string(), Some("refs/heads/topic".to_string())))
    );
    assert_eq!(
        both.destination_on("origin"),
        Some("refs/heads/main".to_string())
    );
    assert_eq!(both.destination_on("other"), None);

    let upstream_only = BranchTracking::parse("\0\0origin\0refs/heads/main").expect("parse");
    assert_eq!(
        upstream_only.default_destination(),
        Some(("origin".to_string(), Some("refs/heads/main".to_string())))
    );
    let unresolved_push =
        BranchTracking::parse("origin\0\0origin\0refs/heads/trunk").expect("parse");
    assert_eq!(
        unresolved_push.default_destination(),
        Some(("origin".to_string(), Some("refs/heads/trunk".to_string())))
    );
    assert_eq!(BranchTracking::default().default_destination(), None);
    assert!(BranchTracking::parse("a\0b\0c").is_none());
    assert!(BranchTracking::parse("a\0b\0c\0d\0e").is_none());
}

#[tokio::test]
async fn target_names_the_upstream_branch_and_the_rewritten_push_url() {
    let fixture = PushFixture::new().await;
    fixture
        .git(&["push", "--quiet", "origin", "main:trunk"])
        .await;
    fixture
        .git(&["branch", "--set-upstream-to=origin/trunk", "main"])
        .await;
    let target = push_target(fixture.folder(), None)
        .await
        .expect("an upstream branch should have a target");
    assert_eq!(
        target,
        GitPushTarget {
            local_branch: "main".to_string(),
            remote: "origin".to_string(),
            destination_branch: "trunk".to_string(),
            push_url: fixture.remote.text().to_string(),
            object_id: fixture.head().await,
        }
    );

    let mirror = format!("{}/mirror.git", fixture.work.text());
    let rewrite = format!("url.{mirror}.pushInsteadOf");
    fixture
        .git(&["config", &rewrite, fixture.remote.text()])
        .await;
    let rewritten = push_target(fixture.folder(), Some("origin"))
        .await
        .expect("a named remote should have a target");
    assert_eq!(rewritten.push_url, mirror);
    assert_eq!(rewritten.destination_branch, "trunk");
}

#[tokio::test]
async fn target_needs_a_branch_and_a_configured_remote() {
    let fixture = PushFixture::new().await;
    assert!(matches!(
        push_target(fixture.folder(), None).await,
        Err(GitPushTargetError::NoRemote)
    ));
    assert!(matches!(
        push_target(fixture.folder(), Some("elsewhere")).await,
        Err(GitPushTargetError::NoRemote)
    ));
    let named = push_target(fixture.folder(), Some("origin"))
        .await
        .expect("a configured remote should have a target");
    assert_eq!(named.destination_branch, "main");

    fixture
        .git(&[
            "remote",
            "set-url",
            "--push",
            "origin",
            "https://user:token@example.invalid/r.git",
        ])
        .await;
    assert!(matches!(
        push_target(fixture.folder(), Some("origin")).await,
        Err(GitPushTargetError::Unsupported)
    ));

    fixture.git(&["checkout", "--quiet", "--detach"]).await;
    assert!(matches!(
        push_target(fixture.folder(), Some("origin")).await,
        Err(GitPushTargetError::Detached)
    ));
}

#[tokio::test]
async fn push_sends_the_recorded_commit_even_after_the_branch_moves() {
    let fixture = PushFixture::new().await;
    let target = push_target(fixture.folder(), Some("origin"))
        .await
        .expect("target should read");
    fixture.commit_file("two.txt", "test: second").await;
    push_object(fixture.folder(), &target)
        .await
        .expect("the recorded commit should push");
    assert_eq!(
        fixture.remote_branch("main").await.as_deref(),
        Some(target.object_id.as_str())
    );
}

#[tokio::test]
async fn push_never_forces_or_takes_another_remote() {
    let fixture = PushFixture::new().await;
    let first = push_target(fixture.folder(), Some("origin"))
        .await
        .expect("target should read");
    fixture.commit_file("two.txt", "test: second").await;
    let second = push_target(fixture.folder(), Some("origin"))
        .await
        .expect("target should read");
    push_object(fixture.folder(), &second)
        .await
        .expect("the newer commit should push");
    assert!(
        push_object(fixture.folder(), &first).await.is_err(),
        "an older commit must not rewind the remote branch"
    );
    assert_eq!(
        fixture.remote_branch("main").await.as_deref(),
        Some(second.object_id.as_str())
    );

    let elsewhere = GitPushTarget {
        remote: fixture.remote.text().to_string(),
        ..second.clone()
    };
    assert!(push_object(fixture.folder(), &elsewhere).await.is_err());
    let forced = GitPushTarget {
        destination_branch: "+main".to_string(),
        ..second
    };
    assert!(push_object(fixture.folder(), &forced).await.is_err());
}

#[test]
fn push_urls_name_only_standard_transports() {
    for url in [
        "file:///srv/git/repo.git",
        "git://example.com/repo.git",
        "git+ssh://git@example.com/repo.git",
        "HTTPS://example.com/repo.git",
        "C:/repo.git",
        "./relative/repo.git",
    ] {
        assert!(is_supported_push_url(url), "{url}");
    }
    for url in [
        "ext::sh -c touch% /tmp/pwned",
        "ext::git-upload-pack",
        "fd::7",
        "custom::https://example.com/repo.git",
        "ftp://example.com/repo.git",
        "persistent-https://example.com/repo.git",
    ] {
        assert!(!is_supported_push_url(url), "{url:?}");
    }
}

#[test]
fn local_push_urls_follow_gits_rule() {
    for url in [
        "/srv/git/repo.git",
        "repo.git",
        "./host:repo.git",
        "file:///srv/git/repo.git",
        "FILE:///srv/git/repo.git",
    ] {
        assert!(is_local_push_url(url), "{url}");
    }
    for url in [
        "git@github.com:org/repo.git",
        "host:repo.git",
        "ssh://host/srv/repo.git",
        "https://example.com/repo.git",
    ] {
        assert!(!is_local_push_url(url), "{url}");
    }
}

#[tokio::test]
async fn target_refuses_a_remote_with_several_push_urls() {
    let fixture = PushFixture::new().await;
    let second = TestDirectory::new("second");
    fixture
        .git(&[
            "remote",
            "set-url",
            "--add",
            "--push",
            "origin",
            fixture.remote.text(),
        ])
        .await;
    fixture
        .git(&[
            "remote",
            "set-url",
            "--add",
            "--push",
            "origin",
            second.text(),
        ])
        .await;
    assert!(matches!(
        push_target(fixture.folder(), Some("origin")).await,
        Err(GitPushTargetError::Unsupported)
    ));
}

/// Writes an executable shell script that appends `name` to `marker`.
#[cfg(unix)]
fn write_marker_hook(path: &Path, marker: &Path, name: &str) {
    use std::os::unix::fs::PermissionsExt;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("hook directory should be created");
    }
    let body = format!(
        "#!/bin/sh\necho {name} >> '{}'\nexit 0\n",
        marker.to_str().expect("marker path should be UTF-8")
    );
    fs::write(path, body).expect("hook should write");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .expect("hook should be executable");
}

fn markers(marker: &Path) -> String {
    fs::read_to_string(marker).unwrap_or_default()
}

/// Hooks a teammate could plant in the folder, in a hooks path it names through an included
/// worktree file, and in a local remote never run when a confirmed request pushes, even in a
/// folder this device trusts. A trusted user push of another branch first proves they are live.
#[cfg(unix)]
#[tokio::test]
async fn confirmed_push_runs_no_folder_hooks_path_or_remote_hooks_even_when_trusted() {
    use super::super::hardened::GitFolderTrust;
    use super::super::hardened::folder_configuration;

    let fixture = PushFixture::new().await;
    let marker_directory = TestDirectory::new("markers");
    let marker = marker_directory.path().join("ran");
    let git_directory = fixture.work.path().join(".git");
    for hook in ["pre-push", "reference-transaction", "post-update"] {
        write_marker_hook(&git_directory.join("hooks").join(hook), &marker, hook);
    }
    for hook in [
        "pre-receive",
        "update",
        "post-receive",
        "reference-transaction",
    ] {
        write_marker_hook(
            &fixture.remote.path().join("hooks").join(hook),
            &marker,
            &format!("remote-{hook}"),
        );
    }
    let trusted = |root| async move {
        let digest = folder_configuration(Path::to_path_buf(root))
            .await
            .expect("configuration should digest")
            .git;
        GitFolderTrust::Trusted(digest)
    };

    let trust = trusted(fixture.work.path()).await;
    let folder = GitFolder::new(fixture.work.path(), trust);
    run(
        folder,
        &["push", "--quiet", "origin", "main:probe"],
        GitCall::UserNetwork,
    )
    .await
    .expect("a trusted user push should succeed");
    let probe = markers(&marker);
    assert!(probe.contains("pre-push"), "{probe}");
    assert!(probe.contains("remote-pre-receive"), "{probe}");
    fs::remove_file(&marker).expect("marker should be removed");

    fixture.commit_file("two.txt", "test: second").await;
    let target = push_target(folder, Some("origin"))
        .await
        .expect("target should read");
    push_object(
        GitFolder::new(fixture.work.path(), trusted(fixture.work.path()).await),
        &target,
    )
    .await
    .expect("the confirmed push should succeed");
    assert_eq!(
        fixture.remote_branch("main").await.as_deref(),
        Some(target.object_id.as_str())
    );
    assert_eq!(markers(&marker), "", "no hook may run on a confirmed push");

    // A hooks path named by a worktree file the repository configuration includes.
    let planted_hooks = fixture.work.path().join("planted-hooks");
    write_marker_hook(&planted_hooks.join("pre-push"), &marker, "planted-pre-push");
    fs::write(
        fixture.work.path().join("planted.config"),
        format!(
            "[core]\n\thooksPath = {}\n",
            planted_hooks.to_str().expect("hooks path should be UTF-8")
        ),
    )
    .expect("planted configuration should write");
    fixture
        .git(&["config", "include.path", "../planted.config"])
        .await;
    fixture.commit_file("three.txt", "test: third").await;
    let target = push_target(fixture.folder(), Some("origin"))
        .await
        .expect("target should read");
    push_object(
        GitFolder::new(fixture.work.path(), trusted(fixture.work.path()).await),
        &target,
    )
    .await
    .expect("the confirmed push should succeed");
    assert_eq!(markers(&marker), "", "a planted hooks path must not run");
}

/// URL rewrites and push URLs a teammate plants through an included worktree file after the
/// request change what a fresh read reports, so confirmation can refuse. Credential helpers,
/// SSH commands, and proxies from repository configuration are refused outright.
#[tokio::test]
async fn planted_rewrites_change_the_target_and_connection_settings_are_refused() {
    use super::super::hardened::requested_configuration;

    let fixture = PushFixture::new().await;
    let planted = fixture.work.path().join("planted.config");
    fs::write(&planted, "").expect("planted configuration should write");
    fixture
        .git(&["config", "include.path", "../planted.config"])
        .await;
    let digest = || async {
        requested_configuration(fixture.work.path().to_path_buf())
            .await
            .expect("configuration should read")
    };
    let recorded = push_target(fixture.folder(), Some("origin"))
        .await
        .expect("target should read");
    let recorded_configuration = digest().await;
    assert!(!recorded_configuration.repository_connection);

    let elsewhere = TestDirectory::new("elsewhere");
    run(
        GitFolder::untrusted(elsewhere.path()),
        &["init", "--bare", "--quiet"],
        GitCall::User,
    )
    .await
    .expect("other remote should initialize");
    for planted_text in [
        format!(
            "[url \"{}\"]\n\tpushInsteadOf = {}\n",
            elsewhere.text(),
            fixture.remote.text()
        ),
        format!("[remote \"origin\"]\n\tpushurl = {}\n", elsewhere.text()),
    ] {
        fs::write(&planted, &planted_text).expect("planted configuration should write");
        let fresh = push_target(fixture.folder(), Some("origin"))
            .await
            .expect("target should read");
        assert_eq!(fresh.push_url, elsewhere.text(), "{planted_text}");
        assert_ne!(fresh, recorded);
        assert_ne!(digest().await.digest, recorded_configuration.digest);
        assert!(
            push_object(fixture.folder(), &recorded).await.is_err(),
            "a push whose URL changed must not run"
        );
        assert!(fixture.remote_branch("main").await.is_none());
        assert!(
            run(
                GitFolder::untrusted(elsewhere.path()),
                &["rev-parse", "--verify", "--quiet", "refs/heads/main"],
                GitCall::Internal,
            )
            .await
            .is_err(),
            "nothing may reach the planted remote"
        );
    }

    for planted_text in [
        "[credential]\n\thelper = !echo planted\n",
        "[credential \"https://example.invalid\"]\n\thelper = store\n",
        "[core]\n\tsshCommand = ssh -o ProxyCommand=planted\n",
        "[http]\n\tproxy = http://127.0.0.1:9\n",
        "[http]\n\tsslVerify = false\n",
        "[remote \"origin\"]\n\tproxy = http://127.0.0.1:9\n",
    ] {
        fs::write(&planted, planted_text).expect("planted configuration should write");
        assert!(digest().await.repository_connection, "{planted_text}");
        assert!(
            matches!(
                push_target(fixture.folder(), Some("origin")).await,
                Err(GitPushTargetError::RepositoryConnection)
            ),
            "{planted_text}"
        );
        assert!(push_object(fixture.folder(), &recorded).await.is_err());
    }
    assert!(fixture.remote_branch("main").await.is_none());

    fs::write(&planted, "").expect("planted configuration should clear");
    push_object(fixture.folder(), &recorded)
        .await
        .expect("the recorded push should succeed once the folder is clean");
}

/// A remote helper chosen by repository configuration is refused even when repository
/// configuration also allows its protocol.
#[tokio::test]
async fn confirmed_push_refuses_remote_helpers_the_repository_allows() {
    let fixture = PushFixture::new().await;
    fixture
        .git(&["config", "remote.origin.vcs", "planted"])
        .await;
    fixture
        .git(&["config", "protocol.planted.allow", "always"])
        .await;
    let target = GitPushTarget {
        local_branch: "main".to_string(),
        remote: "origin".to_string(),
        destination_branch: "main".to_string(),
        push_url: fixture.remote.text().to_string(),
        object_id: fixture.head().await,
    };
    let error = push_object(fixture.folder(), &target)
        .await
        .expect_err("a planted remote helper must not run");
    let stderr = error
        .details
        .as_ref()
        .and_then(|details| details.get("stderr"))
        .and_then(|stderr| stderr.as_str())
        .unwrap_or_default()
        .to_string();
    assert!(
        stderr.contains("not allowed"),
        "the protocol allowlist should refuse the helper: {stderr}"
    );
    assert!(fixture.remote_branch("main").await.is_none());
}
