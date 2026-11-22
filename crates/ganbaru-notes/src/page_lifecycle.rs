//! The lifecycle of a Notes page and its guarded writers.
//!
//! A page is live, archived, or in Trash. `notes_pages` stores the state in `in_trash`,
//! `archived`, `trashed_time`, and `trash_owner`. Trash, its time, and the deletion token that
//! owns it replicate as one coupled value, so a CHECK constraint ties those three columns
//! together: a trashed page always has a trash time, and a page outside Trash has neither a
//! time nor an owner. Archive replicates as a separate value, so a trashed page may keep
//! `archived = 1`, which means a restore returns it to Archive. Every local writer of these
//! columns runs one [`PageMove`] through [`PageLifecycle::transition`] and writes it with a
//! guard on the legal source, so a row in any other state is left alone.

use sqlx::{Sqlite, Transaction};

/// Where a trashed page returns when it is restored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreTo {
    /// Back among live pages.
    Live,
    /// Back into Archive.
    Archived,
}

/// The lifecycle state of one page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageLifecycle {
    /// Shown in the page tree.
    Live,
    /// Hidden in Archive.
    Archived,
    /// In Trash since `since`, owned by the deletion token `owner` when a graph trash moved it.
    Trashed {
        owner: Option<String>,
        since: String,
        restore_to: RestoreTo,
    },
}

/// The stored lifecycle columns of a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageLifecycleColumns {
    pub in_trash: i64,
    pub archived: i64,
    pub trashed_time: Option<String>,
    pub trash_owner: Option<String>,
}

/// A requested lifecycle move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageMove {
    /// Move a live page into Archive.
    Archive,
    /// Move an archived page back among live pages.
    Unarchive,
    /// Move a page outside Trash into it. `keep_archive` keeps an archived page's way back into
    /// Archive; otherwise a restore returns it among live pages.
    Trash {
        owner: Option<String>,
        since: String,
        keep_archive: bool,
    },
    /// Take a trashed page out of Trash. With `owner`, only a page trashed by that deletion
    /// token moves.
    Restore { owner: Option<String> },
    /// Make the page live from any state, as history restores do.
    Revive,
}

/// The legal result of a move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageStep {
    /// The page moves to this state.
    Moves(PageLifecycle),
    /// The page is already where the move leads.
    AlreadyInState,
}

/// The result of a guarded lifecycle write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageLifecycleWrite {
    /// The page moved.
    Applied,
    /// The page was already where the move leads, so nothing changed.
    AlreadyInState,
    /// The move is not legal from the page's current state.
    Rejected { from: PageLifecycle },
    /// No page has the id.
    Missing,
}

impl PageLifecycleWrite {
    pub fn is_applied(&self) -> bool {
        matches!(self, Self::Applied)
    }
}

impl PageLifecycle {
    /// Decodes the stored columns, refusing a combination the CHECK constraint forbids.
    pub fn from_columns(columns: PageLifecycleColumns) -> Result<Self, String> {
        let restore_to = match columns.archived {
            0 => RestoreTo::Live,
            1 => RestoreTo::Archived,
            other => return Err(format!("notes page archived flag {other} is not 0 or 1")),
        };
        match (columns.in_trash, columns.trashed_time, columns.trash_owner) {
            (0, None, None) => Ok(match restore_to {
                RestoreTo::Live => Self::Live,
                RestoreTo::Archived => Self::Archived,
            }),
            (1, Some(since), owner) => Ok(Self::Trashed {
                owner,
                since,
                restore_to,
            }),
            (in_trash, trashed_time, owner) => Err(format!(
                "notes page lifecycle contradicts itself: in_trash {in_trash}, trash time {}, trash owner {}",
                if trashed_time.is_some() {
                    "set"
                } else {
                    "missing"
                },
                if owner.is_some() { "set" } else { "missing" },
            )),
        }
    }

    /// The stored columns of this state.
    pub fn columns(&self) -> PageLifecycleColumns {
        match self {
            Self::Live => PageLifecycleColumns {
                in_trash: 0,
                archived: 0,
                trashed_time: None,
                trash_owner: None,
            },
            Self::Archived => PageLifecycleColumns {
                in_trash: 0,
                archived: 1,
                trashed_time: None,
                trash_owner: None,
            },
            Self::Trashed {
                owner,
                since,
                restore_to,
            } => PageLifecycleColumns {
                in_trash: 1,
                archived: i64::from(*restore_to == RestoreTo::Archived),
                trashed_time: Some(since.clone()),
                trash_owner: owner.clone(),
            },
        }
    }

