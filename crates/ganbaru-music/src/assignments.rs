//! Portable music-assignment contracts shared by desktop and mobile planning.

use crate::error::{MusicLibraryError, MusicLibraryResult};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use sqlx::{Sqlite, Transaction};
use std::collections::{HashMap, HashSet};

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
        pub enum $name {
            $(#[serde(rename = $value)] $variant),+
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                match self {
                    $(Self::$variant => $value),+
                }
            }
        }

        impl TryFrom<&str> for $name {
            type Error = String;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                match value {
                    $($value => Ok(Self::$variant),)+
                    _ => Err(format!("unknown {} '{value}'", stringify!($name))),
                }
            }
        }
    };
}

string_enum!(MusicActivityPhase {
    Focus => "focus",
    ShortBreak => "short-break",
    LongBreak => "long-break",
});
string_enum!(MusicAssignmentBehavior {
    Inherit => "inherit",
    PlayAutomatically => "play-automatically",
    PrepareSilently => "prepare-silently",
    PauseMusic => "pause-music",
    KeepCurrentMusic => "keep-current-music",
});
string_enum!(MusicSoundscapeBehavior {
    Inherit => "inherit",
    PlaySelected => "play-selected",
    PauseSoundscape => "pause-soundscape",
    KeepCurrentSoundscape => "keep-current-soundscape",
});
string_enum!(MusicAssignmentOwnerKind {
    ProjectDefault => "project-default",
    EventSnapshot => "event-snapshot",
    EventOverride => "event-override",
    WorkEnvironment => "work-environment",
});
string_enum!(MusicAssignmentProvenanceKind {
    Explicit => "explicit",
    CopiedProject => "copied-project",
    WorkEnvironment => "work-environment",
});

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicContextAssignment {
    pub owner_kind: MusicAssignmentOwnerKind,
    pub owner_id: String,
    pub phase: MusicActivityPhase,
    pub behavior: MusicAssignmentBehavior,
    pub playlist_id: Option<String>,
    pub soundscape_id: Option<String>,
    pub soundscape_behavior: MusicSoundscapeBehavior,
    pub provenance_kind: MusicAssignmentProvenanceKind,
    pub provenance_id: Option<String>,
    pub updated_at_ms: i64,
    pub version: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicContextAssignmentDraft {
    pub phase: MusicActivityPhase,
    pub behavior: MusicAssignmentBehavior,
    pub playlist_id: Option<String>,
    pub soundscape_id: Option<String>,
    pub soundscape_behavior: MusicSoundscapeBehavior,
    pub provenance_kind: MusicAssignmentProvenanceKind,
    pub provenance_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicContextAssignmentSet {
    pub owner_kind: MusicAssignmentOwnerKind,
    pub owner_id: String,
    pub assignments: Vec<MusicContextAssignmentDraft>,
    pub updated_at_ms: i64,
}

/// Canonical assignment fields shared by context reads and bounded transfers.
pub type AssignmentRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    Option<String>,
    i64,
    i64,
);

/// The assignments of one owner, ordered by phase.
pub async fn load_assignments(
    pool: &SqlitePool,
    owner_kind: MusicAssignmentOwnerKind,
    owner_id: &str,
) -> MusicLibraryResult<Vec<MusicContextAssignment>> {
    validate_id(owner_id, "ownerId")?;
    let rows = sqlx::query_as::<_, AssignmentRow>(
        "SELECT owner_kind, owner_id, phase, behavior, playlist_id, soundscape_id, soundscape_behavior,
                provenance_kind, provenance_id, updated_at_ms, version
         FROM music_context_assignments
         WHERE owner_kind = ? AND owner_id = ?
         ORDER BY CASE phase WHEN 'focus' THEN 0 WHEN 'short-break' THEN 1 ELSE 2 END",
    )
    .bind(owner_kind.as_ref())
    .bind(owner_id)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load music context assignments", error))?;
    rows.into_iter().map(decode).collect()
}

#[cfg(test)]
async fn replace_assignments(
    pool: &SqlitePool,
    request: MusicContextAssignmentSet,
) -> MusicLibraryResult<Vec<MusicContextAssignment>> {
    validate_set(&request)?;
    let mut transaction = pool.begin().await.map_err(|error| {
        MusicLibraryError::database("begin music context assignment update", error)
    })?;
    replace_assignments_in_transaction(
        &mut transaction,
        request.owner_kind,
        &request.owner_id,
        request.assignments,
        request.updated_at_ms,
    )
    .await?;
    transaction.commit().await.map_err(|error| {
        MusicLibraryError::database("commit music context assignment update", error)
    })?;
    load_assignments(pool, request.owner_kind, &request.owner_id).await
}

/// Replaces every phase assignment of one owner inside the caller's transaction, advancing each
/// phase's version.
pub async fn replace_assignments_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    owner_kind: MusicAssignmentOwnerKind,
    owner_id: &str,
    assignments: Vec<MusicContextAssignmentDraft>,
    updated_at_ms: i64,
) -> MusicLibraryResult<()> {
    validate_set(&MusicContextAssignmentSet {
        owner_kind,
        owner_id: owner_id.to_string(),
        assignments: assignments.clone(),
        updated_at_ms,
    })?;
    let prior_versions = sqlx::query_as::<_, (String, i64)>(
        "SELECT phase, version FROM music_context_assignments
         WHERE owner_kind = ? AND owner_id = ?",
    )
    .bind(owner_kind.as_ref())
    .bind(owner_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load music context assignment versions", error))?
    .into_iter()
    .collect::<HashMap<_, _>>();
    sqlx::query("DELETE FROM music_context_assignments WHERE owner_kind = ? AND owner_id = ?")
        .bind(owner_kind.as_ref())
        .bind(owner_id)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("replace music context assignments", error))?;
    for assignment in assignments {
        let version = prior_versions
            .get(assignment.phase.as_ref())
            .copied()
            .unwrap_or(0)
            + 1;
        sqlx::query(
            "INSERT INTO music_context_assignments
                (owner_kind, owner_id, phase, behavior, playlist_id, soundscape_id, soundscape_behavior,
                 provenance_kind, provenance_id, updated_at_ms, version)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(owner_kind.as_ref())
        .bind(owner_id)
        .bind(assignment.phase.as_ref())
        .bind(assignment.behavior.as_ref())
        .bind(assignment.playlist_id)
        .bind(assignment.soundscape_id)
        .bind(assignment.soundscape_behavior.as_ref())
        .bind(assignment.provenance_kind.as_ref())
        .bind(assignment.provenance_id)
        .bind(updated_at_ms)
        .bind(version)
        .execute(&mut **transaction)
        .await
        .map_err(|error| {
            MusicLibraryError::database("write music context assignment", error)
        })?;
    }
    Ok(())
}

/// Validates an owner, its timestamp, and its phase assignments before a replacement.
pub fn validate_set(request: &MusicContextAssignmentSet) -> MusicLibraryResult<()> {
    validate_id(&request.owner_id, "ownerId")?;
    if request.updated_at_ms <= 0 {
        return Err(MusicLibraryError::validation(
            "updatedAt",
            "must be positive",
        ));
    }
    validate_drafts(request.owner_kind, &request.assignments)
}

/// Validate portable assignment intent before an owner or timestamp is allocated.
pub fn validate_drafts(
    owner_kind: MusicAssignmentOwnerKind,
    assignments: &[MusicContextAssignmentDraft],
) -> MusicLibraryResult<()> {
    if assignments.len() > 3 {
        return Err(MusicLibraryError::validation(
            "assignments",
            "cannot contain more than three phases",
        ));
    }
    let mut phases = HashSet::new();
    for assignment in assignments {
        if !phases.insert(assignment.phase) {
            return Err(MusicLibraryError::validation(
                "assignments",
                "cannot contain a phase more than once",
            ));
        }
        validate_optional_id(&assignment.playlist_id, "playlistId")?;
        validate_optional_id(&assignment.soundscape_id, "soundscapeId")?;
        validate_optional_id(&assignment.provenance_id, "provenanceId")?;
        let expected_provenance = match owner_kind {
            MusicAssignmentOwnerKind::EventSnapshot => MusicAssignmentProvenanceKind::CopiedProject,
            MusicAssignmentOwnerKind::WorkEnvironment => {
                MusicAssignmentProvenanceKind::WorkEnvironment
            }
            MusicAssignmentOwnerKind::ProjectDefault | MusicAssignmentOwnerKind::EventOverride => {
                MusicAssignmentProvenanceKind::Explicit
            }
        };
        if assignment.provenance_kind != expected_provenance {
            return Err(MusicLibraryError::validation(
                "provenanceKind",
                format!(
                    "{} assignments require {} provenance",
                    owner_kind.as_ref(),
                    expected_provenance.as_ref()
                ),
            ));
        }
    }
    Ok(())
}

fn validate_id(value: &str, field: &str) -> MusicLibraryResult<()> {
    const MAX_ID_BYTES: usize = 200;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(MusicLibraryError::validation(field, "is required"));
    }
    if trimmed.len() > MAX_ID_BYTES {
        return Err(MusicLibraryError::validation(
            field,
            format!("exceeds the {MAX_ID_BYTES} byte limit"),
        ));
    }
    Ok(())
}

