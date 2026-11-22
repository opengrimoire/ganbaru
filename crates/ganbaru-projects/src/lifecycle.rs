//! Visibility lifecycle shared by projects and sections: active, hidden, or archived, with one
//! guarded transition per row kind.

use serde::Deserialize;
use sqlx::SqlitePool;

use super::models::ProjectsMutationRows;
use super::mutations::project_mutation;
use super::validation::require_non_empty;

/// Where a project or section sits in its visibility lifecycle.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Lifecycle {
    Active,
    Hidden,
    Archived,
}

impl Lifecycle {
    /// The persisted name, as the `projects.status` column stores it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Hidden => "hidden",
            Self::Archived => "archived",
        }
    }

    /// Moves to `next`. Every state reaches every other one, and staying put is refused so a
    /// caller that lost track of the current state learns it instead of rewriting the row.
    pub fn transition(self, next: Self) -> Result<Self, String> {
        if self == next {
            return Err(format!("already {}", self.as_str()));
        }
        Ok(next)
    }
}

/// A requested project lifecycle move, refused when the project no longer has `expected`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLifecycleChange {
    pub project_id: String,
    pub expected: Lifecycle,
    pub next: Lifecycle,
}

/// Moves a project to `change.next`, guarded on the state the caller last read.
pub async fn set_project_lifecycle(
    pool: &SqlitePool,
    change: ProjectLifecycleChange,
) -> Result<ProjectsMutationRows, String> {
    require_non_empty(&change.project_id, "project_id")?;
    let next = change.expected.transition(change.next)?;
    let result = sqlx::query(
        "UPDATE projects
         SET status = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND status = ?",
    )
    .bind(next.as_str())
    .bind(&change.project_id)
    .bind(change.expected.as_str())
    .execute(pool)
    .await
    .map_err(|e| format!("update project status: {e}"))?;
    if result.rows_affected() == 0 {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM projects WHERE id = ?)")
                .bind(&change.project_id)
                .fetch_one(pool)
                .await
                .map_err(|e| format!("load project status: {e}"))?;
        return Err(if exists {
            "the project status changed since it was read".to_string()
        } else {
            "project not found".to_string()
        });
    }
    project_mutation(pool, &change.project_id).await
}
