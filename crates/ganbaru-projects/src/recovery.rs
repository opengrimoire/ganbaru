//! Restoring Projects rows offered for sync recovery.
//!
//! A deleted group, project, section, status, or task comes back as a new row with a key of its
//! own and its retained values, together with copies of its deleted descendants, all inserted as
//! ordinary local writes so the restore seals like any edit. Every restored row gets a new id,
//! since a deleted key cannot be inserted again, and derived keys are computed from the new ids.
//! References to restored rows follow them, references to rows that still exist are kept, and
//! `updated_at` is the restore time. The root goes to the end of its ordered group; descendants
//! keep their retained order unless they land among existing siblings.
//!
//! Where the root goes:
//!
//! - A group is appended to the group list.
//! - A project returns into its group when that group exists, and otherwise into the first group
//!   in list order. It gets a new managed working folder as its primary folder.
//! - A section, status, or task needs its project; when the project is gone the restore is
//!   refused, which leaves the offer to be discarded or restored through the project's own offer.
//! - A task whose parent is gone, or is now itself a subtask, becomes top-level, and a task whose
//!   section or status is gone moves to the first section, or the first status that is not
//!   deleted, of its project.
//!
//! A descendant that can no longer be inserted, such as a dependency whose other task is gone or
//! that would close a cycle with the dependencies that exist now, or a value whose field is
//! gone, is left out. Retained managed working folders are left out
//! because the restored project creates its own; external folders return with new ids and are
//! bound again on each device. A restored task's Calendar event links return for the events that
//! still exist.

use std::collections::HashMap;

use ganbaru_sync::RecoveryDescendant;
use ganbaru_sync::codec;
use ganbaru_sync::manifest::vault::calendar::TASK_EVENT_LINKS;
use ganbaru_sync::manifest::vault::projects::{
    DEPENDENCIES_TABLE, DOMAIN, FOLDERS_TABLE, GROUPS_TABLE, PRIORITIES_TABLE, PROJECTS_TABLE,
    SECTIONS_TABLE, STATUSES_TABLE, TAG_LINKS_TABLE, TASKS_TABLE, VALUES_TABLE,
    VIEW_PREFERENCES_TABLE,
};
use ganbaru_sync::manifest::{OwnedOrder, OwnedTable, TableSpec};
use ganbaru_sync_contracts::{Field, GroupId, TableId, Value};
use ring::rand::{SecureRandom, SystemRandom};
use sqlx::{AssertSqlSafe, Connection, SqliteConnection};

use crate::keys;
use crate::mutations::{
    can_parent_subtasks, dependency_would_cycle, task_section_order, task_status_order,
};
use crate::order::{OrderGroup, OrderPlacement};
use crate::project_commands::insert_managed_working_folder;

/// SQL expression of the current time in the stored timestamp form.
const NOW_SQL: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

/// Column every replicated Projects table stamps with its last write.
const UPDATED_AT: &str = "updated_at";

