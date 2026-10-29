//! Restoring Notes rows offered for sync recovery.
//!
//! A deleted page, block, data source, template, comment thread, or comment comes back as a new
//! row with a key of its own and its retained values, together with copies of its deleted Notes
//! descendants, all inserted as ordinary local writes so the restore seals like any edit. A
//! deleted key cannot be inserted again, so every restored row gets a new id. A nested page and
//! its `child_page` block, and a database and its `child_database` block, share one new id as
//! they shared the retained one. Derived keys are computed from the new ids, references to
//! restored rows follow them, and references to rows that still exist are kept.
//!
//! Where the root goes:
//!
//! - A page returns under its parent when its `child_page` block returns with it, into its data
//!   source when that still exists or returns with it, and otherwise to the workspace. A row page
//!   that returns to the workspace leaves its property values and relations out.
//! - A block needs its parent page or block, a data source its database, a data source template
//!   its data source, a comment thread the page or block it is on, and a comment its thread.
//!   When that row is gone the restore is refused, which leaves the offer to be discarded or the
//!   row to be restored through the offer of what held it.
//! - A database is refused: it shares its id with the block that holds it, so it comes back with
//!   that block.
//! - A page template has no parent and always returns.
//!
//! The root leaves the trash, together with the rows that the same trash operation moved there,
//! and a restored comment is no longer marked deleted. A descendant that can no longer be
//! inserted, such as one whose referenced row is gone or an alias whose name is now taken, is
//! left out. Mentions inside rich text keep the ids they were written with.
//!
//! Notes rows are written only by the device that writes Notes, so on a replica the restore is
//! refused and the offer can only be discarded there.

use std::collections::{HashMap, HashSet};

use ganbaru_sync::RecoveryDescendant;
use ganbaru_sync::codec;
use ganbaru_sync::guards::is_guard_abort;
use ganbaru_sync::manifest::vault::notes::{
    ASSETS_TABLE, BLOCKS_TABLE, COMMENTS_TABLE, DATA_SOURCE_TEMPLATES_TABLE, DATA_SOURCES_TABLE,
    DATABASES_TABLE, DOMAIN, FOLDERS_TABLE, LOCAL_USERS_TABLE, OPTIONS_TABLE, PAGE_TEMPLATES_TABLE,
    PAGES_TABLE, RELATION_EDGES_TABLE, THREADS_TABLE, VALUES_TABLE,
};
use ganbaru_sync::manifest::{OnDelete, OwnedOrder, OwnedTable, TableSpec};
use ganbaru_sync_contracts::{Field, GroupId, TableId, Value};
use sqlx::{AssertSqlSafe, Connection, SqliteConnection};

use crate::sync_apply::refresh_after_apply;
use crate::sync_keys::DERIVED_KEY_TABLES;

/// SQL expression of the current time in the stored timestamp form.
const NOW_SQL: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

/// SQL expression of a new version 4 UUID, the form of every Notes id.
const NEW_ID_SQL: &str = "SELECT lower(hex(randomblob(4))) || '-' ||
    lower(hex(randomblob(2))) || '-' ||
    '4' || substr(lower(hex(randomblob(2))), 2) || '-' ||
    substr('89ab', abs(random() % 4) + 1, 1) || substr(lower(hex(randomblob(2))), 2) || '-' ||
    lower(hex(randomblob(6)))";

/// Column every replicated Notes table stamps with its last edit.
const LAST_EDITED: &str = "last_edited_time";

/// Tables whose deleted rows can root a Notes recovery offer.
pub const ROOT_TABLES: &[TableId] = &[
    PAGES_TABLE,
    BLOCKS_TABLE,
    DATABASES_TABLE,
    DATA_SOURCES_TABLE,
    DATA_SOURCE_TEMPLATES_TABLE,
    PAGE_TEMPLATES_TABLE,
    THREADS_TABLE,
    COMMENTS_TABLE,
];

/// Refusal of a restore on a device that does not write Notes.
pub const REPLICA_REFUSAL: &str =
    "deleted Notes rows can be restored only on the device that writes Notes";

