//! Conflicts on Projects names, task titles and descriptions, checklist item titles, and
//! dependencies hidden to break a cycle.
//!
//! Concurrent edits from linked devices whose values differ leave one displayed value on every
//! replica and the others as versions to choose from. Resolving writes the chosen value as a
//! local edit and seals the group, so the conflict closes on every device.
//!
//! A dependency that closes a cycle with concurrently added dependencies is hidden: it is absent
//! from the dependency table while its existence conflict stays open. Keeping it removed seals
//! the removal; restoring adds it again, which is refused while it would recreate the cycle.

use std::collections::HashSet;

use ganbaru_sync::manifest::vault::projects::{
    CHECKLIST_TABLE, DEPENDENCIES_TABLE, PROJECTS_TABLE, TASKS_TABLE, checklist_group,
    dependency_group, project_group, task_group,
};
use ganbaru_sync::{ConflictDetail, ConflictVersion, DeviceNames, DeviceRef, Engine, local};
use ganbaru_sync_contracts::{Field, GroupId, SpaceId, TableId, Value, WriterId};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::keys::dependency_key;
use crate::models::{ProjectTaskDependencyCreate, ProjectsMutationRows};
use crate::mutations::{
    checklist_item_mutation, dependency_would_cycle, project_mutation, task_mutation,
};
use crate::relationship_commands::create_task_dependency;
use crate::validation::require_non_empty;

/// Stable conflict outcomes, independent of diagnostic wording.
#[derive(Debug, Serialize)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum ProjectsConflictError {
    /// The conflict or the chosen version no longer exists; reload and show the current state.
    Resolved(String),
    /// Restoring the dependency would recreate the cycle it was hidden to break.
    WouldCycle(String),
    Failed(String),
}

impl From<String> for ProjectsConflictError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

/// A row kind that can hold a text conflict.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectsConflictTarget {
    Project,
    Task,
    ChecklistItem,
}

/// A text field that can hold a conflict.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectsConflictField {
    Name,
    Title,
    Description,
}

/// Where a conflicted field lives: its table, group, and column.
struct FieldSpec {
    table: TableId,
    group: GroupId,
    sql: &'static str,
}

impl ProjectsConflictTarget {
    fn table(self) -> TableId {
        match self {
            Self::Project => PROJECTS_TABLE,
            Self::Task => TASKS_TABLE,
            Self::ChecklistItem => CHECKLIST_TABLE,
        }
    }

    /// The conflict-capable fields of the row kind.
    fn fields(self) -> &'static [(ProjectsConflictField, GroupId)] {
        match self {
            Self::Project => &[(ProjectsConflictField::Name, project_group::NAME)],
            Self::Task => &[
                (ProjectsConflictField::Title, task_group::TITLE),
                (ProjectsConflictField::Description, task_group::DESCRIPTION),
            ],
            Self::ChecklistItem => &[(ProjectsConflictField::Title, checklist_group::TITLE)],
        }
    }

    fn field_spec(self, field: ProjectsConflictField) -> Option<FieldSpec> {
        let group = self
            .fields()
            .iter()
            .find(|(candidate, _)| *candidate == field)
            .map(|(_, group)| *group)?;
        let sql = match (self, field) {
            (Self::Project, _) => {
                "UPDATE projects
                 SET name = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?"
            }
            (Self::Task, ProjectsConflictField::Title) => {
                "UPDATE project_tasks
                 SET title = ?, revision = revision + 1,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?"
            }
            (Self::Task, _) => {
                "UPDATE project_tasks
                 SET description = ?, revision = revision + 1,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?"
            }
            (Self::ChecklistItem, _) => {
                "UPDATE project_checklist_items
                 SET title = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?"
            }
        };
        Some(FieldSpec {
            table: self.table(),
            group,
            sql,
        })
    }
}