    /// Applies a move, or returns `None` when the move is not legal from this state.
    pub fn transition(&self, request: &PageMove) -> Option<PageStep> {
        use PageLifecycle::{Archived, Live, Trashed};
        match (self, request) {
            (Live, PageMove::Archive) => Some(PageStep::Moves(Archived)),
            (Archived, PageMove::Archive) => Some(PageStep::AlreadyInState),
            (Archived, PageMove::Unarchive) => Some(PageStep::Moves(Live)),
            (Live, PageMove::Unarchive) => Some(PageStep::AlreadyInState),
            (Trashed { .. }, PageMove::Archive | PageMove::Unarchive) => None,
            (
                Live | Archived,
                PageMove::Trash {
                    owner,
                    since,
                    keep_archive,
                },
            ) => Some(PageStep::Moves(Trashed {
                owner: owner.clone(),
                since: since.clone(),
                restore_to: if *self == Archived && *keep_archive {
                    RestoreTo::Archived
                } else {
                    RestoreTo::Live
                },
            })),
            (Trashed { .. }, PageMove::Trash { .. }) => Some(PageStep::AlreadyInState),
            (
                Trashed {
                    owner, restore_to, ..
                },
                PageMove::Restore { owner: wanted },
            ) => {
                if wanted.is_some() && wanted != owner {
                    return None;
                }
                Some(PageStep::Moves(match restore_to {
                    RestoreTo::Live => Live,
                    RestoreTo::Archived => Archived,
                }))
            }
            (Live | Archived, PageMove::Restore { .. }) => Some(PageStep::AlreadyInState),
            (Live, PageMove::Revive) => Some(PageStep::AlreadyInState),
            (Archived | Trashed { .. }, PageMove::Revive) => Some(PageStep::Moves(Live)),
        }
    }
}

/// Loads the lifecycle of a page, or `None` when no page has the id.
pub async fn load_page_lifecycle_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<Option<PageLifecycle>, String> {
    let row: Option<(i64, i64, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT in_trash, archived, trashed_time, trash_owner FROM notes_pages WHERE id = ?",
    )
    .bind(page_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes page lifecycle: {e}"))?;
    row.map(|(in_trash, archived, trashed_time, trash_owner)| {
        PageLifecycle::from_columns(PageLifecycleColumns {
            in_trash,
            archived,
            trashed_time,
            trash_owner,
        })
    })
    .transpose()
}

/// The current time in the stored timestamp format.
pub async fn lifecycle_now_tx(tx: &mut Transaction<'_, Sqlite>) -> Result<String, String> {
    sqlx::query_scalar("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')")
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("read notes lifecycle time: {e}"))
}

/// Applies one move to a page inside the caller's transaction.
///
/// The write is guarded by the columns it was decided from, so a page that changed in between
/// is reported as rejected and left alone. A move also touches `last_edited_time`.
pub async fn move_page_lifecycle_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    request: &PageMove,
) -> Result<PageLifecycleWrite, String> {
    let Some(from) = load_page_lifecycle_tx(tx, page_id).await? else {
        return Ok(PageLifecycleWrite::Missing);
    };
    let to = match from.transition(request) {
        None => return Ok(PageLifecycleWrite::Rejected { from }),
        Some(PageStep::AlreadyInState) => return Ok(PageLifecycleWrite::AlreadyInState),
        Some(PageStep::Moves(to)) => to,
    };
    let source = from.columns();
    let target = to.columns();
    let written = sqlx::query(
        "UPDATE notes_pages
         SET in_trash = ?, archived = ?, trashed_time = ?, trash_owner = ?,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND in_trash = ? AND archived = ?
           AND trashed_time IS ? AND trash_owner IS ?",
    )
    .bind(target.in_trash)
    .bind(target.archived)
    .bind(&target.trashed_time)
    .bind(&target.trash_owner)
    .bind(page_id)
    .bind(source.in_trash)
    .bind(source.archived)
    .bind(&source.trashed_time)
    .bind(&source.trash_owner)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("write notes page lifecycle: {e}"))?;
    if written.rows_affected() == 0 {
        return Ok(PageLifecycleWrite::Rejected { from });
    }
    Ok(PageLifecycleWrite::Applied)
}