/// Refusal of a database root, which shares its id with the block that holds it.
const DATABASE_REFUSAL: &str =
    "a deleted Notes database comes back by restoring the block that holds it";

/// One row to restore: its table, retained key, and retained column values.
#[derive(Clone)]
struct RestoredRow {
    spec: &'static TableSpec,
    old_key: String,
    columns: Vec<(&'static str, Field)>,
    owned: Vec<(&'static OwnedTable, Vec<u8>)>,
}

impl RestoredRow {
    /// Maps retained group values of a Notes row to columns in manifest order.
    fn new(table: TableId, old_key: &str, values: &[(GroupId, Value)]) -> Result<Self, String> {
        let spec = spec_of(table)?;
        let mut columns = Vec::new();
        let mut owned = Vec::new();
        for (group_id, value) in values {
            // The existence group of a derived-key table holds a presence flag, not a column.
            if spec.is_exists_group(*group_id) {
                continue;
            }
            let group = spec
                .group(*group_id)
                .ok_or_else(|| format!("{} has no field group {}", spec.name, group_id.get()))?;
            let fields = value.fields();
            if fields.len() != group.field_count() {
                return Err(format!(
                    "a recovered {} {} value has the wrong shape",
                    spec.name, group.name
                ));
            }
            let (column_fields, owned_fields) = fields.split_at(group.columns().len());
            for (column, field) in group.columns().iter().zip(column_fields) {
                columns.push((column.name, field.clone()));
            }
            for (table, field) in group.owned_tables().iter().zip(owned_fields) {
                let Field::Blob(bytes) = field else {
                    return Err(format!(
                        "a recovered {} owned value is not encoded",
                        spec.name
                    ));
                };
                owned.push((table, bytes.clone()));
            }
        }
        Ok(Self {
            spec,
            old_key: old_key.to_string(),
            columns,
            owned,
        })
    }

    fn id(&self) -> (TableId, String) {
        (self.spec.id, self.old_key.clone())
    }

    fn field(&self, column: &str) -> Option<&Field> {
        self.columns
            .iter()
            .find(|(name, _)| *name == column)
            .map(|(_, field)| field)
    }

    fn text(&self, column: &str) -> Option<&str> {
        match self.field(column) {
            Some(Field::Text(text)) => Some(text),
            _ => None,
        }
    }

    fn set(&mut self, column: &'static str, field: Field) {
        match self.columns.iter_mut().find(|(name, _)| *name == column) {
            Some((_, existing)) => *existing = field,
            None => self.columns.push((column, field)),
        }
    }

    fn set_text(&mut self, column: &'static str, text: &str) {
        self.set(column, Field::Text(text.to_string()));
    }

    /// Position of the table in the manifest, which lists referenced tables first.
    fn table_order(&self) -> usize {
        table_order(self.spec.id)
    }
}

/// Whether a retained derived-key row records the removal of its identity rather than a row.
fn is_removal(spec: &TableSpec, values: &[(GroupId, Value)]) -> bool {
    values
        .iter()
        .any(|(group, value)| spec.is_exists_group(*group) && value.fields() == [Field::Integer(0)])
}

fn spec_of(table: TableId) -> Result<&'static TableSpec, String> {
    DOMAIN
        .tables
        .iter()
        .find(|spec| spec.id == table)
        .ok_or_else(|| format!("table {} is not a Notes table", table.0))
}

fn table_order(table: TableId) -> usize {
    DOMAIN
        .tables
        .iter()
        .position(|spec| spec.id == table)
        .unwrap_or(usize::MAX)
}

/// Name of a Notes row kind in refusal messages.
fn kind(table: TableId) -> &'static str {
    match table {
        PAGES_TABLE => "page",
        BLOCKS_TABLE => "block",
        DATABASES_TABLE => "database",
        DATA_SOURCES_TABLE => "data source",
        DATA_SOURCE_TEMPLATES_TABLE => "data source template",
        PAGE_TEMPLATES_TABLE => "page template",
        THREADS_TABLE => "comment thread",
        COMMENTS_TABLE => "comment",
        LOCAL_USERS_TABLE => "author",
        FOLDERS_TABLE => "folder",
        _ => "row",
    }
}