/// One concurrent version of a text field.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectsConflictVersion {
    /// Opaque version id for resolving.
    version: String,
    device: DeviceRef,
    /// When the version was written, in Unix milliseconds.
    edited_at_ms: u64,
    /// Whether this version is the one every device shows.
    displayed: bool,
    text: String,
}

/// The versions of one text field in conflict, newest first.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectsFieldConflict {
    target: ProjectsConflictTarget,
    id: String,
    field: ProjectsConflictField,
    versions: Vec<ProjectsConflictVersion>,
}

/// A dependency hidden to break a cycle.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectsDependencyConflict {
    /// Dependency key, the id the dependency has while it exists.
    key: String,
    blocking_task_id: String,
    blocked_task_id: String,
    blocking_task_title: String,
    blocked_task_title: String,
    /// Whether adding the dependency again would close a cycle now.
    would_cycle: bool,
    /// Device that added the dependency.
    device: DeviceRef,
    /// When the dependency was added, in Unix milliseconds.
    added_at_ms: u64,
}

/// The conflicts of a project; empty when it has none.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectsConflictRead {
    project_id: String,
    fields: Vec<ProjectsFieldConflict>,
    dependencies: Vec<ProjectsDependencyConflict>,
}

/// How to resolve a text field in conflict.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectsFieldChoice {
    /// Keep the displayed version.
    Displayed,
    /// Use another version.
    Version { version: String },
}

/// How to resolve a hidden dependency.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectsDependencyChoice {
    /// Keep the dependency removed.
    KeepRemoved,
    /// Add the dependency again.
    Restore,
}

/// A conflict resolution request.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectsConflictResolution {
    Field {
        target: ProjectsConflictTarget,
        id: String,
        field: ProjectsConflictField,
        choice: ProjectsFieldChoice,
    },
    #[serde(rename_all = "camelCase")]
    Dependency {
        blocking_task_id: String,
        blocked_task_id: String,
        choice: ProjectsDependencyChoice,
    },
}

/// Text of a single-text group value.
fn text_from_value(value: &Value) -> Result<String, String> {
    match value.fields() {
        [Field::Text(text)] => Ok(text.clone()),
        _ => Err("projects text value is invalid".to_string()),
    }
}

/// The two task ids of a dependency identity value.
fn identity_from_value(value: &Value) -> Option<(String, String)> {
    match value.fields() {
        [Field::Text(blocking), Field::Text(blocked)] => Some((blocking.clone(), blocked.clone())),
        _ => None,
    }
}

/// The identity among `values` whose derived dependency key is `key`.
fn identity_for_key(values: &[Value], key: &str) -> Option<(String, String)> {
    values
        .iter()
        .filter_map(identity_from_value)
        .find(|(blocking, blocked)| dependency_key(blocking, blocked).as_deref() == Ok(key))
}

fn version_read(
    version: &ConflictVersion,
    devices: &DeviceNames,
) -> Result<ProjectsConflictVersion, String> {
    Ok(ProjectsConflictVersion {
        version: version.writer.to_hex(),
        device: devices.describe(&version.device_id),
        edited_at_ms: version.clock.physical_ms(),
        displayed: version.displayed,
        text: text_from_value(&version.value)?,
    })
}

/// The sync space of a vault that has a person identity.
async fn sync_space(
    tx: &mut Transaction<'_, Sqlite>,
    vault_id: &str,
) -> Result<SpaceId, ProjectsConflictError> {
    local::space_anchor(tx)
        .await
        .map_err(|error| format!("read sync space: {error}"))?
        .map(|_| SpaceId::personal(vault_id))
        .ok_or_else(|| ProjectsConflictError::Resolved("this vault does not sync".to_string()))
}

async fn details(
    tx: &mut Transaction<'_, Sqlite>,
    space: SpaceId,
    table: TableId,
    key: &str,
) -> Result<Vec<ConflictDetail>, ProjectsConflictError> {
    Ok(Engine::vault()
        .conflict_details(tx, space, table, key)
        .await
        .map_err(|error| format!("read projects conflicts: {error}"))?)
}