/// One row to restore: its table, retained key, and retained column values.
struct RestoredRow {
    spec: &'static TableSpec,
    old_key: String,
    columns: Vec<(&'static str, Field)>,
    owned: Vec<(&'static OwnedTable, Vec<u8>)>,
}

impl RestoredRow {
    /// Maps retained group values to columns in manifest order.
    fn new(table: TableId, old_key: &str, values: &[(GroupId, Value)]) -> Result<Self, String> {
        let spec = DOMAIN
            .tables
            .iter()
            .find(|spec| spec.id == table)
            .ok_or_else(|| format!("table {} is not a Projects table", table.0))?;
        Self::of(spec, old_key, values)
    }

    /// Maps retained group values of a row of `spec` to columns in manifest order.
    fn of(
        spec: &'static TableSpec,
        old_key: &str,
        values: &[(GroupId, Value)],
    ) -> Result<Self, String> {
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

    fn required_text(&self, column: &str) -> Result<String, String> {
        self.text(column)
            .map(str::to_string)
            .ok_or_else(|| format!("a recovered {} row has no {column}", self.spec.name))
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
        DOMAIN
            .tables
            .iter()
            .position(|spec| spec.id == self.spec.id)
            .unwrap_or(usize::MAX)
    }
}

/// Whether a descendant is a subtask, which inserts after the top-level tasks it may name.
fn is_subtask(row: &RestoredRow) -> bool {
    row.spec.id == TASKS_TABLE && matches!(row.field("parent_task_id"), Some(Field::Text(_)))
}

/// What became of a reference column while restoring.
enum Resolved {
    /// The referenced row was restored with this id, or still exists under the retained id.
    Row { id: String, restored: bool },
    /// The column was null.
    Null,
    /// The referenced row is gone.
    Missing,
}

/// Ids of the rows restored so far, by table and retained key.
#[derive(Default)]
struct RestoredIds(HashMap<(TableId, String), String>);

impl RestoredIds {
    async fn resolve(
        &self,
        conn: &mut SqliteConnection,
        table: TableId,
        field: Option<&Field>,
    ) -> Result<Resolved, String> {
        let old = match field {
            Some(Field::Text(old)) => old,
            None | Some(Field::Null) => return Ok(Resolved::Null),
            Some(_) => return Err("a recovered reference is not text".to_string()),
        };
        if let Some(id) = self.0.get(&(table, old.clone())) {
            return Ok(Resolved::Row {
                id: id.clone(),
                restored: true,
            });
        }
        let spec = DOMAIN
            .tables
            .iter()
            .find(|spec| spec.id == table)
            .ok_or_else(|| format!("table {} is not a Projects table", table.0))?;
        let exists: Option<i64> = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT 1 FROM {} WHERE {} = ?",
            spec.name, spec.key_column
        )))
        .bind(old)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|error| format!("look up {} row: {error}", spec.name))?;
        Ok(match exists {
            Some(_) => Resolved::Row {
                id: old.clone(),
                restored: false,
            },
            None => Resolved::Missing,
        })
    }
}

/// A random version 4 UUID, the form the frontend gives new Projects rows.
fn new_id() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "secure random generator is unavailable".to_string())?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

/// Restores a deleted Projects row and its deleted descendants on a connection inside the
/// caller's transaction, and returns the id of the new root row.
pub async fn restore_recovered_row(
    conn: &mut SqliteConnection,
    table: TableId,
    row_key: &str,
    values: &[(GroupId, Value)],
    descendants: &[RecoveryDescendant],
) -> Result<String, String> {
    if ![
        GROUPS_TABLE,
        PROJECTS_TABLE,
        SECTIONS_TABLE,
        STATUSES_TABLE,
        TASKS_TABLE,
    ]
    .contains(&table)
    {
        return Err(format!(
            "table {} cannot root a Projects recovery offer",
            table.0
        ));
    }
    let root = RestoredRow::new(table, row_key, values)?;
    let (event_links, descendants): (Vec<_>, Vec<_>) = descendants
        .iter()
        .partition(|descendant| descendant.table == TASK_EVENT_LINKS.id);
    // Descendants of other domains, such as the project's Chat channels, are not restored: the
    // restored project has a new id, and Chat creates its default channel when it lists them.
    let mut rows = descendants
        .iter()
        .filter(|descendant| DOMAIN.tables.iter().any(|spec| spec.id == descendant.table))
        .map(|descendant| {
            RestoredRow::new(descendant.table, &descendant.row_key, &descendant.values)
        })
        .collect::<Result<Vec<_>, _>>()?;
    rows.sort_by_key(|row| (row.table_order(), is_subtask(row)));

    let mut ids = RestoredIds::default();
    let root_id = restore_row(conn, &mut ids, root, true)
        .await?
        .ok_or_else(|| format!("the deleted {} row cannot be restored", table_name(table)))?;
    for row in rows {
        let mut savepoint = conn
            .begin()
            .await
            .map_err(|error| format!("begin restore descendant: {error}"))?;
        match restore_row(&mut savepoint, &mut ids, row, false).await {
            Ok(_) => savepoint
                .commit()
                .await
                .map_err(|error| format!("commit restore descendant: {error}"))?,
            Err(_) => savepoint
                .rollback()
                .await
                .map_err(|error| format!("roll back restore descendant: {error}"))?,
        }
    }
    for link in event_links {
        restore_event_link(conn, &ids, link).await?;
    }
    Ok(root_id)
}

