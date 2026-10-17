use super::*;
use notify::event::{CreateKind, RenameMode};
use std::time::{SystemTime, UNIX_EPOCH};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock should be valid")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ganbaru-workspace-observer-test-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(path.join("src")).expect("test directory should be created");
        Self(path.canonicalize().expect("test path should canonicalize"))
    }

    fn scope(&self) -> ObserverScope {
        ObserverScope {
            working_folder_id: ProjectWorkingFolderId::new("workspace:observer-test")
                .expect("workspace ID should be valid"),
            execution_environment_id: Some("environment:test".to_string()),
            root: self.0.clone(),
        }
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn accumulator_coalesces_paths_parents_and_renames() {
    let directory = TestDirectory::new();
    fs::write(directory.0.join("src/old.rs"), "old\n").expect("old file should write");
    fs::write(directory.0.join("src/new.rs"), "new\n").expect("new file should write");
    let mut accumulator = WorkspaceChangeAccumulator::new(directory.scope(), 7, Vec::new());
    accumulator.push(ObserverMessage::Event(
        Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(directory.0.join("src/old.rs"))
            .add_path(directory.0.join("src/new.rs")),
    ));

    let batch = accumulator.finish().expect("batch should be emitted");
    assert_eq!(batch.relative_paths, ["src/new.rs", "src/old.rs"]);
    assert_eq!(batch.affected_parent_directories, ["src"]);
    assert_eq!(
        batch.renames,
        [ChatWorkspaceRename {
            previous_relative_path: "src/old.rs".to_string(),
            relative_path: "src/new.rs".to_string(),
        }]
    );
}

#[test]
fn accumulator_filters_generated_symlink_and_git_paths() {
    let directory = TestDirectory::new();
    fs::create_dir_all(directory.0.join("node_modules/pkg"))
        .expect("generated directory should exist");
    fs::create_dir_all(directory.0.join(".git/refs/heads")).expect("Git directory should exist");
    fs::write(directory.0.join("src/visible.rs"), "visible\n").expect("visible file should write");
    fs::write(directory.0.join("node_modules/pkg/index.js"), "ignored\n")
        .expect("generated file should write");
    fs::write(directory.0.join(".git/index"), "index\n").expect("Git index should write");
    let watched = discover_workspace_watch_directories(&directory.0)
        .expect("workspace directories should be discovered");
    assert!(watched.contains(&directory.0));
    assert!(watched.contains(&directory.0.join("src")));
    assert!(!watched.contains(&directory.0.join("node_modules")));
    assert!(!watched.contains(&directory.0.join(".git")));
    let mut accumulator =
        WorkspaceChangeAccumulator::new(directory.scope(), 1, vec![directory.0.join(".git")]);
    for path in [
        directory.0.join("src/visible.rs"),
        directory.0.join("node_modules/pkg/index.js"),
        directory.0.join(".git/index"),
    ] {
        accumulator.push(ObserverMessage::Event(
            Event::new(EventKind::Create(CreateKind::File)).add_path(path),
        ));
    }

    let batch = accumulator.finish().expect("batch should be emitted");
    assert_eq!(batch.relative_paths, ["src/visible.rs"]);
    assert!(batch.git_metadata_changed);
}

#[test]
fn accumulator_bounds_unique_paths_and_marks_overflow() {
    let directory = TestDirectory::new();
    let mut accumulator = WorkspaceChangeAccumulator::new(directory.scope(), 2, Vec::new());
    for index in 0..=MAX_BATCH_PATHS {
        accumulator.push(ObserverMessage::Internal {
            relative_paths: vec![format!("file-{index}.txt")],
            git_metadata_changed: false,
        });
    }

    let batch = accumulator.finish().expect("batch should be emitted");
    assert_eq!(batch.relative_paths.len(), MAX_BATCH_PATHS);
    assert!(batch.overflowed);
}

#[test]
fn accumulator_bounds_rename_metadata_and_marks_overflow() {
    let directory = TestDirectory::new();
    let mut accumulator = WorkspaceChangeAccumulator::new(directory.scope(), 3, Vec::new());
    for index in 0..=MAX_BATCH_PATHS {
        accumulator.push(ObserverMessage::Event(
            Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                .add_path(directory.0.join(format!("old-{index}.txt")))
                .add_path(directory.0.join(format!("new-{index}.txt"))),
        ));
    }

    let batch = accumulator.finish().expect("batch should be emitted");
    assert_eq!(batch.renames.len(), MAX_BATCH_PATHS);
    assert!(batch.overflowed);
}
