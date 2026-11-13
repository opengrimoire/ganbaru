//! Escape attempts against the helper binary on the running kernel.
//!
//! Each test starts this test binary again under the helper, with a role in the environment;
//! the [`child_role`] entry performs that one operation inside the sandbox and reports through
//! its exit status whether the kernel allowed it.
#![cfg(target_os = "linux")]

use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ganbaru_sandbox::{NetworkAccess, SandboxHelper, SandboxPolicy, probe};
use socket2::{Domain, SockAddr, Socket, Type};

const ROLE: &str = "GANBARU_SANDBOX_TEST_ROLE";
const TARGET: &str = "GANBARU_SANDBOX_TEST_TARGET";
const ALLOWED: i32 = 10;
const DENIED: i32 = 20;
const UNEXPECTED: i32 = 30;
/// Exit status of the helper when it refuses a policy, from `src/main.rs`.
const HELPER_REFUSED: i32 = 125;
/// Exit status of the helper when the confined program could not be executed.
const HELPER_NOT_EXECUTED: i32 = 126;
/// ELF program header type of the interpreter path.
const PT_INTERP: u32 = 3;

/// Entry the confined child runs; a no-op in the normal test run.
#[test]
fn child_role() {
    let Some(role) = std::env::var_os(ROLE) else {
        return;
    };
    let target = std::env::var_os(TARGET).map(PathBuf::from);
    let outcome = perform(role.to_str().unwrap_or_default(), target.as_deref());
    std::process::exit(match outcome {
        Ok(()) => ALLOWED,
        Err(error) if is_refusal(&error) => {
            eprintln!("refused: {error}");
            DENIED
        }
        Err(error) => {
            eprintln!("unexpected error: {error}");
            UNEXPECTED
        }
    });
}

fn perform(role: &str, target: Option<&Path>) -> std::io::Result<()> {
    let target = || target.ok_or_else(|| std::io::Error::other("no target"));
    match role {
        "write" => std::fs::write(target()?, b"escape"),
        "read" => std::fs::read(target()?).map(drop),
        "list" => std::fs::read_dir(target()?).map(drop),
        "execute" => Command::new(target()?).arg("--list").output().map(drop),
        "tcp" => TcpStream::connect(target_address(target()?)?).map(drop),
        "udp" => UdpSocket::bind("127.0.0.1:0").map(drop),
        "unix_stream" => UnixStream::connect(target()?).map(drop),
        "unix_datagram" => UnixDatagram::unbound()?
            .send_to(b"escape", target()?)
            .map(drop),
        "socketpair" => {
            let (mut left, mut right) = UnixStream::pair()?;
            left.write_all(b"x")?;
            let mut byte = [0];
            right.read_exact(&mut byte)
        }
        "pair_datagram_send" => {
            let (left, _right) = UnixDatagram::pair()?;
            left.send_to(b"escape", target()?).map(drop)
        }
        "pair_datagram_connect" => {
            let (left, _right) = UnixDatagram::pair()?;
            left.connect(target()?)?;
            left.send(b"escape").map(drop)
        }
        "seqpacket_pair" => {
            let (left, right) = seqpacket_pair()?;
            left.send(b"x")?;
            receive_from_peer(&right)
        }
        "pair_seqpacket_send" => {
            // The kernel ignores the address on a connected sequenced packet socket, so the
            // send succeeds only by reaching the peer of the pair.
            let (left, right) = seqpacket_pair()?;
            left.send_to(b"x", &SockAddr::unix(target()?)?)?;
            receive_from_peer(&right)
        }
        "pair_seqpacket_connect" => {
            let (left, _right) = seqpacket_pair()?;
            left.connect(&SockAddr::unix(target()?)?)?;
            left.send(b"escape").map(drop)
        }
        other => Err(std::io::Error::other(format!("unknown role {other}"))),
    }
}

fn seqpacket_pair() -> std::io::Result<(Socket, Socket)> {
    Socket::pair(Domain::UNIX, Type::SEQPACKET, None)
}

/// Reads one byte from `socket` and checks it is the one its peer sent.
fn receive_from_peer(socket: &Socket) -> std::io::Result<()> {
    let mut byte = [0];
    let received = (&*socket).read(&mut byte)?;
    if received == 1 && byte == *b"x" {
        Ok(())
    } else {
        Err(std::io::Error::other("the peer did not receive the packet"))
    }
}

/// Checks that nothing reached `socket` without waiting for it.
fn assert_nothing_received(socket: &UnixDatagram) {
    socket.set_nonblocking(true).unwrap();
    let mut buffer = [0; 16];
    let error = socket.recv(&mut buffer).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
}

