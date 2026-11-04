//! How the polkit policy and the installed helper admit a privileged request.
#![cfg(target_os = "linux")]

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::process::Command;

const POLICY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../apps/client/src-tauri/packaging/linux/org.opengrimoire.ganbaru.policy"
));

#[test]
fn the_policy_asks_for_the_administrator_password_on_every_run() {
    let defaults: Vec<&str> = POLICY
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("<allow_"))
        .collect();
    assert_eq!(
        defaults,
        [
            "<allow_any>no</allow_any>",
            "<allow_inactive>no</allow_inactive>",
            "<allow_active>auth_admin</allow_active>",
        ]
    );
    assert!(!POLICY.contains("_keep"));
    assert!(POLICY.contains(
        r#"<annotate key="org.freedesktop.policykit.exec.path">/usr/lib/ganbaru/ganbaru-linking-firewall</annotate>"#
    ));
    assert!(POLICY.contains(
        r#"<annotate key="org.freedesktop.policykit.exec.argv1">--manage-linking-firewall</annotate>"#
    ));
}

#[test]
fn a_request_that_is_not_utf8_is_refused_without_a_panic() {
    let output = Command::new(env!("CARGO_BIN_EXE_ganbaru-linking-firewall"))
        .arg("--manage-linking-firewall")
        .arg("grant")
        .arg(OsStr::from_bytes(b"uf\xffw"))
        .output()
        .expect("run the helper");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("not UTF-8"));
}