/// What a reference column resolves to while restoring.
enum Reference {
    /// The referenced row was restored with this id, or still exists under the retained id.
    Row(String),
    /// The referenced row is part of the restore and not inserted yet.
    Wait,
    /// The referenced row is gone.
    Missing,
}

/// The outcome of preparing one row for insertion.
enum Step {
    /// The row is ready with its new key and the set-null references to fill in afterwards.
    Ready {
        row: RestoredRow,
        key: String,
        patches: Vec<Patch>,
    },
    /// The row waits for a row of the restore it references.
    Wait,
    /// The row cannot be restored, for the given reason.
    Skip(String),
}

/// A set-null reference to a row of the restore that was inserted after the referring row.
struct Patch {
    column: &'static str,
    table: TableId,
    old: String,
}

/// State of one restore.
#[derive(Default)]
struct Restore {
    /// New ids by retained id. Pages and blocks, and databases and blocks, share ids, so one map
    /// serves every table keyed by a plain id.
    fresh: HashMap<String, String>,
    /// New keys of the inserted rows by table and retained key.
    inserted: HashMap<(TableId, String), String>,
    /// Rows of the restore not inserted or left out yet.
    pending: HashSet<(TableId, String)>,
    /// Retained ids of pages, blocks, and databases that leave the trash.
    untrashed: HashSet<String>,
    /// Retained ids of row pages restored outside their data source.
    detached: HashSet<String>,
    /// Trash operation of the root, whose other rows leave the trash with it.
    trash_owner: Option<String>,
    /// Set-null references to fill in once their rows are inserted, by table and new key.
    patches: Vec<(&'static TableSpec, String, Patch)>,
}

/// Restores a deleted Notes row and its deleted Notes descendants on a connection inside the
/// caller's transaction, and returns the id of the new root row.
pub async fn restore_recovered_row(
    conn: &mut SqliteConnection,
    table: TableId,
    row_key: &str,
    values: &[(GroupId, Value)],
    descendants: &[RecoveryDescendant],
) -> Result<String, String> {
    restore(conn, table, row_key, values, descendants)
        .await
        .map_err(|error| {
            if is_guard_abort(&error) {
                REPLICA_REFUSAL.to_string()
            } else {
                error
            }
        })
}

async fn restore(
    conn: &mut SqliteConnection,
    table: TableId,
    row_key: &str,
    values: &[(GroupId, Value)],
    descendants: &[RecoveryDescendant],
) -> Result<String, String> {
    if !ROOT_TABLES.contains(&table) {
        return Err(format!(
            "table {} cannot root a Notes recovery offer",
            table.0
        ));
    }
    if table == DATABASES_TABLE {
        return Err(DATABASE_REFUSAL.to_string());
    }
    let root = RestoredRow::new(table, row_key, values)?;
    // Descendants of other domains are restored by their own domain or not at all, and managed
    // assets are never deleted.
    let mut rows = Vec::new();
    for descendant in descendants {
        if descendant.table == ASSETS_TABLE {
            continue;
        }
        let Ok(spec) = spec_of(descendant.table) else {
            continue;
        };
        if is_removal(spec, &descendant.values) {
            continue;
        }
        rows.push(RestoredRow::new(
            descendant.table,
            &descendant.row_key,
            &descendant.values,
        )?);
    }
    rows.sort_by_key(RestoredRow::table_order);

    let mut state = Restore::default();
    state.pending.insert(root.id());
    state.pending.extend(rows.iter().map(RestoredRow::id));
    if matches!(root.field("in_trash"), Some(Field::Integer(1))) {
        state.trash_owner = root.text("trash_owner").map(str::to_string);
    }

    let root_id = match state.prepare(conn, &root, true).await? {
        Step::Ready { row, key, patches } => {
            state.insert(conn, row, &key, patches, true).await?;
            key
        }
        Step::Wait => {
            return Err(format!(
                "the deleted Notes {} references itself",
                kind(table)
            ));
        }
        Step::Skip(reason) => return Err(reason),
    };

    let mut queue = rows;
    while !queue.is_empty() {
        let mut progressed = false;
        let mut waiting = Vec::new();
        for row in queue {
            match state.prepare(conn, &row, false).await {
                Ok(Step::Wait) => waiting.push(row),
                Ok(Step::Ready {
                    row: ready,
                    key,
                    patches,
                }) => {
                    progressed = true;
                    let mut savepoint = conn
                        .begin()
                        .await
                        .map_err(|error| format!("begin restore descendant: {error}"))?;
                    match state
                        .insert(&mut savepoint, ready, &key, patches, false)
                        .await
                    {
                        Ok(()) => savepoint
                            .commit()
                            .await
                            .map_err(|error| format!("commit restore descendant: {error}"))?,
                        Err(_) => {
                            savepoint.rollback().await.map_err(|error| {
                                format!("roll back restore descendant: {error}")
                            })?;
                            state.leave_out(&row);
                        }
                    }
                }
                Ok(Step::Skip(_)) | Err(_) => {
                    progressed = true;
                    state.leave_out(&row);
                }
            }
        }
        queue = waiting;
        if !progressed {
            break;
        }
    }

    state.apply_patches(conn).await?;
    for spec in DOMAIN.tables {
        let keys: Vec<String> = state
            .inserted
            .iter()
            .filter(|((table, _), _)| *table == spec.id)
            .map(|(_, key)| key.clone())
            .collect();
        refresh_after_apply(conn, spec.id, &keys).await?;
    }
    Ok(root_id)
}

impl Restore {
    /// Forgets a row that cannot be restored, so rows that need it are left out too.
    fn leave_out(&mut self, row: &RestoredRow) {
        self.pending.remove(&row.id());
        self.untrashed.remove(&row.old_key);
    }

