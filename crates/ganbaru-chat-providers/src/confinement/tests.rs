use super::terminal::{confined_env_arguments, terminal_policy, toolchain_directories};
use super::toolchains::{self, ToolchainGrants};
use super::*;
use ganbaru_sandbox::NetworkAccess;

/// Teammate provider named in refusals by these tests.
const PROVIDER: &str = "Grok";

#[test]
fn the_internal_mcp_port_must_be_a_loopback_port() {
    assert_eq!(
        internal_mcp_port("http://127.0.0.1:41234/mcp", PROVIDER).unwrap(),
        41234
    );
    assert_eq!(
        internal_mcp_port("http://localhost:8123/mcp", PROVIDER).unwrap(),
        8123
    );
    assert_eq!(
        internal_mcp_port("http://[::1]:9000/mcp", PROVIDER).unwrap(),
        9000
    );
    for refused in [
        "http://192.168.1.10:41234/mcp",
        "http://example.com:41234/mcp",
        "http://127.0.0.1:0/mcp",
        "not a url",
    ] {
        assert!(internal_mcp_port(refused, PROVIDER).is_err(), "{refused}");
    }
}

#[test]
fn toolchain_folders_never_open_the_home_or_folder_ancestors() {
    let home = Path::new("/home/person");
    let workspace = Path::new("/data/vault/projects/site");
    let directories = toolchain_directories(
        [
            "/home/person/.cargo/bin",
            "relative/bin",
            "/",
            "/home",
            "/home/person",
            "/data/vault",
            "/usr/bin",
            "/home/person/.cargo/bin",
            "/missing",
            "/home/person/link-to-home",
        ]
        .map(PathBuf::from),
        Some(home),
        workspace,
        |path| match path.to_str() {
            Some("/missing") => None,
            Some("/home/person/link-to-home") => Some(PathBuf::from("/home/person")),
            _ => Some(path.to_path_buf()),
        },
    );
    assert_eq!(
        directories,
        vec![
            PathBuf::from("/home/person/.cargo/bin"),
            PathBuf::from("/usr/bin")
        ]
    );
}

#[test]
fn terminal_commands_write_the_folder_and_have_no_network() {
    let workspace = Path::new("/data/vault/projects/site");
    let temporary = Path::new("/tmp/ganbaru-confined-b");
    let toolchains = [PathBuf::from("/home/person/.cargo/bin")];
    let homes = ToolchainGrants {
        execute: vec![
            PathBuf::from("/home/person/.cargo/bin"),
            PathBuf::from("/home/person/.rustup"),
        ],
        read: vec![PathBuf::from("/home/person/.cargo/registry")],
        write: vec![PathBuf::from("/home/person/.cargo/.package-cache")],
        environment: Vec::new(),
    };
    let policy = terminal_policy(
        workspace,
        temporary,
        &toolchains,
        &homes,
        |_| true,
        PROVIDER,
    )
    .unwrap();
    assert_eq!(
        policy
            .execute
            .iter()
            .filter(|path| path.ends_with(".cargo/bin"))
            .count(),
        1
    );
    assert!(policy.execute.contains(&homes.execute[1]));
    assert!(policy.read.contains(&homes.read[0]));
    assert_eq!(policy.network, NetworkAccess::Denied);
    assert_eq!(
        policy.write,
        vec![
            PathBuf::from(workspace),
            PathBuf::from(temporary),
            PathBuf::from("/dev/null"),
            homes.write[0].clone()
        ]
    );
    assert!(policy.execute.contains(&PathBuf::from(workspace)));
    assert!(policy.execute.contains(&PathBuf::from("/usr")));
    assert!(policy.execute.contains(&toolchains[0]));
    for path in policy.read.iter().chain(&policy.execute) {
        assert!(!Path::new("/home/person").starts_with(path), "{path:?}");
    }
    assert!(
        terminal_policy(
            workspace,
            Path::new("/data/vault/projects/site/tmp"),
            &[],
            &ToolchainGrants::default(),
            |_| true,
            PROVIDER
        )
        .is_err()
    );
}

#[test]
fn confined_environments_are_set_by_env_before_the_program() {
    assert_eq!(
        confined_env_arguments(
            "cargo",
            &["test".to_string(), "A=B".to_string()],
            [("GIT_TERMINAL_PROMPT", "0"), ("VALUE", "x=y")],
        )
        .unwrap(),
        [
            "--",
            "GIT_TERMINAL_PROMPT=0",
            "VALUE=x=y",
            "cargo",
            "test",
            "A=B"
        ]
    );
    assert!(confined_env_arguments("A=B", &[], []).is_err());
    assert!(confined_env_arguments("true", &[], [("LD_PRELOAD=/x.so#", "")]).is_err());
    assert!(confined_env_arguments("true", &[], [("", "value")]).is_err());
}

