//! Conflicts on theme content: editor saves of one theme on linked devices that did not see each
//! other. Every replica shows the same winner; resolving keeps one version and seals a write that
//! covers every version, so the conflict closes on every device. Keeping both is done by
//! duplicating the theme before picking a version.

use ganbaru_sync::domains::themes::ThemeContent;
use ganbaru_sync::manifest::vault::themes::{THEMES_TABLE, group};
use ganbaru_sync::{ConflictDetail, ConflictVersion, DeviceNames, DeviceRef, Engine, local};
use ganbaru_sync_contracts::{SpaceId, WriterId};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use super::{UserThemeRead, load_one, now_ms, touch_theme, validate_theme_id, write_theme_content};

/// Stable conflict outcomes, independent of diagnostic wording.
#[derive(Debug, Serialize)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum ThemeConflictError {
    /// The conflict or the chosen version no longer exists; reload and show the current state.
    Resolved(String),
    Failed(String),
}

impl From<String> for ThemeConflictError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

/// One token of a version.
#[derive(Debug, Serialize)]
pub struct ThemeConflictToken {
    kind: String,
    key: String,
    value: String,
    isolated: bool,
}

/// One concurrent version of a theme's content.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeConflictVersion {
    /// Opaque version id for resolving.
    version: String,
    device: DeviceRef,
    /// When the version was written, in Unix milliseconds.
    edited_at_ms: u64,
    /// Whether this version is the one every device shows.
    displayed: bool,
    blend_canvas: String,
    derivation_engine_version: i64,
    calendar_default_mode: String,
    calendar_default_custom: String,
    /// Tokens sorted by kind and key.
    tokens: Vec<ThemeConflictToken>,
    /// Event palette in slot order.
    palette: Vec<String>,
}

/// The content versions of a theme in conflict, newest first; empty when it has none.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeConflictRead {
    id: String,
    versions: Vec<ThemeConflictVersion>,
}

/// How to resolve a content conflict.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ThemeConflictChoice {
    /// Keep the displayed version.
    Displayed,
    /// Use another version.
    Version { version: String },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeConflictResolution {
    id: String,
    choice: ThemeConflictChoice,
}

fn content_of(version: &ConflictVersion) -> Result<ThemeContent, String> {
    ThemeContent::from_value(&version.value).map_err(|error| error.to_string())
}

fn version_read(
    version: &ConflictVersion,
    devices: &DeviceNames,
) -> Result<ThemeConflictVersion, String> {
    let content = content_of(version)?;
    Ok(ThemeConflictVersion {
        version: version.writer.to_hex(),
        device: devices.describe(&version.device_id),
        edited_at_ms: version.clock.physical_ms(),
        displayed: version.displayed,
        blend_canvas: content.blend_canvas,
        derivation_engine_version: content.derivation_engine_version,
        calendar_default_mode: content.calendar_default_mode,
        calendar_default_custom: content.calendar_default_custom,
        tokens: content
            .tokens
            .into_iter()
            .map(|token| ThemeConflictToken {
                kind: token.kind,
                key: token.key,
                value: token.value,
                isolated: token.isolated,
            })
            .collect(),
        palette: content.palette,
    })
}

/// The content conflict of a theme, if any.
async fn content_detail(
    tx: &mut Transaction<'_, Sqlite>,
    vault_id: &str,
    id: &str,
) -> Result<Option<ConflictDetail>, ThemeConflictError> {
    let anchor = local::space_anchor(tx)
        .await
        .map_err(|error| format!("read sync space: {error}"))?;
    if anchor.is_none() {
        return Err(ThemeConflictError::Resolved(
            "this vault does not sync".to_string(),
        ));
    }
    let details = Engine::vault()
        .conflict_details(tx, SpaceId::personal(vault_id), THEMES_TABLE, id)
        .await
        .map_err(|error| format!("read theme conflicts: {error}"))?;
    Ok(details
        .into_iter()
        .find(|detail| detail.group == group::CONTENT))
}

/// Reads the content conflict of a theme.
pub async fn conflict_from_pool(
    pool: &SqlitePool,
    vault_id: &str,
    devices: &DeviceNames,
    id: &str,
) -> Result<ThemeConflictRead, ThemeConflictError> {
    validate_theme_id(id)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin theme conflict read: {error}"))?;
    let detail = content_detail(&mut tx, vault_id, id).await?;
    tx.rollback()
        .await
        .map_err(|error| format!("end theme conflict read: {error}"))?;
    let versions = match detail {
        Some(detail) => detail
            .versions
            .iter()
            .map(|version| version_read(version, devices))
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };
    Ok(ThemeConflictRead {
        id: id.to_string(),
        versions,
    })
}

/// Resolves a content conflict. The chosen content is written as a local edit and the content
/// group is sealed even when nothing changed, so the write covers every version it saw. Returns
/// the theme as it now stands.
pub async fn resolve_from_pool(
    pool: &SqlitePool,
    vault_id: &str,
    request: ThemeConflictResolution,
    now: u64,
) -> Result<UserThemeRead, ThemeConflictError> {
    validate_theme_id(&request.id)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin theme conflict resolution: {error}"))?;
    let detail = content_detail(&mut tx, vault_id, &request.id)
        .await?
        .ok_or_else(|| ThemeConflictError::Resolved("the conflict is resolved".to_string()))?;
    if let ThemeConflictChoice::Version { version } = &request.choice {
        let writer = WriterId::from_hex(version);
        let chosen = detail
            .versions
            .iter()
            .find(|candidate| Some(candidate.writer) == writer)
            .ok_or_else(|| ThemeConflictError::Resolved("the version is gone".to_string()))?;
        let content = content_of(chosen)?;
        if write_theme_content(&mut tx, &request.id, &content).await? {
            touch_theme(&mut tx, &request.id, now_ms()?).await?;
        }
    }
    Engine::vault()
        .force_group(&mut tx, THEMES_TABLE, &request.id, group::CONTENT, now)
        .await
        .map_err(|error| format!("mark theme conflict resolved: {error}"))?;
    tx.commit()
        .await
        .map_err(|error| format!("commit theme conflict resolution: {error}"))?;
    Ok(load_one(pool, &request.id).await?)
}