async fn ids(
    tx: &mut Transaction<'_, Sqlite>,
    sql: &str,
    project_id: &str,
) -> Result<HashSet<String>, String> {
    let rows: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(project_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| format!("read project rows: {error}"))?;
    Ok(rows.into_iter().collect())
}

/// Reads the conflicts of a project.
pub async fn conflicts_from_pool(
    pool: &SqlitePool,
    vault_id: &str,
    devices: &DeviceNames,
    project_id: &str,
) -> Result<ProjectsConflictRead, ProjectsConflictError> {
    require_non_empty(project_id, "project_id")?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin projects conflict read: {error}"))?;
    let space = sync_space(&mut tx, vault_id).await?;
    let engine = Engine::vault();
    let task_ids = ids(
        &mut tx,
        "SELECT id FROM project_tasks WHERE project_id = ?",
        project_id,
    )
    .await?;
    let checklist_ids = ids(
        &mut tx,
        "SELECT i.id FROM project_checklist_items i
         JOIN project_tasks t ON t.id = i.task_id WHERE t.project_id = ?",
        project_id,
    )
    .await?;
    let mut fields = Vec::new();
    for (target, scope) in [
        (
            ProjectsConflictTarget::Project,
            HashSet::from([project_id.to_string()]),
        ),
        (ProjectsConflictTarget::Task, task_ids.clone()),
        (ProjectsConflictTarget::ChecklistItem, checklist_ids),
    ] {
        let open = engine
            .open_conflicts(&mut tx, space, target.table())
            .await
            .map_err(|error| format!("read projects conflicts: {error}"))?;
        for row in open.into_iter().filter(|row| scope.contains(&row.row_key)) {
            for detail in row.groups {
                let Some((field, _)) = target
                    .fields()
                    .iter()
                    .find(|(_, group)| *group == detail.group)
                else {
                    continue;
                };
                let versions = detail
                    .versions
                    .iter()
                    .map(|version| version_read(version, devices))
                    .collect::<Result<Vec<_>, _>>()?;
                fields.push(ProjectsFieldConflict {
                    target,
                    id: row.row_key.clone(),
                    field: *field,
                    versions,
                });
            }
        }
    }
    let mut dependencies = Vec::new();
    let hidden = engine
        .open_conflicts(&mut tx, space, DEPENDENCIES_TABLE)
        .await
        .map_err(|error| format!("read projects conflicts: {error}"))?;
    if !hidden.is_empty() {
        let identities = local::retained_group_values(
            &engine,
            &mut tx,
            DEPENDENCIES_TABLE,
            dependency_group::IDENTITY,
        )
        .await
        .map_err(|error| format!("read hidden dependency identities: {error}"))?;
        for row in hidden {
            let Some((blocking, blocked)) = identity_for_key(&identities, &row.row_key) else {
                continue;
            };
            if !task_ids.contains(&blocking) || !task_ids.contains(&blocked) {
                continue;
            }
            let Some(added) = row
                .groups
                .iter()
                .find(|detail| detail.group == dependency_group::EXISTS)
                .and_then(|detail| detail.versions.first())
            else {
                continue;
            };
            dependencies.push(ProjectsDependencyConflict {
                would_cycle: dependency_would_cycle(&mut tx, &blocking, &blocked).await?,
                blocking_task_title: task_title(&mut tx, &blocking).await?,
                blocked_task_title: task_title(&mut tx, &blocked).await?,
                key: row.row_key,
                blocking_task_id: blocking,
                blocked_task_id: blocked,
                device: devices.describe(&added.device_id),
                added_at_ms: added.clock.physical_ms(),
            });
        }
    }
    tx.rollback()
        .await
        .map_err(|error| format!("end projects conflict read: {error}"))?;
    Ok(ProjectsConflictRead {
        project_id: project_id.to_string(),
        fields,
        dependencies,
    })
}