/// Runs a real command under the helper Cargo built for this workspace, when it is present and
/// the kernel enforces it.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn confined_terminal_commands_write_the_folder_and_cannot_read_the_home() {
    let built = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/ganbaru-sandbox");
    let Ok(built) = std::fs::canonicalize(built) else {
        eprintln!("skipped: the sandbox helper is not built");
        return;
    };
    let helper: &'static SandboxHelper = Box::leak(Box::new(SandboxHelper::at(built)));
    if ganbaru_sandbox::probe(helper).is_err() {
        eprintln!("skipped: the kernel does not enforce the sandbox");
        return;
    }
    let workspace = crate::probe_directory::ProbeDirectory::create().unwrap();
    let outside = crate::probe_directory::ProbeDirectory::create().unwrap();
    std::fs::write(outside.join("secret"), "secret").unwrap();
    let sandbox = TerminalSandbox::prepare(helper, &workspace, PROVIDER).unwrap();
    let script = format!(
        "echo \"$REQUESTED\" > made && ! cat {outside}/secret 2>/dev/null",
        outside = outside.display()
    );
    let output = sandbox
        .command(
            POSIX_SHELL_FOR_TESTS,
            &["-c".to_string(), script],
            [("REQUESTED", "made")],
        )
        .unwrap()
        .current_dir(&*workspace)
        .output()
        .await
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    // A test runner that leaks a descriptor into this process makes the helper refuse to start,
    // which is the helper working as intended rather than a policy failure.
    if stderr.contains("inherited descriptors") {
        eprintln!("skipped: the test runner leaked a descriptor: {stderr}");
        return;
    }
    assert!(output.status.success(), "{stderr}");
    assert_eq!(
        std::fs::read_to_string(workspace.join("made")).unwrap(),
        "made\n"
    );
}

#[cfg(target_os = "linux")]
const POSIX_SHELL_FOR_TESTS: &str = "/bin/sh";

/// A file system view for toolchain grants: each entry is a path, the path it resolves to, and
/// whether it is a file.
fn resolver(
    entries: &'static [(&'static str, &'static str, bool)],
) -> impl Fn(&Path) -> Option<toolchains::ResolvedPath> {
    move |path| {
        entries
            .iter()
            .find(|(name, _, _)| Path::new(name) == path)
            .map(|(_, target, is_file)| toolchains::ResolvedPath {
                path: PathBuf::from(target),
                is_file: *is_file,
            })
    }
}

#[test]
fn toolchain_homes_grant_programs_and_caches_but_never_credentials() {
    let resolve = resolver(&[
        ("/home/person/.cargo", "/home/person/.cargo", false),
        ("/home/person/.cargo/bin", "/home/person/.cargo/bin", false),
        (
            "/home/person/.cargo/registry",
            "/home/person/.cargo/registry",
            false,
        ),
        (
            "/home/person/.cargo/config.toml",
            "/home/person/.cargo/config.toml",
            true,
        ),
        (
            "/home/person/.cargo/credentials.toml",
            "/home/person/.cargo/credentials.toml",
            true,
        ),
        (
            "/home/person/.cargo/.package-cache",
            "/home/person/.cargo/.package-cache",
            true,
        ),
        // A lock that is a folder would open the folder to writes.
        (
            "/home/person/.cargo/.package-cache-mutate",
            "/home/person/.cargo/.package-cache-mutate",
            false,
        ),
        ("/opt/rustup", "/opt/rustup", false),
        (
            "/home/person/.local/share/pnpm",
            "/home/person/.local/share/pnpm",
            false,
        ),
    ]);
    let grants = toolchains::toolchain_grants(
        |name| match name {
            "RUSTUP_HOME" => Some("/opt/rustup".into()),
            // A relative variable falls back to the default place.
            "PNPM_HOME" => Some("pnpm".into()),
            _ => None,
        },
        Some(Path::new("/home/person")),
        Path::new("/data/vault/projects/site"),
        resolve,
    );
    assert_eq!(
        grants.execute,
        [
            PathBuf::from("/home/person/.cargo/bin"),
            PathBuf::from("/opt/rustup"),
            PathBuf::from("/home/person/.local/share/pnpm"),
        ]
    );
    assert_eq!(
        grants.read,
        [
            PathBuf::from("/home/person/.cargo/registry"),
            PathBuf::from("/home/person/.cargo/config.toml"),
        ]
    );
    assert_eq!(
        grants.write,
        [PathBuf::from("/home/person/.cargo/.package-cache")]
    );
    assert_eq!(
        grants
            .environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.to_str().unwrap()))
            .collect::<Vec<_>>(),
        [
            ("CARGO_HOME", "/home/person/.cargo"),
            ("RUSTUP_HOME", "/opt/rustup"),
            ("PNPM_HOME", "/home/person/.local/share/pnpm"),
        ]
    );
}