/// Inserts a deleted task's Calendar event link for its restored task when the event still
/// exists. Calendar owns the link table, so its rows are not Projects descendants to restore.
async fn restore_event_link(
    conn: &mut SqliteConnection,
    ids: &RestoredIds,
    link: &RecoveryDescendant,
) -> Result<(), String> {
    let row = RestoredRow::of(&TASK_EVENT_LINKS, &link.row_key, &link.values)?;
    let Some(task_id) = row
        .text("task_id")
        .and_then(|task| ids.0.get(&(TASKS_TABLE, task.to_string())))
    else {
        return Ok(());
    };
    let event_id = row.required_text("event_id")?;
    let event_exists: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM calendar_events WHERE id = ?")
            .bind(&event_id)
            .fetch_optional(&mut *conn)
            .await
            .map_err(|error| format!("look up a restored task's event: {error}"))?;
    if event_exists.is_none() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO project_task_event_links (task_id, event_id, link_kind, created_at, sync_key)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(task_id)
    .bind(&event_id)
    .bind(row.required_text("link_kind")?)
    .bind(row.required_text("created_at")?)
    .bind(keys::task_event_key(task_id, &event_id)?)
    .execute(&mut *conn)
    .await
    .map_err(|error| format!("restore a task's event link: {error}"))?;
    Ok(())
}

fn table_name(table: TableId) -> &'static str {
    DOMAIN
        .tables
        .iter()
        .find(|spec| spec.id == table)
        .map_or("Projects", |spec| spec.name)
}

/// Inserts one restored row and records its new id. Returns `None` for a row that is left out on
/// purpose, and an error for a row that cannot be inserted.
async fn restore_row(
    conn: &mut SqliteConnection,
    ids: &mut RestoredIds,
    mut row: RestoredRow,
    is_root: bool,
) -> Result<Option<String>, String> {
    let table = row.spec.id;
    if table == FOLDERS_TABLE && row.text("kind") == Some("managed") {
        return Ok(None);
    }
    let tree_parent = row
        .spec
        .tree
        .as_ref()
        .and_then(|tree| row.spec.group(tree.parent_group))
        .and_then(|group| group.columns().first())
        .map(|column| column.name);
    let mut restored_scope = HashMap::new();
    for (_, _, reference) in row.spec.references() {
        let resolved = ids
            .resolve(conn, reference.table, row.field(reference.column))
            .await?;
        match resolved {
            Resolved::Row { id, restored } => {
                restored_scope.insert(reference.column, restored);
                row.set_text(reference.column, &id);
            }
            Resolved::Null => {}
            Resolved::Missing if Some(reference.column) == tree_parent => {
                row.set(reference.column, Field::Null);
            }
            Resolved::Missing => {
                let fallback = fallback_reference(conn, &row, reference.column).await?;
                let Some(id) = fallback else {
                    return Err(format!(
                        "the {} of a restored {} row is gone",
                        reference.column, row.spec.name
                    ));
                };
                restored_scope.insert(reference.column, false);
                row.set_text(reference.column, &id);
            }
        }
    }
    if table == TASKS_TABLE
        && let Some(parent) = row.text("parent_task_id").map(str::to_string)
        && !can_parent_subtasks(conn, &row.required_text("project_id")?, &parent).await?
    {
        row.set("parent_task_id", Field::Null);
        restored_scope.remove("parent_task_id");
    }
    if table == DEPENDENCIES_TABLE
        && dependency_would_cycle(
            conn,
            &row.required_text("blocking_task_id")?,
            &row.required_text("blocked_task_id")?,
        )
        .await?
    {
        return Ok(None);
    }

    let key = new_key(&mut row)?;
    place(conn, &mut row, &key, is_root, &restored_scope).await?;
    insert(conn, &row).await?;
    for (owned, bytes) in &row.owned {
        insert_owned(conn, ids, owned, &key, bytes).await?;
    }
    if table == PROJECTS_TABLE {
        let name = row.required_text("name")?;
        insert_managed_working_folder(conn, &key, name.trim()).await?;
    }
    ids.0.insert((table, row.old_key.clone()), key.clone());
    Ok(Some(key))
}

