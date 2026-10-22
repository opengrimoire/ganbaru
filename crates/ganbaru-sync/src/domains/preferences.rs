//! Portable preferences adapter: value checks that mirror the `vault_preferences` table, the
//! profile picture asset reference, and the hook that reports changed preference keys.
//!
//! Preference values are opaque JSON to the engine; which keys exist and which values they
//! accept is application policy, so the adapter never refuses a key it does not know and a
//! newer device can add preferences. The one value shape the engine reads is the profile
//! picture, [`ProfileImage`], whose file every device fetches.

use ganbaru_sync_contracts::{Field, GroupId, TableId, Value};
use serde_json::Value as Json;
use sqlx::SqliteConnection;

use crate::adapter::{AdapterError, DomainAdapter, Presentation};
use crate::assets::{AssetRef, is_sha256_hex};
use crate::hooks::{ChangeSink, HookFuture, MaterializeHook};
use crate::manifest::TableSpec;
use crate::manifest::vault::preferences::{ADAPTER, PREFERENCES_TABLE, PROFILE_CLASS, group};

/// Largest canonical JSON text of one preference value in bytes, as the table checks it.
pub const MAX_VALUE_BYTES: usize = 64 * 1024;

/// Fewest characters of an `updated_at` timestamp, as the table checks it.
pub const MIN_UPDATED_AT_CHARS: usize = 20;

/// Largest profile picture in bytes.
pub const MAX_PROFILE_IMAGE_BYTES: u32 = 3 * 1024 * 1024;

/// Change notice topic of a replicated preference row; the notice key is the preference key.
pub const PREFERENCE_TOPIC: &str = "vault_preferences";

/// Field of a [`ProfileImage`] value naming the file.
pub const PROFILE_IMAGE_PATH_FIELD: &str = "path";

/// Field of a [`ProfileImage`] value holding the file size.
pub const PROFILE_IMAGE_SIZE_FIELD: &str = "byteSize";

/// Directory prefix of a profile picture path, the asset class directory.
const PROFILE_IMAGE_PREFIX: &str = "profile/";

/// Extensions a profile picture may have, with their content types.
const PROFILE_IMAGE_TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("webp", "image/webp"),
];

/// A profile picture value: `{"path": "profile/{sha256}.{ext}", "byteSize": n}`, an object with
/// exactly these two fields. The path names the content-addressed file in the profile asset
/// class and the size is its exact length, which asset transfer checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileImage {
    /// Lowercase hex SHA-256 of the file.
    pub sha256: String,
    /// Media type the extension names.
    pub content_type: &'static str,
    /// Exact size of the file in bytes.
    pub byte_size: u32,
}

impl ProfileImage {
    /// Reads a profile picture value, or `None` when `value` has another shape.
    pub fn from_json(value: &Json) -> Option<Self> {
        let object = value.as_object()?;
        if object.len() != 2 {
            return None;
        }
        let path = object.get(PROFILE_IMAGE_PATH_FIELD)?.as_str()?;
        let byte_size = object.get(PROFILE_IMAGE_SIZE_FIELD)?.as_u64()?;
        let byte_size = u32::try_from(byte_size)
            .ok()
            .filter(|size| (1..=MAX_PROFILE_IMAGE_BYTES).contains(size))?;
        let (sha256, extension) = path.strip_prefix(PROFILE_IMAGE_PREFIX)?.split_once('.')?;
        if !is_sha256_hex(sha256) {
            return None;
        }
        let content_type = PROFILE_IMAGE_TYPES
            .iter()
            .find(|(known, _)| *known == extension)
            .map(|(_, content_type)| *content_type)?;
        Some(Self {
            sha256: sha256.to_owned(),
            content_type,
            byte_size,
        })
    }

    /// The asset reference of the picture.
    pub fn asset_ref(&self) -> Option<AssetRef> {
        AssetRef::new(
            &PROFILE_CLASS,
            self.sha256.clone(),
            self.content_type.to_owned(),
            self.byte_size,
        )
        .ok()
    }
}

/// The JSON value of a preference group value, after the table's checks: text of at most
/// [`MAX_VALUE_BYTES`] that parses as JSON.
fn preference_json(value: &Value) -> Result<Json, AdapterError> {
    let [Field::Text(text)] = value.fields() else {
        return Err(AdapterError("preference value"));
    };
    if text.len() > MAX_VALUE_BYTES {
        return Err(AdapterError("preference value is too long"));
    }
    serde_json::from_str(text).map_err(|_| AdapterError("preference value is not JSON"))
}

/// Adapter of the preferences domain.
#[derive(Debug, Default, Clone, Copy)]
pub struct PreferencesAdapter;