    /// New id for a retained id, the same for every table that shares it.
    async fn fresh_id(&mut self, conn: &mut SqliteConnection, old: &str) -> Result<String, String> {
        if let Some(id) = self.fresh.get(old) {
            return Ok(id.clone());
        }
        let id: String = sqlx::query_scalar(NEW_ID_SQL)
            .fetch_one(&mut *conn)
            .await
            .map_err(|error| format!("generate Notes id: {error}"))?;
        if self.fresh.values().any(|taken| *taken == id) {
            return Err("generated a Notes id twice".to_string());
        }
        self.fresh.insert(old.to_string(), id.clone());
        Ok(id)
    }

    async fn resolve(
        &self,
        conn: &mut SqliteConnection,
        table: TableId,
        old: &str,
    ) -> Result<Reference, String> {
        if let Some(id) = self.inserted.get(&(table, old.to_string())) {
            return Ok(Reference::Row(id.clone()));
        }
        if self.pending.contains(&(table, old.to_string())) {
            return Ok(Reference::Wait);
        }
        let spec = spec_of(table)?;
        let exists: Option<i64> = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT 1 FROM {} WHERE {} = ?",
            spec.name, spec.key_column
        )))
        .bind(old)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|error| format!("check {} row: {error}", spec.name))?;
        Ok(if exists.is_some() {
            Reference::Row(old.to_string())
        } else {
            Reference::Missing
        })
    }

    /// Resolves the row's key and references and applies the placement and trash rules.
    async fn prepare(
        &mut self,
        conn: &mut SqliteConnection,
        original: &RestoredRow,
        root: bool,
    ) -> Result<Step, String> {
        let spec = original.spec;
        let table = spec.id;
        let mut row = original.clone();
        let gone = |parent: TableId| {
            Step::Skip(format!(
                "the deleted Notes {} cannot be restored because the {} that held it is gone",
                kind(table),
                kind(parent)
            ))
        };

        let mut key = match spec.key_parent {
            Some(parent) => match self.resolve(conn, parent, &row.old_key).await? {
                Reference::Row(id) => Some(id),
                Reference::Wait => return Ok(Step::Wait),
                Reference::Missing => return Ok(gone(parent)),
            },
            None if spec.derived_key.is_none() => Some(self.fresh_id(conn, &row.old_key).await?),
            None => None,
        };

        let mut patches = Vec::new();
        for (_, _, reference) in spec.references() {
            let Some(Field::Text(old)) = row.field(reference.column).cloned() else {
                continue;
            };
            match self.resolve(conn, reference.table, &old).await? {
                Reference::Row(id) => row.set_text(reference.column, &id),
                Reference::Wait if reference.on_delete == OnDelete::Cascade => {
                    return Ok(Step::Wait);
                }
                Reference::Wait => {
                    row.set(reference.column, Field::Null);
                    patches.push(Patch {
                        column: reference.column,
                        table: reference.table,
                        old,
                    });
                }
                // A page whose parent block or data source is gone returns to the workspace.
                Reference::Missing
                    if reference.on_delete == OnDelete::SetNull || table == PAGES_TABLE =>
                {
                    row.set(reference.column, Field::Null);
                }
                Reference::Missing => return Ok(gone(reference.table)),
            }
        }

        match table {
            PAGES_TABLE => {
                self.place_page(conn, original, &mut row, key.as_deref())
                    .await?
            }
            BLOCKS_TABLE => {
                let parent_type = if row.text("parent_page_id").is_some() {
                    "page_id"
                } else {
                    "block_id"
                };
                row.set_text("parent_type", parent_type);
            }
            OPTIONS_TABLE => {
                // The option names its data source as plain text beside its property reference.
                if let Some(old) = row.text("data_source_id").map(str::to_string) {
                    match self.resolve(conn, DATA_SOURCES_TABLE, &old).await? {
                        Reference::Row(id) => row.set_text("data_source_id", &id),
                        Reference::Wait => return Ok(Step::Wait),
                        Reference::Missing => return Ok(gone(DATA_SOURCES_TABLE)),
                    }
                }
            }
            VALUES_TABLE => {
                let page = original.text("page_id").unwrap_or_default();
                if self.detached.contains(page) || !value_property_exists(conn, &row).await? {
                    return Ok(Step::Skip("the value's property is gone".to_string()));
                }
            }
            RELATION_EDGES_TABLE => {
                let detached = ["source_page_id", "target_page_id"]
                    .iter()
                    .any(|column| self.detached.contains(original.text(column).unwrap_or("")));
                if detached {
                    return Ok(Step::Skip(
                        "the relation's page left its data source".to_string(),
                    ));
                }
            }
            COMMENTS_TABLE if root => row.set("deleted_at", Field::Null),
            _ => {}
        }

        self.leave_trash(original, &mut row, root);

        if key.is_none() {
            key = Some(derived_key(&row)?);
        }
        let key = key.ok_or_else(|| format!("a recovered {} row has no key", spec.name))?;
        Ok(Step::Ready { row, key, patches })
    }

    /// Places a page under its restored `child_page` block, into its data source, or at the
    /// workspace, and sets the parent type the local nesting check reads.
    async fn place_page(
        &mut self,
        conn: &mut SqliteConnection,
        original: &RestoredRow,
        row: &mut RestoredRow,
        key: Option<&str>,
    ) -> Result<(), String> {
        let nested = row.text("child_block_id").map(str::to_string);
        // Only the block that shares the page's id can hold it; a surviving block cannot.
        let nested = nested.filter(|block| Some(block.as_str()) == key);
        let parent_type = match &nested {
            Some(block) => {
                let under_page: Option<bool> = sqlx::query_scalar(
                    "SELECT parent_page_id IS NOT NULL FROM notes_blocks WHERE id = ?",
                )
                .bind(block)
                .fetch_optional(&mut *conn)
                .await
                .map_err(|error| format!("read restored child page block: {error}"))?;
                match under_page {
                    Some(true) => "page_id",
                    Some(false) => "block_id",
                    None => "workspace",
                }
            }
            None => {
                row.set("child_block_id", Field::Null);
                if row.text("parent_data_source_id").is_some() {
                    "data_source_id"
                } else {
                    "workspace"
                }
            }
        };
        if parent_type == "workspace" {
            row.set("child_block_id", Field::Null);
        }
        if original.text("parent_data_source_id").is_some()
            && row.text("parent_data_source_id").is_none()
        {
            self.detached.insert(original.old_key.clone());
        }
        if parent_type != "workspace" {
            row.set("folder_id", Field::Null);
        }
        row.set_text("parent_type", parent_type);
        Ok(())
    }

    /// Takes the root, and the rows its trash operation moved with it, out of the trash.
    fn leave_trash(&mut self, original: &RestoredRow, row: &mut RestoredRow, root: bool) {
        if !matches!(row.field("in_trash"), Some(Field::Integer(1))) {
            return;
        }
        let table = row.spec.id;
        let leaves = match table {
            PAGES_TABLE | BLOCKS_TABLE => {
                root || (self.trash_owner.is_some()
                    && original.text("trash_owner") == self.trash_owner.as_deref())
            }
            // A database shares its id with its block, and a data source follows its database.
            DATABASES_TABLE => self.untrashed.contains(&original.old_key),
            DATA_SOURCES_TABLE => {
                root || original
                    .text("database_id")
                    .is_some_and(|database| self.untrashed.contains(database))
            }
            _ => root,
        };
        if !leaves {
            return;
        }
        row.set("in_trash", Field::Integer(0));
        if row.field("trash_owner").is_some() {
            row.set("trash_owner", Field::Null);
        }
        if row.field("trashed_time").is_some() {
            row.set("trashed_time", Field::Null);
        }
        if matches!(table, PAGES_TABLE | BLOCKS_TABLE | DATABASES_TABLE) {
            self.untrashed.insert(original.old_key.clone());
        }
    }

    /// Inserts a prepared row and its owned rows, and records its new key.
    async fn insert(
        &mut self,
        conn: &mut SqliteConnection,
        row: RestoredRow,
        key: &str,
        patches: Vec<Patch>,
        root: bool,
    ) -> Result<(), String> {
        insert_row(conn, &row, key, root).await?;
        for (owned, bytes) in &row.owned {
            insert_owned(conn, owned, key, bytes).await?;
        }
        let id = row.id();
        self.pending.remove(&id);
        self.inserted.insert(id, key.to_string());
        self.patches.extend(
            patches
                .into_iter()
                .map(|patch| (row.spec, key.to_string(), patch)),
        );
        Ok(())
    }

    /// Fills the set-null references to rows inserted after the rows that name them. A
    /// reference that no longer fits, such as one that would close a row-page cycle, stays null.
    async fn apply_patches(&self, conn: &mut SqliteConnection) -> Result<(), String> {
        for (spec, key, patch) in &self.patches {
            let Some(target) = self.inserted.get(&(patch.table, patch.old.clone())) else {
                continue;
            };
            let mut savepoint = conn
                .begin()
                .await
                .map_err(|error| format!("begin restore reference: {error}"))?;
            let updated = sqlx::query(AssertSqlSafe(format!(
                "UPDATE {} SET {} = ? WHERE {} = ?",
                spec.name, patch.column, spec.key_column
            )))
            .bind(target)
            .bind(key)
            .execute(&mut *savepoint)
            .await;
            match updated {
                Ok(_) => savepoint
                    .commit()
                    .await
                    .map_err(|error| format!("commit restore reference: {error}"))?,
                Err(_) => savepoint
                    .rollback()
                    .await
                    .map_err(|error| format!("roll back restore reference: {error}"))?,
            }
        }
        Ok(())
    }
}