async fn task_title(tx: &mut Transaction<'_, Sqlite>, task_id: &str) -> Result<String, String> {
    sqlx::query_scalar("SELECT title FROM project_tasks WHERE id = ?")
        .bind(task_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| format!("read task title: {error}"))
}

/// Resolves a conflict and returns the rows it changed. A text field's chosen value is written
/// as a local edit and the field is sealed even when its value did not change, so the write
/// covers every version it saw.
pub async fn resolve_from_pool(
    pool: &SqlitePool,
    vault_id: &str,
    request: ProjectsConflictResolution,
    now_ms: u64,
) -> Result<ProjectsMutationRows, ProjectsConflictError> {
    match request {
        ProjectsConflictResolution::Field {
            target,
            id,
            field,
            choice,
        } => resolve_field(pool, vault_id, target, &id, field, choice, now_ms).await,
        ProjectsConflictResolution::Dependency {
            blocking_task_id,
            blocked_task_id,
            choice,
        } => resolve_dependency(pool, vault_id, &blocking_task_id, &blocked_task_id, choice).await,
    }
}

async fn resolve_field(
    pool: &SqlitePool,
    vault_id: &str,
    target: ProjectsConflictTarget,
    id: &str,
    field: ProjectsConflictField,
    choice: ProjectsFieldChoice,
    now_ms: u64,
) -> Result<ProjectsMutationRows, ProjectsConflictError> {
    require_non_empty(id, "id")?;
    let spec = target
        .field_spec(field)
        .ok_or_else(|| "this field cannot hold a conflict".to_string())?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin projects conflict resolution: {error}"))?;
    let space = sync_space(&mut tx, vault_id).await?;
    let detail = details(&mut tx, space, spec.table, id)
        .await?
        .into_iter()
        .find(|detail| detail.group == spec.group)
        .ok_or_else(|| ProjectsConflictError::Resolved("the conflict is resolved".to_string()))?;
    if let ProjectsFieldChoice::Version { version } = &choice {
        let writer = WriterId::from_hex(version);
        let chosen = detail
            .versions
            .iter()
            .find(|candidate| Some(candidate.writer) == writer)
            .ok_or_else(|| ProjectsConflictError::Resolved("the version is gone".to_string()))?;
        let result = sqlx::query(spec.sql)
            .bind(text_from_value(&chosen.value)?)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|error| format!("write projects conflict choice: {error}"))?;
        if result.rows_affected() != 1 {
            return Err("the row was not found".to_string().into());
        }
    }
    Engine::vault()
        .force_group(&mut tx, spec.table, id, spec.group, now_ms)
        .await
        .map_err(|error| format!("mark projects conflict resolved: {error}"))?;
    let task_id = match target {
        ProjectsConflictTarget::ChecklistItem => Some(
            sqlx::query_scalar::<_, String>(
                "SELECT task_id FROM project_checklist_items WHERE id = ?",
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|error| format!("read checklist item task: {error}"))?,
        ),
        _ => None,
    };
    tx.commit()
        .await
        .map_err(|error| format!("commit projects conflict resolution: {error}"))?;
    Ok(match (target, task_id) {
        (ProjectsConflictTarget::Project, _) => project_mutation(pool, id).await?,
        (ProjectsConflictTarget::Task, _) => task_mutation(pool, id).await?,
        (ProjectsConflictTarget::ChecklistItem, task_id) => {
            let task_id = task_id.unwrap_or_default();
            checklist_item_mutation(pool, id, &task_id).await?
        }
    })
}