impl DomainAdapter for PreferencesAdapter {
    fn name(&self) -> &'static str {
        ADAPTER
    }

    fn asset_groups(&self, table: TableId) -> &'static [GroupId] {
        if table == PREFERENCES_TABLE {
            &[group::VALUE]
        } else {
            &[]
        }
    }

    /// The profile picture a value names. A value of any other shape references nothing, since
    /// the adapter never sees the row key.
    fn asset_refs(
        &self,
        table: TableId,
        group: GroupId,
        value: &Value,
    ) -> Result<Vec<AssetRef>, AdapterError> {
        if table != PREFERENCES_TABLE || group != group::VALUE {
            return Ok(Vec::new());
        }
        let json = preference_json(value)?;
        Ok(ProfileImage::from_json(&json)
            .and_then(|image| image.asset_ref())
            .into_iter()
            .collect())
    }

    fn derived(
        &self,
        _table: TableId,
        _group: GroupId,
        _value: &Value,
    ) -> Result<Vec<(&'static str, Field)>, AdapterError> {
        Ok(Vec::new())
    }

    fn priority(&self, _table: TableId, _group: GroupId, _value: &Value) -> u8 {
        0
    }

    fn validate(&self, table: TableId, group: GroupId, value: &Value) -> Result<(), AdapterError> {
        if table != PREFERENCES_TABLE {
            return Ok(());
        }
        if group == group::VALUE {
            preference_json(value)?;
        } else if group == group::UPDATED {
            let [Field::Text(updated_at)] = value.fields() else {
                return Err(AdapterError("preference timestamp"));
            };
            if updated_at.chars().count() < MIN_UPDATED_AT_CHARS {
                return Err(AdapterError("preference timestamp is too short"));
            }
        }
        Ok(())
    }

    /// Preference conflicts are never surfaced, so rows present nothing.
    fn presentation(&self, _table: TableId, _values: &[(GroupId, Value)]) -> Presentation {
        Presentation::default()
    }
}

/// Reports every preference key a materialization changed under [`PREFERENCE_TOPIC`], so the
/// app reloads its preference view and tells the native owners that cache a preference.
#[derive(Debug, Default, Clone, Copy)]
pub struct PreferencesHook;

impl MaterializeHook for PreferencesHook {
    fn after_materialize<'a>(
        &'a self,
        _conn: &'a mut SqliteConnection,
        _table: &'static TableSpec,
        changed_keys: &'a [String],
        changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(async move {
            for key in changed_keys {
                changes.report(PREFERENCE_TOPIC, key.clone());
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn text(value: &str) -> Value {
        Value::new(vec![Field::Text(value.to_owned())]).unwrap()
    }

    fn refs(value: &str) -> Vec<AssetRef> {
        PreferencesAdapter
            .asset_refs(PREFERENCES_TABLE, group::VALUE, &text(value))
            .unwrap()
    }

    #[test]
    fn preferences_profile_image_value_references_its_file() {
        let found = refs(&format!(
            r#"{{"path":"profile/{DIGEST}.jpg","byteSize":2048}}"#
        ));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].class(), &PROFILE_CLASS);
        assert_eq!(found[0].sha256(), DIGEST);
        assert_eq!(found[0].content_type(), "image/jpeg");
        assert_eq!(found[0].byte_size(), 2048);
        assert_eq!(found[0].file_name(), format!("{DIGEST}.jpg"));
    }

    #[test]
    fn preferences_other_values_reference_nothing() {
        let upper = DIGEST.to_ascii_uppercase();
        for value in [
            "true".to_owned(),
            "7".to_owned(),
            format!(r#""profile/{DIGEST}.png""#),
            format!(r#"{{"path":"profile/{DIGEST}.png"}}"#),
            format!(r#"{{"path":"profile/{DIGEST}.png","byteSize":0}}"#),
            format!(
                r#"{{"path":"profile/{DIGEST}.png","byteSize":{}}}"#,
                MAX_PROFILE_IMAGE_BYTES + 1
            ),
            format!(r#"{{"path":"profile/{DIGEST}.svg","byteSize":10}}"#),
            format!(r#"{{"path":"profile/{DIGEST}.jpeg","byteSize":10}}"#),
            format!(r#"{{"path":"profile/{upper}.png","byteSize":10}}"#),
            format!(r#"{{"path":"other/{DIGEST}.png","byteSize":10}}"#),
            format!(r#"{{"path":"profile/{DIGEST}.png","byteSize":10,"extra":1}}"#),
        ] {
            assert!(refs(&value).is_empty(), "{value}");
        }
    }

    #[test]
    fn preferences_validation_mirrors_the_table_checks() {
        let adapter = PreferencesAdapter;
        let valid = |group, value: &Value| adapter.validate(PREFERENCES_TABLE, group, value);
        assert_eq!(valid(group::VALUE, &text(r#"{"a":[1,2]}"#)), Ok(()));
        assert_eq!(valid(group::VALUE, &text("\"es\"")), Ok(()));
        assert!(valid(group::VALUE, &text("not json")).is_err());
        let long = format!("\"{}\"", "a".repeat(MAX_VALUE_BYTES - 1));
        assert!(valid(group::VALUE, &text(&long)).is_err());
        let fits = format!("\"{}\"", "a".repeat(MAX_VALUE_BYTES - 2));
        assert_eq!(valid(group::VALUE, &text(&fits)), Ok(()));
        assert_eq!(
            valid(group::UPDATED, &text("2026-10-22T10:51:41.000Z")),
            Ok(())
        );
        assert!(valid(group::UPDATED, &text("2026-10-22")).is_err());
    }
}
