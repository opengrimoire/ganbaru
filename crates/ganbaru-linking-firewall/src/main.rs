//! Root-run helper that adds and removes the Ganbaru device linking firewall rules.
//!
//! Packaged Linux builds run it through `pkexec` under the polkit policy that names its
//! installed path, and package removal runs it to revoke every rule. It is separate from
//! the app so that no GUI library code runs with administrator rights.

use std::process::ExitCode;

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    match ganbaru_handoff::network_access::run_privileged_helper_if_requested() {
        Some(Ok(())) => ExitCode::SUCCESS,
        Some(Err(error)) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
        None => {
            eprintln!(
                "usage: ganbaru-linking-firewall --manage-linking-firewall <grant|revoke> (<ufw|firewalld> <interface> <address> <source> <zone|->)..."
            );
            eprintln!("       ganbaru-linking-firewall --manage-linking-firewall revoke-all");
            ExitCode::from(2)
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    eprintln!("the device linking firewall helper is available only on Linux");
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    const LOCKFILE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock"));
    /// Package name prefixes of GUI, webview, and desktop toolkit crates.
    const GUI_PACKAGE_PREFIXES: &[&str] = &[
        "tauri",
        "wry",
        "tao",
        "muda",
        "tray-icon",
        "gtk",
        "gdk",
        "glib",
        "gio",
        "webkit",
        "javascriptcore",
        "soup",
        "cairo",
        "pango",
        "atk",
        "libappindicator",
        "x11",
        "winit",
    ];

    /// Maps each package name in `Cargo.lock` to the names it depends on, merging versions.
    fn dependency_graph(lockfile: &str) -> BTreeMap<String, BTreeSet<String>> {
        let mut graph = BTreeMap::<String, BTreeSet<String>>::new();
        for block in lockfile.split("[[package]]").skip(1) {
            let mut name = None;
            let mut dependencies = BTreeSet::new();
            let mut in_dependencies = false;
            for line in block.lines().map(str::trim) {
                if let Some(value) = line.strip_prefix("name = ") {
                    name = Some(value.trim_matches('"').to_string());
                } else if line == "dependencies = [" {
                    in_dependencies = true;
                } else if in_dependencies && line == "]" {
                    in_dependencies = false;
                } else if in_dependencies {
                    let entry = line.trim_end_matches(',').trim_matches('"');
                    if let Some(dependency) = entry.split_whitespace().next() {
                        dependencies.insert(dependency.to_string());
                    }
                }
            }
            let name = name.expect("every Cargo.lock package has a name");
            graph.entry(name).or_default().extend(dependencies);
        }
        graph
    }

    fn closure(graph: &BTreeMap<String, BTreeSet<String>>, root: &str) -> BTreeSet<String> {
        let mut reached = BTreeSet::new();
        let mut pending = vec![root.to_string()];
        while let Some(name) = pending.pop() {
            if reached.insert(name.clone()) {
                pending.extend(graph.get(&name).into_iter().flatten().cloned());
            }
        }
        reached
    }

    fn gui_packages(packages: &BTreeSet<String>) -> Vec<&String> {
        packages
            .iter()
            .filter(|name| {
                GUI_PACKAGE_PREFIXES
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
            })
            .collect()
    }

    #[test]
    fn the_root_helper_links_no_gui_library() {
        let graph = dependency_graph(LOCKFILE);
        let helper = closure(&graph, env!("CARGO_PKG_NAME"));

        assert!(helper.contains("ganbaru-handoff"));
        assert_eq!(gui_packages(&helper), Vec::<&String>::new());
    }

    #[test]
    fn the_gui_check_detects_the_app_dependencies() {
        let graph = dependency_graph(LOCKFILE);
        let app = closure(&graph, "ganbaru");

        let found = gui_packages(&app);
        assert!(found.iter().any(|name| name.as_str() == "tauri"));
        assert!(found.iter().any(|name| name.starts_with("webkit")));
    }
}
