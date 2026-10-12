use super::*;
use ganbaru_db::run_migrations;
use sqlx::sqlite::SqlitePoolOptions;

/// Runs persistence tests on a single-thread runtime without platform initialization.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create quick notes test runtime")
        .block_on(future)
}

async fn pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    run_migrations(&pool).await.unwrap();
    pool
}

fn runs(content: &str) -> Vec<QuickNoteTextRun> {
    vec![QuickNoteTextRun {
        content: content.to_string(),
        bold: false,
        italic: false,
        underline: false,
    }]
}

fn note(id: &str, body: &str) -> QuickNoteWrite {
    QuickNoteWrite {
        id: id.to_string(),
        title: id.to_string(),
        runs: runs(body),
        color: 0,
        tag_id: None,
        pinned: false,
    }
}

async fn create(pool: &SqlitePool, id: &str, body: &str) -> QuickNoteRead {
    create_from_pool(pool, note(id, body)).await.unwrap()
}

fn revision(id: &str, expected_revision: i64) -> QuickNoteRevisionRequest {
    QuickNoteRevisionRequest {
        id: id.to_string(),
        expected_revision,
    }
}

fn list_request(collection: QuickNotesCollection, tag_id: Option<&str>) -> QuickNotesListRequest {
    QuickNotesListRequest {
        collection,
        query: None,
        tag_id: tag_id.map(str::to_string),
        cursor: None,
        page_size: Some(60),
    }
}

async fn listed(
    pool: &SqlitePool,
    collection: QuickNotesCollection,
    tag_id: Option<&str>,
) -> Vec<String> {
    list_window_from_pool(pool, list_request(collection, tag_id))
        .await
        .unwrap()
        .notes
        .into_iter()
        .map(|note| note.id)
        .collect()
}

async fn create_tag(
    pool: &SqlitePool,
    id: &str,
    name: &str,
) -> Result<QuickNoteTagRead, QuickNoteTagError> {
    tags::create_from_pool(
        pool,
        QuickNoteTagWrite {
            id: id.to_string(),
            name: name.to_string(),
        },
    )
    .await
}

