//! Managed assets: content-addressed files under the vault's `assets/` directory that
//! replicated values reference by digest.
//!
//! Each domain declares its [`AssetClass`]es. A class is a directory under [`ASSETS_DIR`] with a
//! byte bound, and every class names its files `{sha256}.{ext}` through [`file_name`], so one
//! function owns asset paths. A domain adapter reports the references a group value holds
//! ([`crate::DomainAdapter::asset_refs`]), and [`crate::Engine::referenced_assets`] lists every
//! reference of live rows, open conflicts, and recovery offers, which sync moves between devices.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use ganbaru_sync_contracts::{GroupId, Value};
use sqlx::{AssertSqlSafe, SqliteConnection};

use crate::Engine;
use crate::error::{SyncResult, corrupt};
use crate::local::retained_group_values;
use crate::manifest::{DomainManifest, GroupSpec, TableSpec};
use crate::seal::read_group_value;
use crate::sql::ident;

/// Vault-relative directory holding every asset class.
pub const ASSETS_DIR: &str = "assets";

/// Largest asset any class may declare, in bytes. The sync wire carries assets up to the same
/// bound.
pub const MAX_ASSET_BYTES: u32 = 64 * 1024 * 1024;

/// Characters of a lowercase hex SHA-256 digest.
pub const SHA256_HEX_CHARS: usize = 64;

/// Largest content type in bytes.
pub const MAX_CONTENT_TYPE_BYTES: usize = 64;

/// Largest class name in bytes.
pub const MAX_CLASS_NAME_BYTES: usize = 64;

/// Extension of an asset whose content type has no entry in [`CONTENT_TYPE_EXTENSIONS`].
pub const UNKNOWN_EXTENSION: &str = "bin";

/// File extensions by content type. The table only grows with types no class accepts yet, so a
/// stored file keeps its name.
pub const CONTENT_TYPE_EXTENSIONS: &[(&str, &str)] = &[
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/webp", "webp"),
    ("image/gif", "gif"),
];

/// A kind of managed file a domain stores, such as Quick note images.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetClass {
    /// Directory under [`ASSETS_DIR`], with `/` separating lowercase segments, for example
    /// `quick-notes/images`.
    pub name: &'static str,
    /// Largest file of the class in bytes, at most [`MAX_ASSET_BYTES`].
    pub max_bytes: u32,
}

impl AssetClass {
    /// The directory of the class in a vault.
    pub fn directory(&self, vault_root: &Path) -> PathBuf {
        self.name
            .split('/')
            .fold(vault_root.join(ASSETS_DIR), |path, segment| {
                path.join(segment)
            })
    }

    /// Why the class cannot be registered, if it cannot.
    pub(crate) fn problem(&self) -> Option<&'static str> {
        let segments_valid = self.name.split('/').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        });
        if self.name.is_empty() || self.name.len() > MAX_CLASS_NAME_BYTES || !segments_valid {
            Some("class name is not a relative path of lowercase segments")
        } else if self.max_bytes == 0 || self.max_bytes > MAX_ASSET_BYTES {
            Some("class byte bound is outside 1 to the asset limit")
        } else {
            None
        }
    }
}

/// An asset reference the engine refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetError {
    /// The digest is not a lowercase hex SHA-256.
    Digest,
    /// The content type is empty, too long, or not printable ASCII.
    ContentType,
    /// The size is zero or above the class bound.
    Size,
}

impl fmt::Display for AssetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Digest => "asset digest is invalid",
            Self::ContentType => "asset content type is invalid",
            Self::Size => "asset size is outside its class bound",
        })
    }
}

impl std::error::Error for AssetError {}

/// Whether a value is a lowercase hex SHA-256 digest.
pub fn is_sha256_hex(value: &str) -> bool {
    value.len() == SHA256_HEX_CHARS
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn content_type_valid(content_type: &str) -> bool {
    !content_type.is_empty()
        && content_type.len() <= MAX_CONTENT_TYPE_BYTES
        && content_type.bytes().all(|byte| byte.is_ascii_graphic())
}

/// Extension of a content type, [`UNKNOWN_EXTENSION`] when the table has no entry.
pub fn extension(content_type: &str) -> &'static str {
    CONTENT_TYPE_EXTENSIONS
        .iter()
        .find(|(known, _)| *known == content_type)
        .map_or(UNKNOWN_EXTENSION, |(_, extension)| extension)
}