/// Whether the property a restored row page value names exists in the page's data source.
async fn value_property_exists(
    conn: &mut SqliteConnection,
    row: &RestoredRow,
) -> Result<bool, String> {
    let (Some(page), Some(property)) = (row.text("page_id"), row.text("property_id")) else {
        return Ok(false);
    };
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM notes_pages AS page
         JOIN notes_data_source_properties AS property
           ON property.data_source_id = page.parent_data_source_id
         WHERE page.id = ? AND property.property_id = ?",
    )
    .bind(page)
    .bind(property)
    .fetch_optional(&mut *conn)
    .await
    .map_err(|error| format!("check restored value property: {error}"))?;
    Ok(exists.is_some())
}

/// The derived key of a row whose key is computed from its identity columns.
fn derived_key(row: &RestoredRow) -> Result<String, String> {
    let table = DERIVED_KEY_TABLES
        .iter()
        .find(|table| table.table == row.spec.name)
        .ok_or_else(|| format!("{} has no derived key", row.spec.name))?;
    let parts = table
        .columns
        .iter()
        .map(|column| {
            row.text(column)
                .ok_or_else(|| format!("a recovered {} row has no {column}", row.spec.name))
        })
        .collect::<Result<Vec<_>, _>>()?;
    table.key(&parts)
}

