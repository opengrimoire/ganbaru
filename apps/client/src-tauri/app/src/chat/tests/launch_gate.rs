//! Pins the project configuration gate to every provider launch path.
//!
//! The gate's decision logic has its own tests, which cannot see a call site
//! that skips the gate or hands it a restriction the provider does not
//! enforce. These tests read the production Chat sources instead: every place
//! that hands a provider a launch folder must be listed here with the file
//! that settles its access, and every settle call takes the restriction and
//! read list from the provider driver that launches.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Each production file that hands a provider a launch folder, paired with the
/// file whose code settles the project configuration access of that folder.
const LAUNCH_SITES: &[(&str, &str)] = &[
    ("threads.rs", "threads.rs"),
    ("send/session.rs", "send/coordinator.rs"),
    (
        "interaction/runtime_operations.rs",
        "interaction/runtime_operations.rs",
    ),
];

/// Source fragments that hand a provider a folder to start in.
const LAUNCH_MARKERS: &[&str] = &["VerifiedWorkspaceContext {", "working_directory: Some("];

const SETTLE_FUNCTION: &str = "settle_project_configuration";
const RESTRICTION_ARGUMENT: usize = 4;
const READS_ARGUMENT: usize = 5;
const DRIVER_RESTRICTION: &str = ".project_configuration_restriction()";
const DRIVER_READS: &str = ".project_configuration_paths()";

/// The one production file that may decide a trusted access.
const GATE_FILE: &str = "workspace/commands.rs";

struct Source {
    relative: String,
    text: String,
}

fn chat_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/chat")
}

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()));
    for entry in entries {
        let path = entry.expect("read Chat source entry").path();
        if path.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) != Some("tests") {
                collect_files(&path, files);
            }
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if name != "tests.rs" && !name.ends_with("_tests.rs") {
                files.push(path);
            }
        }
    }
}

/// Keeps the production part of one source file: line comments are dropped
/// and an inline test module ends the file.
fn production_text(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut kept = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let next = lines.get(index + 1).map(|line| line.trim()).unwrap_or("");
        if trimmed.starts_with("#[cfg(")
            && trimmed.contains("test")
            && next.starts_with("mod ")
            && next.ends_with('{')
        {
            break;
        }
        if !trimmed.starts_with("//") {
            kept.push(*line);
        }
    }
    kept.join("\n")
}

fn production_sources() -> Vec<Source> {
    let root = chat_directory();
    let mut files = Vec::new();
    collect_files(&root, &mut files);
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let text = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            let relative = path
                .strip_prefix(&root)
                .expect("source lies under the Chat directory")
                .to_string_lossy()
                .replace('\\', "/");
            Source {
                relative,
                text: production_text(&text),
            }
        })
        .collect()
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Byte offsets of each call to `function` in `text`, skipping its definition.
fn call_offsets(text: &str, function: &str) -> Vec<usize> {
    let pattern = format!("{function}(");
    let bytes = text.as_bytes();
    text.match_indices(&pattern)
        .map(|(offset, _)| offset)
        .filter(|&offset| offset == 0 || !is_identifier_byte(bytes[offset - 1]))
        .filter(|&offset| !text[..offset].trim_end().ends_with("fn"))
        .collect()
}