/// File name of an asset, `{sha256}.{ext}`, after checking the digest and content type.
pub fn file_name(sha256: &str, content_type: &str) -> Result<String, AssetError> {
    if !is_sha256_hex(sha256) {
        return Err(AssetError::Digest);
    }
    if !content_type_valid(content_type) {
        return Err(AssetError::ContentType);
    }
    Ok(format!("{sha256}.{}", extension(content_type)))
}

/// A reference to one asset file: its class, digest, content type, and exact size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetRef {
    class: &'static AssetClass,
    sha256: String,
    content_type: String,
    byte_size: u32,
}

impl AssetRef {
    /// Checks the digest, content type, and size against the class.
    pub fn new(
        class: &'static AssetClass,
        sha256: String,
        content_type: String,
        byte_size: u32,
    ) -> Result<Self, AssetError> {
        file_name(&sha256, &content_type)?;
        if byte_size == 0 || byte_size > class.max_bytes {
            return Err(AssetError::Size);
        }
        Ok(Self {
            class,
            sha256,
            content_type,
            byte_size,
        })
    }

    /// The class the file belongs to.
    pub fn class(&self) -> &'static AssetClass {
        self.class
    }

    /// Lowercase hex SHA-256 of the file.
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Media type of the file.
    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    /// Exact size of the file in bytes.
    pub fn byte_size(&self) -> u32 {
        self.byte_size
    }

    /// File name of the asset.
    pub fn file_name(&self) -> String {
        format!("{}.{}", self.sha256, extension(&self.content_type))
    }

    /// Path of the asset in a vault.
    pub fn path(&self, vault_root: &Path) -> PathBuf {
        self.class.directory(vault_root).join(self.file_name())
    }
}

impl Engine {
    /// Every asset that a live row, an open conflict, or a recovery offer references, once per
    /// class and digest, ordered by class name and digest.
    ///
    /// Live rows whose stored values are invalid are skipped, as sealing skips them.
    pub async fn referenced_assets(
        &self,
        conn: &mut SqliteConnection,
    ) -> SyncResult<Vec<AssetRef>> {
        let mut found = BTreeMap::new();
        for domain in self.manifest().domains {
            for table in domain.tables {
                let Some(adapter) = self.adapter(table) else {
                    continue;
                };
                for group_id in adapter.asset_groups(table.id) {
                    let group = table
                        .group(*group_id)
                        .ok_or_else(|| corrupt("an adapter names an unknown asset group"))?;
                    let mut values = live_group_values(self, conn, table, group).await?;
                    values.extend(retained_group_values(self, conn, table.id, group.id).await?);
                    for value in values {
                        for asset in self.asset_refs(domain, table, group.id, &value)? {
                            found.insert((asset.class.name, asset.sha256.clone()), asset);
                        }
                    }
                }
            }
        }
        Ok(found.into_values().collect())
    }

    /// Tables whose rows hold the asset references of the domain that declares `class`, so a
    /// caller can name the tables an arriving file affects. Empty for an unregistered class.
    pub fn asset_tables(&self, class: &AssetClass) -> Vec<&'static str> {
        let mut tables = Vec::new();
        for domain in self.manifest().domains {
            if !domain.assets.contains(class) {
                continue;
            }
            for table in domain.tables {
                let Some(adapter) = self.adapter(table) else {
                    continue;
                };
                for group in adapter
                    .asset_groups(table.id)
                    .iter()
                    .filter_map(|id| table.group(*id))
                {
                    match group.owned_tables() {
                        [] => tables.push(table.name),
                        owned => tables.extend(owned.iter().map(|owned| owned.table)),
                    }
                }
            }
        }
        tables.sort_unstable();
        tables.dedup();
        tables
    }

    /// References of one group value, checked against the domain's classes.
    fn asset_refs(
        &self,
        domain: &DomainManifest,
        table: &TableSpec,
        group: GroupId,
        value: &Value,
    ) -> SyncResult<Vec<AssetRef>> {
        let Some(adapter) = self.adapter(table) else {
            return Ok(Vec::new());
        };
        let assets = adapter
            .asset_refs(table.id, group, value)
            .map_err(|error| corrupt(format!("stored value: {error}")))?;
        if let Some(asset) = assets
            .iter()
            .find(|asset| !domain.assets.iter().any(|class| class == asset.class))
        {
            return Err(corrupt(format!(
                "{} references undeclared asset class {}",
                table.name, asset.class.name
            )));
        }
        Ok(assets)
    }
}