/// Inserts the row's columns under its new key. The root's last edit is the restore time.
async fn insert_row(
    conn: &mut SqliteConnection,
    row: &RestoredRow,
    key: &str,
    root: bool,
) -> Result<(), String> {
    let mut names = vec![row.spec.key_column];
    let mut placeholders = vec!["?"];
    let mut binds = vec![Field::Text(key.to_string())];
    for (name, field) in &row.columns {
        if *name == row.spec.key_column {
            continue;
        }
        names.push(name);
        if root && *name == LAST_EDITED {
            placeholders.push(NOW_SQL);
            continue;
        }
        placeholders.push("?");
        binds.push(field.clone());
    }
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        row.spec.name,
        names.join(", "),
        placeholders.join(", ")
    );
    let mut query = sqlx::query(AssertSqlSafe(sql));
    for field in binds {
        query = match field {
            Field::Null => query.bind(None::<String>),
            Field::Integer(value) => query.bind(value),
            Field::Text(text) => query.bind(text),
            Field::Blob(bytes) => query.bind(bytes),
        };
    }
    query
        .execute(&mut *conn)
        .await
        .map_err(|error| format!("restore {} row: {error}", row.spec.name))?;
    Ok(())
}

/// Inserts the owned rows of a restored row, such as a template's blocks, in retained order.
async fn insert_owned(
    conn: &mut SqliteConnection,
    owned: &'static OwnedTable,
    parent_key: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let OwnedOrder::Ordinal(ordinal_column) = owned.order else {
        return Err(format!("restoring {} rows is not supported", owned.table));
    };
    if !owned.references.is_empty() {
        return Err(format!("restoring {} rows is not supported", owned.table));
    }
    let rows = codec::decode(owned, bytes)
        .map_err(|reason| format!("decode recovered {}: {reason}", owned.table))?;
    let names: Vec<&str> = owned.columns.iter().map(|column| column.name).collect();
    let sql = format!(
        "INSERT INTO {} ({}, {}, {}) VALUES (?, ?{})",
        owned.table,
        owned.parent_column,
        ordinal_column,
        names.join(", "),
        ", ?".repeat(names.len())
    );
    for (ordinal, decoded) in (0_i64..).zip(rows) {
        let mut query = sqlx::query(AssertSqlSafe(sql.as_str()))
            .bind(parent_key)
            .bind(ordinal);
        for field in decoded.fields {
            query = match field {
                Field::Null => query.bind(None::<String>),
                Field::Integer(value) => query.bind(value),
                Field::Text(text) => query.bind(text),
                Field::Blob(bytes) => query.bind(bytes),
            };
        }
        query
            .execute(&mut *conn)
            .await
            .map_err(|error| format!("restore {} row: {error}", owned.table))?;
    }
    Ok(())
}
