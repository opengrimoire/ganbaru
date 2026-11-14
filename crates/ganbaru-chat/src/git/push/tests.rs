use super::super::{GitCall, GitFolder, commit, initialize, run, stage};
use super::{
    BranchTracking, GitPushTarget, GitPushTargetError, is_object_id, is_supported_push_url,
    push_object, push_target,
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