fn validate_optional_id(value: &Option<String>, field: &str) -> MusicLibraryResult<()> {
    if let Some(value) = value {
        validate_id(value, field)?;
    }
    Ok(())
}

/// Decode persisted assignment variants consistently at both native boundaries.
pub fn decode(row: AssignmentRow) -> MusicLibraryResult<MusicContextAssignment> {
    Ok(MusicContextAssignment {
        owner_kind: MusicAssignmentOwnerKind::try_from(row.0.as_str())
            .map_err(|message| MusicLibraryError::runtime("decode assignment owner", message))?,
        owner_id: row.1,
        phase: MusicActivityPhase::try_from(row.2.as_str())
            .map_err(|message| MusicLibraryError::runtime("decode assignment phase", message))?,
        behavior: MusicAssignmentBehavior::try_from(row.3.as_str())
            .map_err(|message| MusicLibraryError::runtime("decode assignment behavior", message))?,
        playlist_id: row.4,
        soundscape_id: row.5,
        soundscape_behavior: MusicSoundscapeBehavior::try_from(row.6.as_str())
            .map_err(|message| MusicLibraryError::runtime("decode soundscape behavior", message))?,
        provenance_kind: MusicAssignmentProvenanceKind::try_from(row.7.as_str()).map_err(
            |message| MusicLibraryError::runtime("decode assignment provenance", message),
        )?,
        provenance_id: row.8,
        updated_at_ms: row.9,
        version: row.10,
    })
}

#[cfg(test)]
mod tests;