/// Values of one group for every live row that may hold it. A group stored only in owned rows
/// is read for owners that have owned rows, which keeps the scan to rows with assets.
async fn live_group_values(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: &TableSpec,
    group: &GroupSpec,
) -> SyncResult<Vec<Value>> {
    let keys_sql = match group.owned_tables() {
        [owned, ..] if group.columns().is_empty() => format!(
            "SELECT DISTINCT {} FROM {}",
            ident(owned.parent_column),
            ident(owned.table)
        ),
        _ => format!(
            "SELECT {} FROM {}",
            ident(table.key_column),
            ident(table.name)
        ),
    };
    let keys: Vec<String> = sqlx::query_scalar(AssertSqlSafe(keys_sql))
        .fetch_all(&mut *conn)
        .await?;
    let mut values = Vec::new();
    for key in keys {
        if let Some(value) = read_group_value(engine, conn, table, group, &key).await? {
            values.push(value);
        }
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    static CLASS: AssetClass = AssetClass {
        name: "quick-notes/images",
        max_bytes: 100,
    };

    #[test]
    fn assets_are_named_by_digest_and_content_type_extension() {
        assert_eq!(
            file_name(DIGEST, "image/png").unwrap(),
            format!("{DIGEST}.png")
        );
        assert_eq!(
            file_name(DIGEST, "image/jpeg").unwrap(),
            format!("{DIGEST}.jpg")
        );
        assert_eq!(
            file_name(DIGEST, "image/webp").unwrap(),
            format!("{DIGEST}.webp")
        );
        assert_eq!(
            file_name(DIGEST, "image/gif").unwrap(),
            format!("{DIGEST}.gif")
        );
        assert_eq!(
            file_name(DIGEST, "application/pdf").unwrap(),
            format!("{DIGEST}.bin")
        );
    }

    #[test]
    fn asset_names_refuse_paths_and_unprintable_types() {
        assert_eq!(file_name("../escape", "image/png"), Err(AssetError::Digest));
        assert_eq!(
            file_name(&DIGEST.to_uppercase(), "image/png"),
            Err(AssetError::Digest)
        );
        assert_eq!(file_name(DIGEST, ""), Err(AssetError::ContentType));
        assert_eq!(
            file_name(DIGEST, "image/png x"),
            Err(AssetError::ContentType)
        );
        assert_eq!(
            file_name(DIGEST, &"a".repeat(MAX_CONTENT_TYPE_BYTES + 1)),
            Err(AssetError::ContentType)
        );
    }

    #[test]
    fn asset_references_stay_within_their_class_bound() {
        let reference = AssetRef::new(&CLASS, DIGEST.into(), "image/png".into(), 100).unwrap();
        assert_eq!(
            reference.path(Path::new("/vault")),
            Path::new("/vault/assets/quick-notes/images").join(format!("{DIGEST}.png"))
        );
        assert_eq!(
            AssetRef::new(&CLASS, DIGEST.into(), "image/png".into(), 101),
            Err(AssetError::Size)
        );
        assert_eq!(
            AssetRef::new(&CLASS, DIGEST.into(), "image/png".into(), 0),
            Err(AssetError::Size)
        );
    }

    #[test]
    fn asset_classes_are_relative_lowercase_paths_within_the_asset_limit() {
        let class = |name, max_bytes| AssetClass { name, max_bytes };
        assert_eq!(class("profile", 1).problem(), None);
        assert_eq!(class("chat/attachments", MAX_ASSET_BYTES).problem(), None);
        for name in ["", "/abs", "a//b", "a/", "../x", "a/./b", "Upper", "a b"] {
            assert!(class(name, 1).problem().is_some(), "{name}");
        }
        assert!(class("profile", 0).problem().is_some());
        assert!(class("profile", MAX_ASSET_BYTES + 1).problem().is_some());
    }
}