/// The row a missing required reference moves to: a project's first group, or a task's first
/// section or first status that is not deleted. Other references have no fallback.
async fn fallback_reference(
    conn: &mut SqliteConnection,
    row: &RestoredRow,
    column: &str,
) -> Result<Option<String>, String> {
    let query = match (row.spec.id, column) {
        (PROJECTS_TABLE, "group_id") => {
            return sqlx::query_scalar(
                "SELECT id FROM project_groups ORDER BY order_key, id LIMIT 1",
            )
            .fetch_optional(&mut *conn)
            .await
            .map_err(|error| format!("find a group for a restored project: {error}"));
        }
        (TASKS_TABLE, "section_id") => {
            "SELECT id FROM project_sections WHERE project_id = ?
             ORDER BY archived_at IS NOT NULL, order_key, id LIMIT 1"
        }
        (TASKS_TABLE, "status_id") => {
            "SELECT id FROM project_statuses WHERE project_id = ? AND deleted_at IS NULL
             ORDER BY order_key, id LIMIT 1"
        }
        _ => return Ok(None),
    };
    let Some(project_id) = row.text("project_id") else {
        return Ok(None);
    };
    sqlx::query_scalar(query)
        .bind(project_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|error| format!("find a {column} for a restored task: {error}"))
}

/// Sets the key of a restored row: its derived key, or a new id.
fn new_key(row: &mut RestoredRow) -> Result<String, String> {
    let derived = match row.spec.id {
        PRIORITIES_TABLE => Some(keys::priority_key(
            &row.required_text("project_id")?,
            &row.required_text("id")?,
        )?),
        DEPENDENCIES_TABLE => Some(keys::dependency_key(
            &row.required_text("blocking_task_id")?,
            &row.required_text("blocked_task_id")?,
        )?),
        TAG_LINKS_TABLE => Some(keys::task_tag_key(
            &row.required_text("task_id")?,
            &row.required_text("tag_id")?,
        )?),
        VALUES_TABLE => Some(keys::custom_field_value_key(
            &row.required_text("task_id")?,
            &row.required_text("field_id")?,
        )?),
        VIEW_PREFERENCES_TABLE => Some(keys::view_preference_key(
            &row.required_text("view_id")?,
            &row.required_text("preference_key")?,
        )?),
        _ if row.spec.derived_key.is_some() => {
            return Err(format!("restoring {} rows is not supported", row.spec.name));
        }
        _ => None,
    };
    let key = match derived {
        Some(key) => key,
        None => new_id()?,
    };
    row.set_text(row.spec.key_column, &key);
    for column in row.spec.key_derived_columns {
        row.set_text(column.name, &format!("{}{key}", column.prefix));
    }
    Ok(key)
}

