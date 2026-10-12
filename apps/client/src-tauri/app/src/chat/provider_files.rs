//! Bounded access to documented coding-agent configuration and instruction files.

use super::settings_commands::read_provider;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProviderInstanceConfig, ProviderInstanceId,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};

const MAX_PROVIDER_FILE_BYTES: u64 = 1024 * 1024;
static PROVIDER_FILE_WRITE_GENERATION: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
pub struct ProviderFileState {
    mutation_lock: Mutex<()>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFileKind {
    Configuration,
    Instructions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFileFormat {
    Toml,
    Json,
    Jsonc,
    Markdown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderFileRead {
    pub file_id: String,
    pub name: String,
    pub kind: ProviderFileKind,
    pub format: ProviderFileFormat,
    pub path: String,
    pub exists: bool,
    pub contents: String,
    pub revision: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProviderFileRequest {
    pub instance_id: ProviderInstanceId,
    pub file_id: String,
    pub contents: String,
    pub expected_revision: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProviderFileSpec {
    file_id: String,
    kind: ProviderFileKind,
    format: ProviderFileFormat,
    path: PathBuf,
}

#[tauri::command]
pub fn chat_read_provider_files(
    app: tauri::AppHandle,
    instance_id: ProviderInstanceId,
) -> ChatResult<Vec<ProviderFileRead>> {
    let provider = read_provider(&app, &instance_id)?;
    provider_file_specs(&provider.configuration)?
        .into_iter()
        .map(read_provider_file)
        .collect()
}

#[tauri::command]
pub fn chat_save_provider_file(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProviderFileState>,
    request: SaveProviderFileRequest,
) -> ChatResult<ProviderFileRead> {
    validate_contents(&request.contents)?;
    let _guard = state
        .mutation_lock
        .lock()
        .map_err(|_| provider_file_error())?;
    let provider = read_provider(&app, &request.instance_id)?;
    let spec = provider_file_specs(&provider.configuration)?
        .into_iter()
        .find(|spec| spec.file_id == request.file_id)
        .ok_or_else(|| ChatError::validation("fileId", "Provider file is not supported"))?;
    let current = read_provider_file(spec.clone())?;
    if current.revision != request.expected_revision {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "The provider file changed outside Ganbaru. Reload it before saving.",
            true,
        ));
    }
    reject_symbolic_path(&spec.path)?;
    write_text_file_atomically(&spec.path, &request.contents)?;
    read_provider_file(spec)
}

fn provider_file_specs(
    configuration: &ProviderInstanceConfig,
) -> ChatResult<Vec<ProviderFileSpec>> {
    let family = configuration.family_id.as_str();
    let home = provider_home(configuration)?;
    let mut specs = match family {
        "codex" => standard_pair(&home, "config.toml", ProviderFileFormat::Toml, "AGENTS.md"),
        "claude" => standard_pair(
            &home,
            "settings.json",
            ProviderFileFormat::Json,
            "CLAUDE.md",
        ),
        "cursor" => vec![configuration_spec(
            "configuration",
            home.join("cli-config.json"),
            ProviderFileFormat::Json,
        )],
        "grok" => standard_pair(&home, "config.toml", ProviderFileFormat::Toml, "AGENTS.md"),
        "opencode" => opencode_specs(configuration, &home)?,
        _ => {
            return Err(ChatError::unsupported(
                "This provider does not expose documented editable files",
            ));
        }
    };
    specs.sort_by(|left, right| left.file_id.cmp(&right.file_id));
    Ok(specs)
}

fn standard_pair(
    home: &Path,
    configuration_name: &str,
    configuration_format: ProviderFileFormat,
    instructions_name: &str,
) -> Vec<ProviderFileSpec> {
    vec![
        configuration_spec(
            "configuration",
            home.join(configuration_name),
            configuration_format,
        ),
        ProviderFileSpec {
            file_id: "instructions".to_string(),
            kind: ProviderFileKind::Instructions,
            format: ProviderFileFormat::Markdown,
            path: home.join(instructions_name),
        },
    ]
}

fn configuration_spec(
    file_id: &str,
    path: PathBuf,
    format: ProviderFileFormat,
) -> ProviderFileSpec {
    ProviderFileSpec {
        file_id: file_id.to_string(),
        kind: ProviderFileKind::Configuration,
        format,
        path,
    }
}

fn opencode_specs(
    configuration: &ProviderInstanceConfig,
    home: &Path,
) -> ChatResult<Vec<ProviderFileSpec>> {
    if let Some(custom_path) = configured_environment(configuration, "OPENCODE_CONFIG") {
        let path = resolve_absolute_path(&custom_path, "OPENCODE_CONFIG")?;
        return Ok(vec![
            configuration_spec("configuration", path.clone(), json_format(&path)),
            ProviderFileSpec {
                file_id: "instructions".to_string(),
                kind: ProviderFileKind::Instructions,
                format: ProviderFileFormat::Markdown,
                path: home.join("AGENTS.md"),
            },
        ]);
    }

    let json = home.join("opencode.json");
    let jsonc = home.join("opencode.jsonc");
    let mut specs = Vec::new();
    if json.exists() || !jsonc.exists() {
        specs.push(configuration_spec(
            "configuration",
            json,
            ProviderFileFormat::Json,
        ));
    }
    if jsonc.exists() {
        specs.push(configuration_spec(
            if specs.is_empty() {
                "configuration"
            } else {
                "configuration_jsonc"
            },
            jsonc,
            ProviderFileFormat::Jsonc,
        ));
    }
    specs.push(ProviderFileSpec {
        file_id: "instructions".to_string(),
        kind: ProviderFileKind::Instructions,
        format: ProviderFileFormat::Markdown,
        path: home.join("AGENTS.md"),
    });
    Ok(specs)
}

fn provider_home(configuration: &ProviderInstanceConfig) -> ChatResult<PathBuf> {
    let family = configuration.family_id.as_str();
    let configured = configuration
        .provider_home
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let value = match family {
        "codex" => configured
            .map(str::to_string)
            .or_else(|| process_environment("CODEX_HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home().join(".codex")),
        "claude" => configured
            .map(str::to_string)
            .or_else(|| process_environment("CLAUDE_CONFIG_DIR"))
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home().join(".claude")),
        "cursor" => user_home().join(".cursor"),
        "grok" => configured
            .map(str::to_string)
            .or_else(|| configured_environment(configuration, "GROK_HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home().join(".grok")),
        "opencode" => configured
            .map(PathBuf::from)
            .or_else(|| {
                configured_environment(configuration, "OPENCODE_CONFIG_DIR").map(PathBuf::from)
            })
            .or_else(|| {
                configured_environment(configuration, "XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .map(|path| path.join("opencode"))
            })
            .unwrap_or_else(|| user_home().join(".config").join("opencode")),
        _ => return Err(ChatError::unsupported("Provider files are unavailable")),
    };
    resolve_absolute_path(value.to_string_lossy().as_ref(), "providerHome")
}

fn configured_environment(configuration: &ProviderInstanceConfig, name: &str) -> Option<String> {
    let value = configuration.environment.get(name)?.trim();
    if value == format!("inherit:{name}") {
        process_environment(name)
    } else if value.is_empty() || value.starts_with("inherit:") {
        None
    } else {
        Some(value.to_string())
    }
}

fn process_environment(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn user_home() -> PathBuf {
    #[cfg(windows)]
    let names = ["USERPROFILE", "HOME"];
    #[cfg(not(windows))]
    let names = ["HOME", "USERPROFILE"];
    names
        .iter()
        .find_map(|name| std::env::var_os(name).filter(|value| !value.is_empty()))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn resolve_absolute_path(value: &str, field: &str) -> ChatResult<PathBuf> {
    let path = if value == "~" || value.starts_with("~/") || value.starts_with("~\\") {
        if value.len() == 1 {
            user_home()
        } else {
            user_home().join(&value[2..])
        }
    } else {
        PathBuf::from(value)
    };
    if !path.is_absolute() || path.components().any(|part| part == Component::ParentDir) {
        return Err(ChatError::validation(
            field,
            "Provider file path must be absolute",
        ));
    }
    Ok(path)
}

fn read_provider_file(spec: ProviderFileSpec) -> ChatResult<ProviderFileRead> {
    reject_symbolic_path(&spec.path)?;
    let exists = spec.path.exists();
    let contents = if exists {
        read_bounded_utf8(&spec.path)?
    } else {
        String::new()
    };
    Ok(ProviderFileRead {
        file_id: spec.file_id,
        name: spec
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or_else(provider_file_error)?,
        kind: spec.kind,
        format: spec.format,
        path: spec.path.to_string_lossy().into_owned(),
        exists,
        revision: revision(&contents, exists),
        contents,
    })
}

fn read_bounded_utf8(path: &Path) -> ChatResult<String> {
    let metadata = fs::metadata(path).map_err(|_| provider_file_error())?;
    if !metadata.is_file() || metadata.len() > MAX_PROVIDER_FILE_BYTES {
        return Err(ChatError::validation(
            "providerFile",
            "Provider file must be a UTF-8 text file no larger than 1 MiB",
        ));
    }
    let mut file = File::open(path).map_err(|_| provider_file_error())?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_PROVIDER_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| provider_file_error())?;
    if bytes.len() as u64 > MAX_PROVIDER_FILE_BYTES {
        return Err(ChatError::validation(
            "providerFile",
            "Provider file exceeds the 1 MiB limit",
        ));
    }
    String::from_utf8(bytes).map_err(|_| {
        ChatError::validation("providerFile", "Provider file must contain valid UTF-8")
    })
}

fn validate_contents(contents: &str) -> ChatResult<()> {
    if contents.len() as u64 > MAX_PROVIDER_FILE_BYTES || contents.contains('\0') {
        return Err(ChatError::validation(
            "contents",
            "Provider file must be UTF-8 text no larger than 1 MiB",
        ));
    }
    Ok(())
}

fn revision(contents: &str, exists: bool) -> String {
    let mut digest = Sha256::new();
    digest.update(if exists {
        b"provider-file-v1\0".as_slice()
    } else {
        b"provider-file-missing-v1\0".as_slice()
    });
    digest.update(contents.as_bytes());
    format!("{:x}", digest.finalize())
}

fn reject_symbolic_path(path: &Path) -> ChatResult<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ChatError::validation(
                    "providerFile",
                    "Symbolic links are not editable from Chat settings",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err(provider_file_error()),
        }
    }
    Ok(())
}

fn write_text_file_atomically(path: &Path, contents: &str) -> ChatResult<()> {
    let parent = path.parent().ok_or_else(provider_file_error)?;
    fs::create_dir_all(parent).map_err(|_| provider_file_error())?;
    reject_symbolic_path(parent)?;
    reject_symbolic_path(path)?;
    let file_name = path
        .file_name()
        .ok_or_else(provider_file_error)?
        .to_string_lossy();
    let generation = PROVIDER_FILE_WRITE_GENERATION.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".{file_name}.ganbaru.{}.{}.tmp",
        std::process::id(),
        generation
    ));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|_| provider_file_error())?;
        file.write_all(contents.as_bytes())
            .map_err(|_| provider_file_error())?;
        file.sync_all().map_err(|_| provider_file_error())?;
        replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(windows))]
fn replace_file(temporary: &Path, target: &Path) -> ChatResult<()> {
    fs::rename(temporary, target).map_err(|_| provider_file_error())
}

#[cfg(windows)]
fn replace_file(temporary: &Path, target: &Path) -> ChatResult<()> {
    if !target.exists() {
        return fs::rename(temporary, target).map_err(|_| provider_file_error());
    }
    let generation = PROVIDER_FILE_WRITE_GENERATION.fetch_add(1, Ordering::Relaxed);
    let backup = target.with_extension(format!(
        "ganbaru.{}.{}.backup",
        std::process::id(),
        generation
    ));
    fs::rename(target, &backup).map_err(|_| provider_file_error())?;
    if fs::rename(temporary, target).is_err() {
        let _ = fs::rename(&backup, target);
        return Err(provider_file_error());
    }
    fs::remove_file(backup).map_err(|_| provider_file_error())
}

fn json_format(path: &Path) -> ProviderFileFormat {
    if path.extension().and_then(|value| value.to_str()) == Some("jsonc") {
        ProviderFileFormat::Jsonc
    } else {
        ProviderFileFormat::Json
    }
}

fn provider_file_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Provider file could not be read or saved",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ganbaru_chat_contracts::models::{ProviderFamilyId, VersionedJson};
    use std::collections::BTreeMap;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let generation = PROVIDER_FILE_WRITE_GENERATION.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "ganbaru-provider-files-{label}-{}-{generation}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn configuration(family: &str, home: &Path) -> ProviderInstanceConfig {
        ProviderInstanceConfig {
            schema_version: 1,
            instance_id: ProviderInstanceId::new(format!("{family}-test")).unwrap(),
            family_id: ProviderFamilyId::new(family).unwrap(),
            label: family.to_string(),
            enabled: true,
            executable: family.to_string(),
            provider_home: Some(home.to_string_lossy().into_owned()),
            launch_arguments: Vec::new(),
            environment: BTreeMap::new(),
            credential_references: BTreeMap::new(),
            visible_model_ids: Vec::new(),
            favorite_model_ids: Vec::new(),
            provider_config: VersionedJson {
                schema_version: 1,
                value: serde_json::json!({}),
            },
            internal_mcp: None,
            unknown_fields: BTreeMap::new(),
        }
    }