/// Trashes pages a history restore leaves out, with no deletion token and no way back into
/// Archive. Pages already in Trash keep their trash time and owner.
pub async fn trash_pages_without_owner_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_ids: &[String],
) -> Result<(), String> {
    if page_ids.is_empty() {
        return Ok(());
    }
    let request = PageMove::Trash {
        owner: None,
        since: lifecycle_now_tx(tx).await?,
        keep_archive: false,
    };
    for page_id in page_ids {
        move_page_lifecycle_tx(tx, page_id, &request).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SINCE: &str = "2026-11-21T00:00:00.000Z";

    fn trashed(owner: Option<&str>, restore_to: RestoreTo) -> PageLifecycle {
        PageLifecycle::Trashed {
            owner: owner.map(str::to_string),
            since: SINCE.to_string(),
            restore_to,
        }
    }

    fn trash(owner: Option<&str>, keep_archive: bool) -> PageMove {
        PageMove::Trash {
            owner: owner.map(str::to_string),
            since: SINCE.to_string(),
            keep_archive,
        }
    }

    fn all_states() -> Vec<PageLifecycle> {
        vec![
            PageLifecycle::Live,
            PageLifecycle::Archived,
            trashed(None, RestoreTo::Live),
            trashed(None, RestoreTo::Archived),
            trashed(Some("token"), RestoreTo::Live),
            trashed(Some("token"), RestoreTo::Archived),
        ]
    }

    #[test]
    fn columns_round_trip_for_every_state() {
        for state in all_states() {
            assert_eq!(
                PageLifecycle::from_columns(state.columns()).as_ref(),
                Ok(&state)
            );
        }
    }

    #[test]
    fn columns_the_check_forbids_are_refused() {
        let impossible = [
            (1, 0, None, None),
            (1, 1, None, Some("token")),
            (0, 0, Some(SINCE), None),
            (0, 0, None, Some("token")),
            (0, 1, Some(SINCE), Some("token")),
            (2, 0, Some(SINCE), None),
            (0, 2, None, None),
        ];
        for (in_trash, archived, trashed_time, trash_owner) in impossible {
            let columns = PageLifecycleColumns {
                in_trash,
                archived,
                trashed_time: trashed_time.map(str::to_string),
                trash_owner: trash_owner.map(str::to_string),
            };
            assert!(
                PageLifecycle::from_columns(columns.clone()).is_err(),
                "{columns:?} should be refused"
            );
        }
    }

    #[test]
    fn every_move_from_every_state_follows_the_table() {
        use PageLifecycle::{Archived, Live};
        let moved = |state| Some(PageStep::Moves(state));
        let same = Some(PageStep::AlreadyInState);
        let cases: Vec<(PageLifecycle, PageMove, Option<PageStep>)> = vec![
            (Live, PageMove::Archive, moved(Archived)),
            (Archived, PageMove::Archive, same.clone()),
            (trashed(None, RestoreTo::Live), PageMove::Archive, None),
            (Archived, PageMove::Unarchive, moved(Live)),
            (Live, PageMove::Unarchive, same.clone()),
            (
                trashed(None, RestoreTo::Archived),
                PageMove::Unarchive,
                None,
            ),
            (
                Live,
                trash(Some("token"), true),
                moved(trashed(Some("token"), RestoreTo::Live)),
            ),
            (
                Archived,
                trash(Some("token"), true),
                moved(trashed(Some("token"), RestoreTo::Archived)),
            ),
            (
                Archived,
                trash(None, false),
                moved(trashed(None, RestoreTo::Live)),
            ),
            (
                trashed(Some("first"), RestoreTo::Live),
                trash(Some("second"), true),
                same.clone(),
            ),
            (
                trashed(Some("token"), RestoreTo::Live),
                PageMove::Restore { owner: None },
                moved(Live),
            ),
            (
                trashed(Some("token"), RestoreTo::Archived),
                PageMove::Restore {
                    owner: Some("token".to_string()),
                },
                moved(Archived),
            ),
            (
                trashed(Some("other"), RestoreTo::Live),
                PageMove::Restore {
                    owner: Some("token".to_string()),
                },
                None,
            ),
            (
                trashed(None, RestoreTo::Live),
                PageMove::Restore {
                    owner: Some("token".to_string()),
                },
                None,
            ),
            (Live, PageMove::Restore { owner: None }, same.clone()),
            (Archived, PageMove::Restore { owner: None }, same.clone()),
            (Live, PageMove::Revive, same.clone()),
            (Archived, PageMove::Revive, moved(Live)),
            (
                trashed(Some("token"), RestoreTo::Archived),
                PageMove::Revive,
                moved(Live),
            ),
        ];
        for (from, request, expected) in cases {
            assert_eq!(
                from.transition(&request),
                expected,
                "{request:?} from {from:?}"
            );
        }
    }

    #[test]
    fn every_reached_state_satisfies_the_column_check() {
        let requests = [
            PageMove::Archive,
            PageMove::Unarchive,
            trash(None, true),
            trash(Some("token"), false),
            PageMove::Restore { owner: None },
            PageMove::Restore {
                owner: Some("token".to_string()),
            },
            PageMove::Revive,
        ];
        for from in all_states() {
            for request in &requests {
                if let Some(PageStep::Moves(to)) = from.transition(request) {
                    assert_eq!(PageLifecycle::from_columns(to.columns()).as_ref(), Ok(&to));
                }
            }
        }
    }
}