/// Gives the root the last place in its ordered group, and a task the last place in each scope
/// it joins among existing siblings.
async fn place(
    conn: &mut SqliteConnection,
    row: &mut RestoredRow,
    key: &str,
    is_root: bool,
    restored_scope: &HashMap<&'static str, bool>,
) -> Result<(), String> {
    let restored = |column: &str| restored_scope.get(column).copied().unwrap_or(false);
    let end = OrderPlacement::End;
    match row.spec.id {
        TASKS_TABLE => {
            let project_id = row.required_text("project_id")?;
            let section_id = row.required_text("section_id")?;
            let status_id = row.required_text("status_id")?;
            let parent = row.text("parent_task_id").map(str::to_string);
            let section_fresh = match parent {
                Some(_) => restored("parent_task_id"),
                None => restored("section_id"),
            };
            let status_fresh = restored("status_id") || restored("parent_task_id");
            if is_root || !section_fresh {
                let order_key = task_section_order(&project_id, &section_id, parent.as_deref())
                    .place(conn, key, &end)
                    .await?;
                row.set_text("section_order_key", order_key.as_str());
            }
            if is_root || !status_fresh {
                let order_key = task_status_order(&project_id, &status_id, parent.as_deref())
                    .place(conn, key, &end)
                    .await?;
                row.set_text("status_order_key", order_key.as_str());
            }
        }
        _ if is_root => {
            let scope = match row.spec.id {
                GROUPS_TABLE => None,
                PROJECTS_TABLE => Some(("group_id", row.required_text("group_id")?)),
                _ => Some(("project_id", row.required_text("project_id")?)),
            };
            let mut group = OrderGroup::new(row.spec.name, "order_key");
            if let Some((column, value)) = &scope {
                group = group.filter(column, Some(value.as_str()));
            }
            let order_key = group.place(conn, key, &end).await?;
            row.set_text("order_key", order_key.as_str());
        }
        _ => {}
    }
    Ok(())
}

/// Inserts the row's columns, its local columns at their default, and `updated_at` as now.
async fn insert(conn: &mut SqliteConnection, row: &RestoredRow) -> Result<(), String> {
    let mut names = Vec::new();
    let mut placeholders = Vec::new();
    let mut binds = Vec::new();
    for (name, field) in &row.columns {
        if *name == UPDATED_AT {
            continue;
        }
        names.push(*name);
        placeholders.push("?");
        binds.push(field);
    }
    for local in row.spec.local_columns {
        names.push(local.name);
        placeholders.push(local.materialize);
    }
    if row.spec.groups.iter().any(|group| {
        group
            .columns()
            .iter()
            .any(|column| column.name == UPDATED_AT)
    }) {
        names.push(UPDATED_AT);
        placeholders.push(NOW_SQL);
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
            Field::Integer(value) => query.bind(*value),
            Field::Text(text) => query.bind(text.clone()),
            Field::Blob(bytes) => query.bind(bytes.clone()),
        };
    }
    query
        .execute(&mut *conn)
        .await
        .map_err(|error| format!("restore {} row: {error}", row.spec.name))?;
    Ok(())
}

/// Inserts the owned rows of a restored row, leaving out rows whose referenced row is gone.
async fn insert_owned(
    conn: &mut SqliteConnection,
    ids: &RestoredIds,
    owned: &'static OwnedTable,
    parent_key: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let OwnedOrder::Ordinal(ordinal_column) = owned.order else {
        return Err(format!("restoring {} rows is not supported", owned.table));
    };
    let rows = codec::decode(owned, bytes)
        .map_err(|reason| format!("decode recovered {}: {reason}", owned.table))?;
    let mut ordinal = 0_i64;
    'rows: for decoded in rows {
        let mut fields = decoded.fields;
        for reference in owned.references {
            let Some(index) = owned
                .columns
                .iter()
                .position(|column| column.name == reference.column)
            else {
                continue;
            };
            match ids
                .resolve(conn, reference.table, fields.get(index))
                .await?
            {
                Resolved::Row { id, .. } => fields[index] = Field::Text(id),
                Resolved::Null => {}
                Resolved::Missing => continue 'rows,
            }
        }
        let names: Vec<&str> = owned.columns.iter().map(|column| column.name).collect();
        let sql = format!(
            "INSERT INTO {} ({}, {}, {}) VALUES (?, ?{})",
            owned.table,
            owned.parent_column,
            ordinal_column,
            names.join(", "),
            ", ?".repeat(names.len())
        );
        let mut query = sqlx::query(AssertSqlSafe(sql))
            .bind(parent_key)
            .bind(ordinal);
        for field in fields {
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
        ordinal += 1;
    }
    Ok(())
}