#[test]
fn toolchain_homes_that_open_user_files_are_left_out() {
    let resolve = resolver(&[
        ("/home/person", "/home/person", false),
        ("/home/person/.ssh", "/home/person/.ssh", false),
        ("/home/person/.ssh/cargo", "/home/person/.ssh/cargo", false),
        ("/data/vault", "/data/vault", false),
        (
            "/data/vault/projects/site/.nvm",
            "/data/vault/projects/site/.nvm",
            false,
        ),
        ("/home/person/.cargo", "/home/person/.cargo", false),
        // A part that leads outside its home is never granted.
        ("/home/person/.cargo/bin", "/home/person", false),
        ("/home/person/.cargo/registry", "/home/person/.ssh", false),
    ]);
    let grants = toolchains::toolchain_grants(
        |name| match name {
            "RUSTUP_HOME" => Some("/home/person".into()),
            "PNPM_HOME" => Some("/home/person/.ssh/cargo".into()),
            "NVM_DIR" => Some("/data/vault/projects/site/.nvm".into()),
            _ => None,
        },
        Some(Path::new("/home/person")),
        Path::new("/data/vault/projects/site"),
        resolve,
    );
    assert!(grants.execute.is_empty(), "{:?}", grants.execute);
    assert!(grants.read.is_empty(), "{:?}", grants.read);
    assert!(grants.write.is_empty());
    assert_eq!(
        grants
            .environment
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["CARGO_HOME"]
    );
    // A home that holds the working folder, such as one around the vault, is left out too.
    let around_the_vault = toolchains::toolchain_grants(
        |name| (name == "NVM_DIR").then(|| "/data/vault".into()),
        None,
        Path::new("/data/vault/projects/site"),
        resolver(&[("/data/vault", "/data/vault", false)]),
    );
    assert_eq!(around_the_vault, ToolchainGrants::default());
}

/// Builds a tiny crate offline under the helper Cargo built for this workspace, with Cargo and
/// rustup found in place, while the home directory's `.ssh` and a vault stay unreadable.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn confined_commands_build_offline_with_the_real_toolchain() {
    let built = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/ganbaru-sandbox");
    let Ok(built) = std::fs::canonicalize(built) else {
        eprintln!("skipped: the sandbox helper is not built");
        return;
    };
    let helper: &'static SandboxHelper = Box::leak(Box::new(SandboxHelper::at(built)));
    if ganbaru_sandbox::probe(helper).is_err() {
        eprintln!("skipped: the kernel does not enforce the sandbox");
        return;
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        eprintln!("skipped: no home directory");
        return;
    };
    let workspace = crate::probe_directory::ProbeDirectory::create().unwrap();
    let vault = crate::probe_directory::ProbeDirectory::create().unwrap();
    std::fs::write(vault.join("ganbaru.sqlite"), "secret").unwrap();
    std::fs::create_dir_all(workspace.join("src")).unwrap();
    std::fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"tiny\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(workspace.join("src/main.rs"), "fn main() {}\n").unwrap();
    let sandbox = TerminalSandbox::prepare(helper, &workspace, PROVIDER).unwrap();
    let script = format!(
        "command -v cargo >/dev/null || exit 77; \
         ! ls \"$REAL_HOME/.ssh\" 2>/dev/null && ! cat {vault}/ganbaru.sqlite 2>/dev/null \
         && cargo build --offline --quiet && test -f target/debug/tiny",
        vault = vault.display()
    );
    let output = sandbox
        .command(
            POSIX_SHELL_FOR_TESTS,
            &["-c".to_string(), script],
            [("REAL_HOME", home.to_str().unwrap())],
        )
        .unwrap()
        .current_dir(&*workspace)
        .output()
        .await
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("inherited descriptors") {
        eprintln!("skipped: the test runner leaked a descriptor: {stderr}");
        return;
    }
    if output.status.code() == Some(77) {
        eprintln!("skipped: Cargo is not on PATH");
        return;
    }
    assert!(output.status.success(), "{stderr}");
}
