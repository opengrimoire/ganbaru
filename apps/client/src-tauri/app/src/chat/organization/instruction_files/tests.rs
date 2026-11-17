use super::*;
use std::path::PathBuf;

/// A private temporary folder removed with the value.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let created = ganbaru_atomic_files::create_private_temporary_directory(
            &std::env::temp_dir(),
            "ganbaru-instruction-files-test-",
        )
        .unwrap();
        Self(fs::canonicalize(created).unwrap())
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn labels(files: &[InstructionFile]) -> Vec<(InstructionScope, &str)> {
    files
        .iter()
        .map(|file| (file.scope, file.label.as_str()))
        .collect()
}

fn file(label: &str, text: &str) -> InstructionFile {
    InstructionFile {
        scope: InstructionScope::Project,
        label: label.to_string(),
        text: text.to_string(),
    }
}

#[test]
fn only_providers_that_skip_files_in_teammate_runs_get_them_as_text() {
    assert_eq!(
        InstructionFileFamily::from_family_id("claude"),
        Some(InstructionFileFamily::Claude)
    );
    assert_eq!(
        InstructionFileFamily::from_family_id("grok"),
        Some(InstructionFileFamily::Grok)
    );
    assert_eq!(
        InstructionFileFamily::from_family_id("opencode"),
        Some(InstructionFileFamily::OpenCode)
    );
    for native in ["codex", "cursor", "acp"] {
        assert_eq!(InstructionFileFamily::from_family_id(native), None);
    }
}

#[test]
fn claude_reads_user_then_project_files_in_load_order() {
    let home = TempDir::new();
    home.write("CLAUDE.md", "User memory");
    home.write("rules/b.md", "User rule b");
    home.write("rules/a/nested.md", "Nested user rule");
    home.write(
        "rules/scoped.md",
        "---\npaths:\n  - src/**\n---\nScoped rule",
    );
    home.write("rules/notes.txt", "Not a rule");
    let folder = TempDir::new();
    folder.write("CLAUDE.md", "Project memory");
    folder.write(".claude/CLAUDE.md", "Hidden project memory");
    folder.write("CLAUDE.local.md", "Local memory");
    folder.write("AGENTS.md", "Shadowed by CLAUDE.md");
    folder.write(".claude/rules/style.md", "Project rule");

    let files = collect_instruction_files(
        InstructionFileFamily::Claude,
        Some(&home.0),
        Some(&folder.0),
    );

    assert_eq!(
        labels(&files),
        vec![
            (InstructionScope::User, "CLAUDE.md"),
            (InstructionScope::User, "rules/a/nested.md"),
            (InstructionScope::User, "rules/b.md"),
            (InstructionScope::Project, "CLAUDE.md"),
            (InstructionScope::Project, ".claude/CLAUDE.md"),
            (InstructionScope::Project, "CLAUDE.local.md"),
            (InstructionScope::Project, ".claude/rules/style.md"),
        ]
    );
    assert_eq!(files[0].text, "User memory");
}

#[test]
fn claude_falls_back_to_agents_md_only_without_a_claude_file() {
    let folder = TempDir::new();
    folder.write("AGENTS.md", "Shared instructions");

    let files = collect_instruction_files(InstructionFileFamily::Claude, None, Some(&folder.0));

    assert_eq!(
        labels(&files),
        vec![(InstructionScope::Project, "AGENTS.md")]
    );
}

#[test]
fn grok_reads_every_supported_name_and_top_level_rules() {
    let folder = TempDir::new();
    folder.write("AGENTS.md", "Agents");
    folder.write("CLAUDE.md", "Claude");
    folder.write(".claude/CLAUDE.local.md", "Claude local");
    folder.write(".grok/rules/one.md", "Grok rule");
    folder.write(".grok/rules/deeper/two.md", "Not scanned");
    folder.write(".cursor/rules/three.md", "Cursor rule");

    let files = collect_instruction_files(InstructionFileFamily::Grok, None, Some(&folder.0));

    assert_eq!(
        labels(&files),
        vec![
            (InstructionScope::Project, "CLAUDE.md"),
            (InstructionScope::Project, "AGENTS.md"),
            (InstructionScope::Project, ".claude/CLAUDE.local.md"),
            (InstructionScope::Project, ".grok/rules/one.md"),
            (InstructionScope::Project, ".cursor/rules/three.md"),
        ]
    );
}

#[test]
fn opencode_reads_only_the_first_project_file_that_exists() {
    let folder = TempDir::new();
    folder.write("CLAUDE.md", "Claude");
    folder.write("CONTEXT.md", "Context");

    let files = collect_instruction_files(InstructionFileFamily::OpenCode, None, Some(&folder.0));

    assert_eq!(
        labels(&files),
        vec![(InstructionScope::Project, "CLAUDE.md")]
    );
}