/// Splits the parenthesized list that opens at `open` into its top-level items.
fn parenthesized_items(text: &str, open: usize) -> Vec<String> {
    assert_eq!(&text[open..=open], "(", "a list opens with a parenthesis");
    let mut depth = 0usize;
    let mut items = Vec::new();
    let mut current = String::new();
    for character in text[open..].chars() {
        match character {
            '(' | '[' | '{' => {
                depth += 1;
                if depth == 1 {
                    continue;
                }
            }
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            ',' if depth == 1 => {
                items.push(current.trim().to_string());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(character);
    }
    let last = current.trim();
    if !last.is_empty() {
        items.push(last.to_string());
    }
    items
}

fn call_arguments(text: &str, offset: usize, function: &str) -> Vec<String> {
    parenthesized_items(text, offset + function.len())
}

/// Name and parameter names of the function whose body holds `offset`.
fn enclosing_function(text: &str, offset: usize) -> (String, Vec<String>) {
    let start = text[..offset]
        .rmatch_indices("fn ")
        .map(|(start, _)| start)
        .find(|&start| start == 0 || !is_identifier_byte(text.as_bytes()[start - 1]))
        .expect("a call lies inside a function");
    let after = &text[start + 3..];
    let name_end = after
        .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .expect("a function has a name");
    let name = after[..name_end].to_string();
    let open = start + 3 + after.find('(').expect("a function has parameters");
    let parameters = parenthesized_items(text, open)
        .into_iter()
        .map(|parameter| {
            let name = parameter.split(':').next().unwrap_or("").trim();
            name.trim_start_matches("mut ").trim().to_string()
        })
        .collect();
    (name, parameters)
}

/// Checks that `argument` comes from the launching driver, following plain
/// parameters back through every production caller of the enclosing function.
fn assert_driver_argument(
    sources: &[Source],
    source: &Source,
    offset: usize,
    argument: &str,
    driver_call: &str,
    depth: usize,
) {
    if argument.ends_with(driver_call) {
        return;
    }
    let (function, parameters) = enclosing_function(&source.text, offset);
    let position = parameters
        .iter()
        .position(|parameter| parameter == argument)
        .unwrap_or_else(|| {
            panic!(
                "{} passes `{argument}` to the project configuration gate in `{function}`; \
                 it must come from the launching driver's `{driver_call}`",
                source.relative
            )
        });
    assert!(
        depth < 4,
        "{} forwards `{argument}` through too many functions to verify",
        source.relative
    );
    let mut callers = 0;
    for caller in sources {
        for call in call_offsets(&caller.text, &function) {
            let arguments = call_arguments(&caller.text, call, &function);
            let forwarded = arguments.get(position).unwrap_or_else(|| {
                panic!(
                    "{} calls `{function}` without its `{argument}` argument",
                    caller.relative
                )
            });
            assert_driver_argument(sources, caller, call, forwarded, driver_call, depth + 1);
            callers += 1;
        }
    }
    assert!(
        callers > 0,
        "{} forwards `{argument}` from `{function}`, which has no production caller",
        source.relative
    );
}

#[test]
fn restricted_run_gate_settles_every_provider_launch_path() {
    let sources = production_sources();
    let launching: BTreeSet<&str> = sources
        .iter()
        .filter(|source| {
            LAUNCH_MARKERS
                .iter()
                .any(|marker| source.text.contains(marker))
        })
        .map(|source| source.relative.as_str())
        .collect();
    let known: BTreeSet<&str> = LAUNCH_SITES.iter().map(|(launch, _)| *launch).collect();
    assert_eq!(
        launching, known,
        "every file that hands a provider a launch folder must settle its project \
         configuration through the provider's own restriction and be listed in LAUNCH_SITES"
    );
    for (launch, settling) in LAUNCH_SITES {
        let source = sources
            .iter()
            .find(|source| source.relative == *settling)
            .unwrap_or_else(|| panic!("{settling} exists"));
        assert!(
            !call_offsets(&source.text, SETTLE_FUNCTION).is_empty(),
            "{settling} must settle the project configuration for the launch in {launch}"
        );
    }
}

#[test]
fn restricted_run_gate_takes_the_restriction_from_the_launching_driver() {
    let sources = production_sources();
    let mut settled = 0;
    for source in &sources {
        for offset in call_offsets(&source.text, SETTLE_FUNCTION) {
            let arguments = call_arguments(&source.text, offset, SETTLE_FUNCTION);
            assert!(
                arguments.len() > READS_ARGUMENT,
                "{} calls {SETTLE_FUNCTION} with too few arguments",
                source.relative
            );
            assert_driver_argument(
                &sources,
                source,
                offset,
                &arguments[RESTRICTION_ARGUMENT],
                DRIVER_RESTRICTION,
                0,
            );
            assert_driver_argument(
                &sources,
                source,
                offset,
                &arguments[READS_ARGUMENT],
                DRIVER_READS,
                0,
            );
            settled += 1;
        }
    }
    assert!(settled >= LAUNCH_SITES.len(), "each launch path settles");
}

#[test]
fn restricted_run_gate_alone_decides_trusted_project_configuration() {
    for source in production_sources() {
        assert!(
            !source
                .text
                .contains("ProjectConfigurationRestriction::Enforced"),
            "{} names an enforced restriction; only the provider driver may claim it",
            source.relative
        );
        if source.relative != GATE_FILE {
            assert!(
                !source.text.contains("ProjectConfigurationAccess::Trusted"),
                "{} grants trusted project configuration outside the gate",
                source.relative
            );
        }
    }
}