fn error_code<T: Serialize>(error: T) -> String {
    serde_json::to_value(error).unwrap()["code"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn set_order_key(pool: &SqlitePool, id: &str, order_key: &str) {
    sqlx::query("UPDATE quick_notes SET order_key = ? WHERE id = ?")
        .bind(order_key)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

#[test]
fn normalizes_adjacent_runs_and_enforces_limits() {
    let merged = normalized_runs(&[
        QuickNoteTextRun {
            content: "one".into(),
            bold: true,
            italic: false,
            underline: false,
        },
        QuickNoteTextRun {
            content: String::new(),
            bold: false,
            italic: false,
            underline: false,
        },
        QuickNoteTextRun {
            content: " two".into(),
            bold: true,
            italic: false,
            underline: false,
        },
    ])
    .unwrap();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].content, "one two");
    assert!(validate_content("id", &"x".repeat(201), &[], 30).is_err());
    assert!(validate_content("id", "title", &runs("body"), 32).is_err());
}

#[test]
fn search_lifecycle_and_revision_conflicts_are_consistent() {
    block_on(async {
        let pool = pool().await;
        create(&pool, "one", "alpha body").await;
        create(&pool, "two", "beta body").await;

        let mut search = list_request(QuickNotesCollection::Active, None);
        search.query = Some("alpha".into());
        let active = list_window_from_pool(&pool, search).await.unwrap();
        assert_eq!(active.notes.len(), 1);
        assert_eq!(active.notes[0].id, "one");

        let archived = revision_mutation(
            &pool,
            &revision("one", 1),
            ARCHIVE_SQL,
            "archive",
            Placement::Unchanged,
        )
        .await
        .unwrap();
        assert!(archived.archived);
        let conflict = revision_mutation(
            &pool,
            &revision("one", 1),
            UNARCHIVE_SQL,
            "unarchive",
            Placement::FirstUnpinned,
        )
        .await
        .unwrap_err();
        assert_eq!(error_code(conflict), "revision_conflict");

        let trashed = revision_mutation(
            &pool,
            &revision("one", 2),
            TRASH_SQL,
            "trash",
            Placement::Unchanged,
        )
        .await
        .unwrap();
        assert!(trashed.trashed_at.is_some());
        let restored = revision_mutation(
            &pool,
            &revision("one", 3),
            RESTORE_SQL,
            "restore",
            Placement::FirstUnpinned,
        )
        .await
        .unwrap();
        assert!(restored.archived);
        assert!(restored.trashed_at.is_none());
    });
}

#[test]
fn list_cursor_preserves_pinned_then_manual_order() {
    block_on(async {
        let pool = pool().await;
        for id in ["a", "b", "c"] {
            create(&pool, id, id).await;
        }
        set_pinned_from_pool(
            &pool,
            QuickNotePinRequest {
                id: "b".into(),
                expected_revision: 1,
                pinned: true,
            },
        )
        .await
        .unwrap();
        let mut request = list_request(QuickNotesCollection::Active, None);
        request.page_size = Some(2);
        let first = list_window_from_pool(&pool, request).await.unwrap();
        assert_eq!(first.notes[0].id, "b");
        assert!(first.next_cursor.is_some());
        let mut request = list_request(QuickNotesCollection::Active, None);
        request.page_size = Some(2);
        request.cursor = first.next_cursor;
        let second = list_window_from_pool(&pool, request).await.unwrap();
        assert_eq!(second.notes.len(), 1);
        assert!(!first.notes.iter().any(|note| note.id == second.notes[0].id));
    });
}

#[test]
fn new_and_returning_notes_go_first_in_their_group() {
    block_on(async {
        let pool = pool().await;
        for id in ["a", "b", "c"] {
            create(&pool, id, id).await;
        }
        assert_eq!(
            listed(&pool, QuickNotesCollection::Active, None).await,
            ["c", "b", "a"]
        );

        let mut pinned = note("p", "pinned");
        pinned.pinned = true;
        create_from_pool(&pool, pinned).await.unwrap();
        assert_eq!(
            listed(&pool, QuickNotesCollection::Active, None).await,
            ["p", "c", "b", "a"]
        );

        set_pinned_from_pool(
            &pool,
            QuickNotePinRequest {
                id: "a".into(),
                expected_revision: 1,
                pinned: true,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            listed(&pool, QuickNotesCollection::Active, None).await,
            ["a", "p", "c", "b"]
        );
        set_pinned_from_pool(
            &pool,
            QuickNotePinRequest {
                id: "p".into(),
                expected_revision: 1,
                pinned: false,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            listed(&pool, QuickNotesCollection::Active, None).await,
            ["a", "p", "c", "b"]
        );

        revision_mutation(
            &pool,
            &revision("b", 1),
            ARCHIVE_SQL,
            "archive",
            Placement::Unchanged,
        )
        .await
        .unwrap();
        revision_mutation(
            &pool,
            &revision("b", 2),
            UNARCHIVE_SQL,
            "unarchive",
            Placement::FirstUnpinned,
        )
        .await
        .unwrap();
        assert_eq!(
            listed(&pool, QuickNotesCollection::Active, None).await,
            ["a", "b", "p", "c"]
        );

        revision_mutation(
            &pool,
            &revision("c", 1),
            TRASH_SQL,
            "trash",
            Placement::Unchanged,
        )
        .await
        .unwrap();
        revision_mutation(
            &pool,
            &revision("c", 2),
            RESTORE_SQL,
            "restore",
            Placement::FirstUnpinned,
        )
        .await
        .unwrap();
        assert_eq!(
            listed(&pool, QuickNotesCollection::Active, None).await,
            ["a", "c", "b", "p"]
        );
    });
}

#[test]
fn pinning_with_a_stale_revision_conflicts_without_moving() {
    block_on(async {
        let pool = pool().await;
        create(&pool, "a", "a").await;
        let error = set_pinned_from_pool(
            &pool,
            QuickNotePinRequest {
                id: "a".into(),
                expected_revision: 7,
                pinned: true,
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error_code(error), "revision_conflict");
        assert!(!load_note_from_pool(&pool, "a").await.unwrap().pinned);
    });
}

#[test]
fn reorder_persists_between_adjacent_visible_anchors() {
    block_on(async {
        let pool = pool().await;
        for id in ["c", "b", "hidden", "a"] {
            create(&pool, id, id).await;
        }
        create_tag(&pool, "tag-1", "Work").await.unwrap();
        sqlx::query("UPDATE quick_notes SET tag_id = 'tag-1' WHERE id IN ('a', 'b', 'c')")
            .execute(&pool)
            .await
            .unwrap();

        reorder_from_pool(
            &pool,
            QuickNoteReorderRequest {
                id: "c".into(),
                previous_id: Some("a".into()),
                next_id: Some("b".into()),
                tag_id: Some("tag-1".into()),
            },
        )
        .await
        .unwrap();

        assert_eq!(
            listed(&pool, QuickNotesCollection::Active, Some("tag-1")).await,
            ["a", "c", "b"]
        );
        assert_eq!(load_note_from_pool(&pool, "c").await.unwrap().revision, 1);
    });
}

#[test]
fn reorder_separates_rows_that_share_a_key() {
    block_on(async {
        let pool = pool().await;
        for id in ["d", "c", "b", "a", "n"] {
            create(&pool, id, id).await;
        }
        for id in ["a", "b", "c"] {
            set_order_key(&pool, id, "a0").await;
        }
        set_order_key(&pool, "d", "a1").await;
        set_order_key(&pool, "n", "a2").await;

        reorder_from_pool(
            &pool,
            QuickNoteReorderRequest {
                id: "n".into(),
                previous_id: Some("a".into()),
                next_id: Some("b".into()),
                tag_id: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            listed(&pool, QuickNotesCollection::Active, None).await,
            ["a", "n", "b", "c", "d"]
        );
    });
}

#[test]
fn reorder_rejects_nonadjacent_and_unknown_anchors() {
    block_on(async {
        let pool = pool().await;
        for id in ["d", "c", "b", "a"] {
            create(&pool, id, id).await;
        }
        let request = |previous: &str, next: &str| QuickNoteReorderRequest {
            id: "d".into(),
            previous_id: Some(previous.into()),
            next_id: Some(next.into()),
            tag_id: None,
        };
        assert!(reorder_from_pool(&pool, request("a", "c")).await.is_err());
        assert!(
            reorder_from_pool(&pool, request("a", "missing"))
                .await
                .is_err()
        );
        assert_eq!(
            listed(&pool, QuickNotesCollection::Active, None).await,
            ["a", "b", "c", "d"]
        );
    });
}

#[test]
fn expired_trash_is_hidden_then_purged_with_its_runs_and_search_projection() {
    block_on(async {
        let pool = pool().await;
        create(&pool, "expired", "old body").await;
        create(&pool, "recent", "new body").await;
        sqlx::query(
            "UPDATE quick_notes SET trashed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-8 days')
             WHERE id = 'expired'",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE quick_notes SET trashed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-1 days')
             WHERE id = 'recent'",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert!(load_note_from_pool(&pool, "expired").await.is_err());
        assert_eq!(
            listed(&pool, QuickNotesCollection::Trash, None).await,
            ["recent"]
        );

        let purge = purge_expired_trash(&pool).await.unwrap();
        assert_eq!(purge.purged, 1);
        let recent_trashed_at = load_note_from_pool(&pool, "recent")
            .await
            .unwrap()
            .trashed_at
            .unwrap();
        let expected: String =
            sqlx::query_scalar("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', ?, '+7 days')")
                .bind(&recent_trashed_at)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(purge.next_purge_at.as_deref(), Some(expected.as_str()));

        let runs: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM quick_note_text_runs WHERE note_id = 'expired'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let search: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM quick_notes_search_fts WHERE note_id = 'expired'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(runs, 0);
        assert_eq!(search, 0);

        sqlx::query("DELETE FROM quick_notes WHERE id = 'recent'")
            .execute(&pool)
            .await
            .unwrap();
        let empty = purge_expired_trash(&pool).await.unwrap();
        assert_eq!(empty.purged, 0);
        assert!(empty.next_purge_at.is_none());
    });
}

#[test]
fn tag_filtering_returns_only_matching_notes() {
    block_on(async {
        let pool = pool().await;
        create_tag(&pool, "tag-1", "Work").await.unwrap();
        let mut tagged = note("tagged", "tagged body");
        tagged.tag_id = Some("tag-1".into());
        create_from_pool(&pool, tagged).await.unwrap();
        create(&pool, "plain", "plain body").await;

        assert_eq!(
            listed(&pool, QuickNotesCollection::Active, Some("tag-1")).await,
            ["tagged"]
        );
    });
}

#[test]
fn tags_append_in_order_and_enforce_the_local_create_rules() {
    block_on(async {
        let pool = pool().await;
        for index in 0..9 {
            create_tag(&pool, &format!("tag-{index}"), &format!("Tag {index}"))
                .await
                .unwrap();
        }
        let tags = tags::list_from_pool(&pool).await.unwrap();
        assert_eq!(
            tags.iter().map(|tag| tag.id.as_str()).collect::<Vec<_>>(),
            (0..9)
                .map(|index| format!("tag-{index}"))
                .collect::<Vec<_>>()
        );
        assert!(
            tags.windows(2)
                .all(|pair| pair[0].order_key < pair[1].order_key)
        );

        let limit = create_tag(&pool, "tag-9", "Tag 9").await.unwrap_err();
        assert_eq!(error_code(limit), "limit_reached");

        tags::delete_from_pool(&pool, "tag-8").await.unwrap();
        let duplicate = create_tag(&pool, "tag-9", " tag 0 ").await.unwrap_err();
        assert_eq!(error_code(duplicate), "duplicate_name");
        let invalid = create_tag(&pool, "tag-9", "   ").await.unwrap_err();
        assert_eq!(error_code(invalid), "failed");
    });
}

#[test]
fn renaming_a_tag_checks_names_and_existence() {
    block_on(async {
        let pool = pool().await;
        create_tag(&pool, "tag-1", "Work").await.unwrap();
        create_tag(&pool, "tag-2", "Home").await.unwrap();
        let rename = |id: &str, name: &str| QuickNoteTagWrite {
            id: id.to_string(),
            name: name.to_string(),
        };

        let renamed = tags::rename_from_pool(&pool, rename("tag-1", " Office "))
            .await
            .unwrap();
        assert_eq!(renamed.name, "Office");
        let recased = tags::rename_from_pool(&pool, rename("tag-1", "OFFICE"))
            .await
            .unwrap();
        assert_eq!(recased.name, "OFFICE");

        let duplicate = tags::rename_from_pool(&pool, rename("tag-2", "office"))
            .await
            .unwrap_err();
        assert_eq!(error_code(duplicate), "duplicate_name");
        let missing = tags::rename_from_pool(&pool, rename("tag-3", "Garden"))
            .await
            .unwrap_err();
        assert_eq!(error_code(missing), "not_found");
    });
}

#[test]
fn deleting_a_tag_untags_its_notes_with_a_new_revision() {
    block_on(async {
        let pool = pool().await;
        create_tag(&pool, "tag-1", "Work").await.unwrap();
        let mut tagged = note("tagged", "body");
        tagged.tag_id = Some("tag-1".into());
        create_from_pool(&pool, tagged).await.unwrap();
        create(&pool, "plain", "body").await;

        tags::delete_from_pool(&pool, "tag-1").await.unwrap();
        tags::delete_from_pool(&pool, "tag-1").await.unwrap();

        let untagged = load_note_from_pool(&pool, "tagged").await.unwrap();
        assert!(untagged.tag_id.is_none());
        assert_eq!(untagged.revision, 2);
        assert_eq!(
            load_note_from_pool(&pool, "plain").await.unwrap().revision,
            1
        );
        assert!(tags::list_from_pool(&pool).await.unwrap().is_empty());

        let stale = QuickNoteUpdate {
            id: "tagged".into(),
            expected_revision: 1,
            title: "tagged".into(),
            runs: runs("edited"),
            color: 0,
            tag_id: Some("tag-1".into()),
            pinned: false,
        };
        assert_eq!(
            error_code(update_from_pool(&pool, stale).await.unwrap_err()),
            "revision_conflict"
        );
    });
}

#[test]
fn writes_that_name_a_deleted_tag_leave_the_note_untagged() {
    block_on(async {
        let pool = pool().await;
        let mut created = note("note", "body");
        created.tag_id = Some("gone".into());
        assert!(
            create_from_pool(&pool, created)
                .await
                .unwrap()
                .tag_id
                .is_none()
        );

        let update = QuickNoteUpdate {
            id: "note".into(),
            expected_revision: 1,
            title: "note".into(),
            runs: runs("edited"),
            color: 0,
            tag_id: Some("gone".into()),
            pinned: false,
        };
        let updated = update_from_pool(&pool, update).await.unwrap();
        assert!(updated.tag_id.is_none());
        assert_eq!(updated.revision, 2);
    });
}