#[test]
fn a_run_without_a_working_folder_gets_only_user_files() {
    let home = TempDir::new();
    home.write("CLAUDE.md", "User memory");

    assert_eq!(
        collect_instruction_files(InstructionFileFamily::Claude, Some(&home.0), None).len(),
        1
    );
    assert!(collect_instruction_files(InstructionFileFamily::OpenCode, None, None).is_empty());
}

#[cfg(unix)]
#[test]
fn project_symlinks_stay_inside_the_working_folder() {
    use std::os::unix::fs::symlink;

    let outside = TempDir::new();
    outside.write("secret.md", "Outside the folder");
    outside.write("rules/leak.md", "Outside rule");
    let folder = TempDir::new();
    folder.write("AGENTS.md", "Shared instructions");
    symlink(folder.0.join("AGENTS.md"), folder.0.join("CLAUDE.md")).unwrap();
    symlink(
        outside.0.join("secret.md"),
        folder.0.join("CLAUDE.local.md"),
    )
    .unwrap();
    fs::create_dir_all(folder.0.join(".claude")).unwrap();
    symlink(outside.0.join("rules"), folder.0.join(".claude/rules")).unwrap();

    let files = collect_instruction_files(InstructionFileFamily::Claude, None, Some(&folder.0));

    assert_eq!(
        labels(&files),
        vec![(InstructionScope::Project, "CLAUDE.md")]
    );
    assert_eq!(files[0].text, "Shared instructions");
}

#[cfg(unix)]
#[test]
fn a_symlinked_project_folder_is_never_traversed() {
    use std::os::unix::fs::symlink;

    let outside = TempDir::new();
    outside.write("CLAUDE.md", "Outside memory");
    let folder = TempDir::new();
    symlink(&outside.0, folder.0.join(".claude")).unwrap();

    let files = collect_instruction_files(InstructionFileFamily::Claude, None, Some(&folder.0));

    assert!(files.is_empty());
}

#[test]
fn identical_files_are_sent_once() {
    let folder = TempDir::new();
    folder.write("AGENTS.md", "Same text\n");
    folder.write("CLAUDE.md", "Same text");

    let files = collect_instruction_files(InstructionFileFamily::Grok, None, Some(&folder.0));

    assert_eq!(
        labels(&files),
        vec![(InstructionScope::Project, "CLAUDE.md")]
    );
}

#[test]
fn rules_name_paths_only_in_front_matter() {
    assert!(names_paths("---\npaths:\n  - src/**\n---\nBody"));
    assert!(!names_paths(
        "---\ndescription: Style\n---\npaths: in the body"
    ));
    assert!(!names_paths("paths: without front matter"));
}

#[test]
fn appending_nothing_keeps_the_instructions_unchanged() {
    assert_eq!(append_instruction_files("Base", &[]), "Base");
}

#[test]
fn appended_files_are_labeled_by_scope_and_path() {
    let files = [
        InstructionFile {
            scope: InstructionScope::User,
            label: "CLAUDE.md".to_string(),
            text: "Be brief.".to_string(),
        },
        file("AGENTS.md", "Run the tests."),
    ];

    let composed = append_instruction_files("Base", &files);

    assert!(composed.starts_with("Base\n\nThe user keeps the instruction files below"));
    assert!(composed.contains("## User instructions: CLAUDE.md\n\nBe brief."));
    assert!(composed.ends_with("## Project instructions: AGENTS.md\n\nRun the tests."));
}

#[test]
fn files_past_the_budget_are_cut_and_counted() {
    let large = "a".repeat(MAX_INSTRUCTION_FILES_BYTES);
    let files = [
        file("AGENTS.md", "Short"),
        file("CLAUDE.md", &large),
        file("CONTEXT.md", "Never reached"),
    ];

    let composed = append_instruction_files("Base", &files);

    assert!(composed.len() - "Base".len() <= MAX_INSTRUCTION_FILES_BYTES);
    assert!(composed.contains(TRUNCATED_MARKER));
    assert!(
        composed
            .ends_with("[1 more instruction files were left out to fit the instruction limit.]")
    );
    assert!(!composed.contains("Never reached"));
}

#[test]
fn files_never_push_instructions_past_the_developer_cap() {
    let base = "b".repeat(MAX_DEVELOPER_INSTRUCTIONS_BYTES - 1024);
    let multibyte = "é".repeat(4096);
    let files = [file("AGENTS.md", &multibyte)];

    let composed = append_instruction_files(&base, &files);

    assert!(composed.len() <= MAX_DEVELOPER_INSTRUCTIONS_BYTES);
    assert!(composed.contains(TRUNCATED_MARKER));
}

#[test]
fn files_are_dropped_when_no_useful_room_is_left() {
    let base = "b".repeat(MAX_DEVELOPER_INSTRUCTIONS_BYTES - 64);
    let files = [file("AGENTS.md", "Run the tests.")];

    assert_eq!(append_instruction_files(&base, &files), base);
}