async fn resolve_dependency(
    pool: &SqlitePool,
    vault_id: &str,
    blocking_task_id: &str,
    blocked_task_id: &str,
    choice: ProjectsDependencyChoice,
) -> Result<ProjectsMutationRows, ProjectsConflictError> {
    require_non_empty(blocking_task_id, "blocking_task_id")?;
    require_non_empty(blocked_task_id, "blocked_task_id")?;
    let key = dependency_key(blocking_task_id, blocked_task_id)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin projects conflict resolution: {error}"))?;
    let space = sync_space(&mut tx, vault_id).await?;
    let open = details(&mut tx, space, DEPENDENCIES_TABLE, &key).await?;
    if !open
        .iter()
        .any(|detail| detail.group == dependency_group::EXISTS)
    {
        return Err(ProjectsConflictError::Resolved(
            "the conflict is resolved".to_string(),
        ));
    }
    match choice {
        ProjectsDependencyChoice::KeepRemoved => {
            // The hidden dependency has no row to edit. Inserting and deleting it again seals
            // an existence write of zero, which every version of the conflict is covered by.
            for sql in [
                "INSERT INTO project_task_dependencies
                    (id, sync_key, blocking_task_id, blocked_task_id, dependency_type)
                 VALUES (?, ?, ?, ?, 'blocks')",
                "DELETE FROM project_task_dependencies WHERE id = ?",
            ] {
                let query = sqlx::query(sql);
                let query = if sql.starts_with("INSERT") {
                    query
                        .bind(&key)
                        .bind(&key)
                        .bind(blocking_task_id)
                        .bind(blocked_task_id)
                } else {
                    query.bind(&key)
                };
                query
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| format!("keep dependency removed: {error}"))?;
            }
            tx.commit()
                .await
                .map_err(|error| format!("commit projects conflict resolution: {error}"))?;
            let mut mutation = ProjectsMutationRows::default();
            mutation
                .removals
                .push(crate::ProjectMutationRemoval::Dependency { id: key });
            Ok(mutation)
        }
        ProjectsDependencyChoice::Restore => {
            if dependency_would_cycle(&mut tx, blocking_task_id, blocked_task_id).await? {
                return Err(ProjectsConflictError::WouldCycle(
                    "restoring the dependency would create a cycle".to_string(),
                ));
            }
            tx.rollback()
                .await
                .map_err(|error| format!("end projects conflict resolution: {error}"))?;
            Ok(create_task_dependency(
                pool,
                ProjectTaskDependencyCreate {
                    blocking_task_id: blocking_task_id.to_string(),
                    blocked_task_id: blocked_task_id.to_string(),
                    dependency_type: "blocks".to_string(),
                },
            )
            .await?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(blocking: &str, blocked: &str) -> Value {
        Value::new(vec![
            Field::Text(blocking.to_string()),
            Field::Text(blocked.to_string()),
        ])
        .expect("an identity value fits the bounds")
    }

    #[test]
    fn identity_for_key_matches_the_pair_whose_derived_key_is_the_row_key() {
        let values = [identity("task-a", "task-b"), identity("task-b", "task-a")];
        let key = dependency_key("task-b", "task-a").unwrap();
        assert_eq!(
            identity_for_key(&values, &key),
            Some(("task-b".to_string(), "task-a".to_string()))
        );
    }

    #[test]
    fn identity_for_key_ignores_values_that_are_not_pairs_of_text() {
        let malformed = Value::new(vec![Field::Integer(1)]).unwrap();
        let key = dependency_key("task-a", "task-b").unwrap();
        assert_eq!(identity_for_key(&[malformed], &key), None);
        assert_eq!(identity_for_key(&[], &key), None);
    }

    #[test]
    fn text_from_value_reads_one_text_field_only() {
        assert_eq!(
            text_from_value(&Value::new(vec![Field::Text("Launch".to_string())]).unwrap()),
            Ok("Launch".to_string())
        );
        assert!(text_from_value(&identity("a", "b")).is_err());
    }

    #[test]
    fn fields_map_to_their_groups_and_updates() {
        assert!(
            ProjectsConflictTarget::Task
                .field_spec(ProjectsConflictField::Description)
                .is_some()
        );
        assert!(
            ProjectsConflictTarget::Project
                .field_spec(ProjectsConflictField::Description)
                .is_none()
        );
        assert!(
            ProjectsConflictTarget::ChecklistItem
                .field_spec(ProjectsConflictField::Title)
                .is_some()
        );
    }
}