    #[test]
    fn provider_specs_keep_each_family_in_its_documented_home() {
        let root = PathBuf::from("/provider-home");
        let cases = [
            ("codex", "config.toml", Some("AGENTS.md")),
            ("claude", "settings.json", Some("CLAUDE.md")),
            ("grok", "config.toml", Some("AGENTS.md")),
        ];
        for (family, config_name, instruction_name) in cases {
            let specs = provider_file_specs(&configuration(family, &root)).unwrap();
            assert!(specs.iter().any(|spec| spec.path == root.join(config_name)));
            assert!(
                specs
                    .iter()
                    .any(|spec| spec.path == root.join(instruction_name.unwrap()))
            );
        }
    }

    #[test]
    fn cursor_exposes_only_its_documented_cli_configuration() {
        let specs = provider_file_specs(&configuration("cursor", Path::new("/ignored"))).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].file_id, "configuration");
        assert!(specs[0].path.ends_with(".cursor/cli-config.json"));
    }

    #[test]
    fn opencode_honors_its_config_directory_and_explicit_config_file() {
        let root = TestDirectory::new("opencode");
        let mut configured_home = configuration("opencode", root.path());
        let specs = provider_file_specs(&configured_home).unwrap();
        assert!(
            specs
                .iter()
                .any(|spec| spec.path == root.path().join("opencode.json"))
        );
        assert!(
            specs
                .iter()
                .any(|spec| spec.path == root.path().join("AGENTS.md"))
        );

        let custom = root.path().join("custom.jsonc");
        configured_home.environment.insert(
            "OPENCODE_CONFIG".to_string(),
            custom.to_string_lossy().into_owned(),
        );
        let specs = provider_file_specs(&configured_home).unwrap();
        let config = specs
            .iter()
            .find(|spec| spec.kind == ProviderFileKind::Configuration)
            .unwrap();
        assert_eq!(config.path, custom);
        assert_eq!(config.format, ProviderFileFormat::Jsonc);
    }

    #[test]
    fn read_and_write_preserve_revisions_and_missing_state() {
        let root = TestDirectory::new("round-trip");
        let spec = configuration_spec(
            "configuration",
            root.path().join("config.toml"),
            ProviderFileFormat::Toml,
        );
        let missing = read_provider_file(spec.clone()).unwrap();
        assert!(!missing.exists);
        write_text_file_atomically(&spec.path, "approval_policy = \"on-request\"\n").unwrap();
        let saved = read_provider_file(spec).unwrap();
        assert!(saved.exists);
        assert_eq!(saved.contents, "approval_policy = \"on-request\"\n");
        assert_ne!(saved.revision, missing.revision);
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_parent_directories_are_rejected() {
        use std::os::unix::fs::symlink;

        let root = TestDirectory::new("symlink");
        let real = root.path().join("real");
        fs::create_dir_all(&real).unwrap();
        let linked = root.path().join("linked");
        symlink(&real, &linked).unwrap();
        let error = reject_symbolic_path(&linked.join("config.toml")).unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
    }

    #[test]
    fn revisions_distinguish_missing_and_empty_files() {
        assert_ne!(revision("", false), revision("", true));
        assert_ne!(revision("first", true), revision("second", true));
    }
}