/// Checks that no connection is waiting on `listener` without waiting for one.
fn assert_no_connection(listener: &Socket) {
    listener.set_nonblocking(true).unwrap();
    let error = listener.accept().unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
}

/// A sequenced packet listener at `path` outside the sandbox.
fn seqpacket_listener(path: &Path) -> Socket {
    let listener = Socket::new(Domain::UNIX, Type::SEQPACKET, None).unwrap();
    listener.bind(&SockAddr::unix(path).unwrap()).unwrap();
    listener.listen(1).unwrap();
    listener
}

fn target_address(target: &Path) -> std::io::Result<String> {
    target
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| std::io::Error::other("address is not text"))
}

/// Whether the kernel refused the operation. `EISCONN` counts: a connected pair cannot be
/// pointed at another socket.
fn is_refusal(error: &std::io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::EACCES | libc::EPERM | libc::EISCONN)
    )
}

/// A directory of this test under Cargo's temporary directory, empty at the start.
fn scratch(name: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("ganbaru-sandbox-escape")
        .join(name);
    if directory.exists() {
        std::fs::remove_dir_all(&directory).unwrap();
    }
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

/// A directory for named local sockets, whose paths must stay shorter than `SUN_LEN`.
struct SocketDirectory(PathBuf);

impl SocketDirectory {
    fn new(name: &str) -> Self {
        let directory =
            std::env::temp_dir().join(format!("ganbaru-sandbox-{}-{name}", std::process::id()));
        if directory.exists() {
            std::fs::remove_dir_all(&directory).unwrap();
        }
        std::fs::create_dir(&directory).unwrap();
        Self(directory)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for SocketDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn helper() -> SandboxHelper {
    SandboxHelper::at(env!("CARGO_BIN_EXE_ganbaru-sandbox"))
}

fn test_binary() -> PathBuf {
    std::env::current_exe().unwrap()
}

/// The dynamic loader named by this test binary, which must be executable to start it again.
fn interpreter() -> Option<PathBuf> {
    let image = std::fs::read(test_binary()).unwrap();
    assert_eq!(&image[..4], b"\x7fELF");
    assert_eq!(image[4], 2, "64-bit ELF expected");
    let u16_at = |offset: usize| u16::from_le_bytes(image[offset..offset + 2].try_into().unwrap());
    let u32_at = |offset: usize| u32::from_le_bytes(image[offset..offset + 4].try_into().unwrap());
    let u64_at = |offset: usize| {
        usize::try_from(u64::from_le_bytes(
            image[offset..offset + 8].try_into().unwrap(),
        ))
        .unwrap()
    };
    let (table, entry_size, entries) = (u64_at(0x20), usize::from(u16_at(0x36)), u16_at(0x38));
    (0..usize::from(entries))
        .map(|index| table + index * entry_size)
        .find(|header| u32_at(*header) == PT_INTERP)
        .map(|header| {
            let start = u64_at(header + 8);
            let path = &image[start..start + u64_at(header + 0x20)];
            let path = path.strip_suffix(b"\0").unwrap_or(path);
            PathBuf::from(std::str::from_utf8(path).unwrap())
        })
}

/// A policy that can start this test binary, and other programs from it, and reaches nothing
/// else.
fn base_policy(network: NetworkAccess) -> SandboxPolicy {
    let mut read = ["/usr", "/etc", "/lib", "/lib64"]
        .map(PathBuf::from)
        .into_iter()
        .filter(|path| path.exists())
        .collect::<Vec<_>>();
    let mut execute = vec![test_binary()];
    if let Some(interpreter) = interpreter() {
        let interpreter = interpreter.canonicalize().unwrap();
        read.push(interpreter.parent().unwrap().to_path_buf());
        execute.push(interpreter);
    }
    SandboxPolicy {
        read,
        execute,
        // Starting a program opens /dev/null for the streams it does not inherit.
        write: vec![PathBuf::from("/dev/null")],
        network,
    }
}

fn run(policy: &SandboxPolicy, role: &str, target: impl Into<OsString>) -> Output {
    through_shell(&helper_command(policy), &close_inherited_descriptors())
        .env(ROLE, role)
        .env(TARGET, target.into())
        .output()
        .unwrap()
}

fn helper_command(policy: &SandboxPolicy) -> Command {
    helper()
        .command(
            policy,
            test_binary(),
            ["--exact", "child_role", "--nocapture", "--test-threads=1"],
        )
        .unwrap()
}

/// A shell that runs `redirections` and then becomes `command`, so the redirections decide
/// which descriptors the helper starts with.
fn through_shell(command: &Command, redirections: &str) -> Command {
    let mut shell = Command::new("/bin/sh");
    shell
        .arg("-c")
        .arg(format!("exec {redirections} \"$@\""))
        .arg("sh")
        .arg(command.get_program())
        .args(command.get_args());
    shell
}

/// Shell redirections that close every descriptor above the standard streams that this test
/// would pass on, since the test may itself inherit one, such as a build lock, that the helper
/// would refuse.
fn close_inherited_descriptors() -> String {
    std::fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(|entry| entry.unwrap().file_name().to_str()?.parse::<u32>().ok())
        .filter(|descriptor| *descriptor > 2 && !closes_on_exec(*descriptor))
        .map(|descriptor| {
            // The POSIX shell redirects only single digit descriptors.
            assert!(
                descriptor < 10,
                "descriptor {descriptor} would reach the helper"
            );
            format!("{descriptor}>&- ")
        })
        .collect()
}

/// Whether `descriptor` has close-on-exec set, read from its `fdinfo` flags in octal.
fn closes_on_exec(descriptor: u32) -> bool {
    let Ok(info) = std::fs::read_to_string(format!("/proc/self/fdinfo/{descriptor}")) else {
        // The listing's own descriptor is gone by the time its information is read.
        return true;
    };
    info.lines()
        .find_map(|line| line.strip_prefix("flags:"))
        .and_then(|flags| u32::from_str_radix(flags.trim(), 8).ok())
        .is_some_and(|flags| flags & libc::O_CLOEXEC as u32 != 0)
}

#[track_caller]
fn assert_outcome(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn the_probe_passes_on_this_kernel() {
    probe(&helper()).unwrap();
}

#[test]
fn writes_stay_inside_writable_paths() {
    let root = scratch("writes");
    let inside = root.join("inside");
    std::fs::create_dir(&inside).unwrap();
    let mut policy = base_policy(NetworkAccess::Denied);
    policy.write.push(inside.clone());
    assert_outcome(&run(&policy, "write", inside.join("file")), ALLOWED);
    assert_outcome(&run(&policy, "write", root.join("outside")), DENIED);
    assert!(!root.join("outside").exists());
}

#[test]
fn reads_stay_inside_granted_paths() {
    let root = scratch("reads");
    let granted = root.join("granted");
    let hidden = root.join("hidden");
    for directory in [&granted, &hidden] {
        std::fs::create_dir(directory).unwrap();
        std::fs::write(directory.join("file"), b"secret").unwrap();
    }
    let mut policy = base_policy(NetworkAccess::Denied);
    policy.read.push(granted.clone());
    assert_outcome(&run(&policy, "read", granted.join("file")), ALLOWED);
    assert_outcome(&run(&policy, "read", hidden.join("file")), DENIED);
    assert_outcome(&run(&policy, "list", &hidden), DENIED);
    assert_outcome(&run(&policy, "write", granted.join("file")), DENIED);
}

#[test]
fn programs_outside_executable_paths_do_not_start() {
    let root = scratch("execute");
    let copy = root.join("copy");
    std::fs::copy(test_binary(), &copy).unwrap();
    let mut policy = base_policy(NetworkAccess::Denied);
    policy.read.push(root.clone());
    assert_outcome(&run(&policy, "execute", &copy), DENIED);
}

#[test]
fn denied_network_refuses_every_socket_that_reaches_out() {
    let root = SocketDirectory::new("denied");
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    let stream_path = root.path("stream");
    let _stream = UnixListener::bind(&stream_path).unwrap();
    let datagram_path = root.path("datagram");
    let _datagram = UnixDatagram::bind(&datagram_path).unwrap();
    let policy = base_policy(NetworkAccess::Denied);
    let address = tcp.local_addr().unwrap().to_string();
    assert_outcome(&run(&policy, "tcp", address), DENIED);
    assert_outcome(&run(&policy, "udp", ""), DENIED);
    assert_outcome(&run(&policy, "unix_stream", &stream_path), DENIED);
    assert_outcome(&run(&policy, "unix_datagram", &datagram_path), DENIED);
    assert_outcome(&run(&policy, "socketpair", ""), ALLOWED);
    assert_outcome(&run(&policy, "pair_datagram_send", &datagram_path), DENIED);
    assert_outcome(
        &run(&policy, "pair_datagram_connect", &datagram_path),
        DENIED,
    );
}

#[test]
fn tcp_access_reaches_only_the_allowed_ports_and_no_local_socket() {
    let root = SocketDirectory::new("tcp");
    let allowed = TcpListener::bind("127.0.0.1:0").unwrap();
    let other = TcpListener::bind("127.0.0.1:0").unwrap();
    let stream_path = root.path("stream");
    let _stream = UnixListener::bind(&stream_path).unwrap();
    let datagram_path = root.path("datagram");
    let _datagram = UnixDatagram::bind(&datagram_path).unwrap();
    let port = allowed.local_addr().unwrap().port();
    let policy = base_policy(NetworkAccess::TcpConnect(vec![port]));
    let address = |listener: &TcpListener| listener.local_addr().unwrap().to_string();
    assert_outcome(&run(&policy, "tcp", address(&allowed)), ALLOWED);
    assert_outcome(&run(&policy, "tcp", address(&other)), DENIED);
    assert_outcome(&run(&policy, "udp", ""), ALLOWED);
    assert_outcome(&run(&policy, "unix_stream", &stream_path), DENIED);
    assert_outcome(&run(&policy, "unix_datagram", &datagram_path), DENIED);
    assert_outcome(&run(&policy, "socketpair", ""), ALLOWED);
    assert_outcome(&run(&policy, "pair_datagram_send", &datagram_path), DENIED);
    assert_outcome(
        &run(&policy, "pair_datagram_connect", &datagram_path),
        DENIED,
    );
}

#[test]
fn sequenced_packet_pairs_reach_only_their_peer_in_both_modes() {
    let root = SocketDirectory::new("seqpacket");
    let datagram_path = root.path("datagram");
    let datagram = UnixDatagram::bind(&datagram_path).unwrap();
    let listener_path = root.path("seqpacket");
    let listener = seqpacket_listener(&listener_path);
    let outside = TcpListener::bind("127.0.0.1:0").unwrap();
    for network in [
        NetworkAccess::Denied,
        NetworkAccess::TcpConnect(vec![outside.local_addr().unwrap().port()]),
    ] {
        let policy = base_policy(network);
        assert_outcome(&run(&policy, "seqpacket_pair", ""), ALLOWED);
        assert_outcome(
            &run(&policy, "pair_seqpacket_send", &datagram_path),
            ALLOWED,
        );
        assert_nothing_received(&datagram);
        assert_outcome(
            &run(&policy, "pair_seqpacket_connect", &listener_path),
            DENIED,
        );
        assert_no_connection(&listener);
    }
}

#[test]
fn an_inherited_descriptor_is_refused_before_the_program_runs() {
    let root = scratch("descriptor");
    let secret = root.join("secret");
    std::fs::write(&secret, b"secret").unwrap();
    let mut policy = base_policy(NetworkAccess::Denied);
    policy.write.push(root.clone());
    // The shell opens the file on a descriptor without close-on-exec and then becomes the
    // helper, which therefore starts with that descriptor open.
    let redirections = format!(
        "{}5<\"$GANBARU_SANDBOX_TEST_SECRET\"",
        close_inherited_descriptors()
    );
    let output = through_shell(&helper_command(&policy), &redirections)
        .env("GANBARU_SANDBOX_TEST_SECRET", &secret)
        .env(ROLE, "write")
        .env(TARGET, root.join("escaped"))
        .output()
        .unwrap();
    assert_outcome(&output, HELPER_REFUSED);
    assert!(String::from_utf8_lossy(&output.stderr).contains("inherited descriptors [5]"));
    assert!(!root.join("escaped").exists());
}

#[test]
fn the_helper_refuses_a_policy_path_that_does_not_exist() {
    let root = scratch("missing");
    let mut policy = base_policy(NetworkAccess::Denied);
    policy.write.push(root.join("missing"));
    let output = run(&policy, "write", root.join("missing").join("file"));
    assert_outcome(&output, HELPER_REFUSED);
}

#[test]
fn a_writable_path_that_contains_the_helper_is_refused() {
    let helper = helper();
    let mut policy = base_policy(NetworkAccess::Denied);
    policy
        .write
        .push(helper.path().parent().unwrap().to_path_buf());
    assert!(
        helper
            .command(&policy, "true", Vec::<OsString>::new())
            .is_err()
    );
}

#[test]
fn a_path_both_writable_and_executable_keeps_both_rights() {
    let root = scratch("write-and-execute");
    let copy = root.join("copy");
    let mut policy = base_policy(NetworkAccess::Denied);
    policy.write.push(root.clone());
    policy.execute.push(root.clone());
    std::fs::copy(test_binary(), &copy).unwrap();
    assert_outcome(&run(&policy, "execute", &copy), ALLOWED);
    assert_outcome(&run(&policy, "write", root.join("file")), ALLOWED);
}

#[test]
fn a_dynamic_program_needs_execute_access_to_its_loader() {
    let Some(loader) = interpreter() else {
        return;
    };
    let loader = loader.canonicalize().unwrap();
    let mut policy = base_policy(NetworkAccess::Denied);
    policy.execute.retain(|path| *path != loader);
    let output = run(&policy, "write", "");
    assert_outcome(&output, HELPER_NOT_EXECUTED);
}
