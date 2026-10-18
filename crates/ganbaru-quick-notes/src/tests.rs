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
        images: Vec::new(),
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
            color: 4,
        },
    )
    .await
}

async fn tag_ids(pool: &SqlitePool) -> Vec<String> {
    tags::list_from_pool(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|tag| tag.id)
        .collect()
}

async fn reorder_tag(
    pool: &SqlitePool,
    id: &str,
    previous_id: Option<&str>,
    next_id: Option<&str>,
) -> Result<(), QuickNoteTagError> {
    tags::reorder_from_pool(
        pool,
        QuickNoteTagReorder {
            id: id.to_string(),
            previous_id: previous_id.map(str::to_string),
            next_id: next_id.map(str::to_string),
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
fn tags_keep_a_palette_color() {
    block_on(async {
        let pool = pool().await;
        let created = create_tag(&pool, "tag-1", "Work").await.unwrap();
        assert_eq!(created.color, 4);
        for color in [-1, 32] {
            let invalid = tags::create_from_pool(
                &pool,
                QuickNoteTagWrite {
                    id: "tag-2".to_string(),
                    name: "Home".to_string(),
                    color,
                },
            )
            .await
            .unwrap_err();
            assert_eq!(error_code(invalid), "failed");
        }
        assert_eq!(tag_ids(&pool).await, ["tag-1"]);
    });
}

#[test]
fn updating_a_tag_checks_names_colors_and_existence() {
    block_on(async {
        let pool = pool().await;
        create_tag(&pool, "tag-1", "Work").await.unwrap();
        create_tag(&pool, "tag-2", "Home").await.unwrap();
        let mut tagged = note("tagged", "body");
        tagged.tag_id = Some("tag-1".into());
        tagged.color = 4;
        create_from_pool(&pool, tagged).await.unwrap();
        let update = |id: &str, name: &str, color: i64| QuickNoteTagWrite {
            id: id.to_string(),
            name: name.to_string(),
            color,
        };

        let renamed = tags::update_from_pool(&pool, update("tag-1", " Office ", 9))
            .await
            .unwrap();
        assert_eq!((renamed.name.as_str(), renamed.color), ("Office", 9));
        let recased = tags::update_from_pool(&pool, update("tag-1", "OFFICE", 9))
            .await
            .unwrap();
        assert_eq!(recased.name, "OFFICE");
        // Recoloring a tag leaves the colors of its notes alone.
        assert_eq!(load_note_from_pool(&pool, "tagged").await.unwrap().color, 4);

        let duplicate = tags::update_from_pool(&pool, update("tag-2", "office", 4))
            .await
            .unwrap_err();
        assert_eq!(error_code(duplicate), "duplicate_name");
        let color = tags::update_from_pool(&pool, update("tag-2", "Home", 32))
            .await
            .unwrap_err();
        assert_eq!(error_code(color), "failed");
        let missing = tags::update_from_pool(&pool, update("tag-3", "Garden", 4))
            .await
            .unwrap_err();
        assert_eq!(error_code(missing), "not_found");
    });
}

#[test]
fn reordering_tags_moves_between_adjacent_neighbors() {
    block_on(async {
        let pool = pool().await;
        for id in ["a", "b", "c", "d"] {
            create_tag(&pool, id, &id.to_uppercase()).await.unwrap();
        }

        reorder_tag(&pool, "d", None, Some("a")).await.unwrap();
        assert_eq!(tag_ids(&pool).await, ["d", "a", "b", "c"]);
        reorder_tag(&pool, "d", Some("c"), None).await.unwrap();
        assert_eq!(tag_ids(&pool).await, ["a", "b", "c", "d"]);
        reorder_tag(&pool, "a", Some("b"), Some("c")).await.unwrap();
        assert_eq!(tag_ids(&pool).await, ["b", "a", "c", "d"]);

        let gap = reorder_tag(&pool, "a", Some("b"), Some("d"))
            .await
            .unwrap_err();
        assert_eq!(error_code(gap), "failed");
        let itself = reorder_tag(&pool, "a", Some("a"), None).await.unwrap_err();
        assert_eq!(error_code(itself), "failed");
        let unknown = reorder_tag(&pool, "a", Some("gone"), None)
            .await
            .unwrap_err();
        assert_eq!(error_code(unknown), "failed");
        let missing = reorder_tag(&pool, "gone", None, Some("a"))
            .await
            .unwrap_err();
        assert_eq!(error_code(missing), "not_found");
        assert_eq!(tag_ids(&pool).await, ["b", "a", "c", "d"]);
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
            images: Vec::new(),
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
            images: Vec::new(),
            color: 0,
            tag_id: Some("gone".into()),
            pinned: false,
        };
        let updated = update_from_pool(&pool, update).await.unwrap();
        assert!(updated.tag_id.is_none());
        assert_eq!(updated.revision, 2);
    });
}

/// Header-only PNG bytes, enough for metadata parsing; `seed` varies the digest.
fn png(width: u32, height: u32, seed: u8) -> Vec<u8> {
    let mut bytes = vec![0; 34];
    bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    bytes[8..12].copy_from_slice(&13u32.to_be_bytes());
    bytes[12..16].copy_from_slice(b"IHDR");
    bytes[16..20].copy_from_slice(&width.to_be_bytes());
    bytes[20..24].copy_from_slice(&height.to_be_bytes());
    bytes[24..29].copy_from_slice(&[8, 6, 0, 0, 0]);
    bytes[33] = seed;
    bytes
}

/// A fresh, empty vault directory for file tests.
fn vault_dir(name: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("ganbaru-quick-notes-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn image(seed: u8) -> QuickNoteImage {
    images::describe_image(&png(40 + u32::from(seed), 30, seed)).unwrap()
}

fn file_of(image: &QuickNoteImage) -> QuickNoteImageFile {
    let value = serde_json::to_value(image).unwrap();
    QuickNoteImageFile::new(
        value["sha256"].as_str().unwrap().to_string(),
        value["contentType"].as_str().unwrap().to_string(),
    )
}

#[test]
fn images_keep_their_order_through_create_update_and_list() {
    block_on(async {
        let pool = pool().await;
        let mut created = note("note", "body");
        created.images = vec![image(1), image(2)];
        let read = create_from_pool(&pool, created).await.unwrap();
        assert_eq!(read.images, [image(1), image(2)]);

        let update = QuickNoteUpdate {
            id: "note".into(),
            expected_revision: 1,
            title: "note".into(),
            runs: runs("body"),
            images: vec![image(3), image(1)],
            color: 0,
            tag_id: None,
            pinned: false,
        };
        let updated = update_from_pool(&pool, update).await.unwrap();
        assert_eq!(updated.images, [image(3), image(1)]);
        assert_eq!(updated.revision, 2);
        let listed = list_window_from_pool(&pool, list_request(QuickNotesCollection::Active, None))
            .await
            .unwrap();
        assert_eq!(listed.notes[0].images, [image(3), image(1)]);
        let json = serde_json::to_value(&listed.notes[0].images[0]).unwrap();
        assert_eq!(json["contentType"], "image/png");
        assert_eq!(json["byteSize"], 34);
        assert_eq!(
            (json["width"].as_u64(), json["height"].as_u64()),
            (Some(43), Some(30))
        );
    });
}

#[test]
fn image_lists_reject_duplicates_and_excess() {
    block_on(async {
        let pool = pool().await;
        let mut duplicate = note("duplicate", "body");
        duplicate.images = vec![image(1), image(1)];
        assert!(create_from_pool(&pool, duplicate).await.is_err());

        let mut excess = note("excess", "body");
        excess.images = (0..=24).map(image).collect();
        assert!(create_from_pool(&pool, excess).await.is_err());

        let mut tampered = note("tampered", "body");
        let mut value = serde_json::to_value(image(1)).unwrap();
        value["sha256"] = "Z".repeat(64).into();
        tampered.images = vec![serde_json::from_value(value).unwrap()];
        assert!(create_from_pool(&pool, tampered).await.is_err());
    });
}

#[test]
fn imported_images_are_content_addressed_and_verified_on_read() {
    let vault = vault_dir("import");
    let bytes = png(640, 480, 7);
    let imported = images::import_image(&vault, &bytes).unwrap();
    assert_eq!(images::import_image(&vault, &bytes).unwrap(), imported);
    let file = file_of(&imported);
    let path = images::images_dir(&vault).join(file.file_name().unwrap());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(images::read_image(&vault, &file).unwrap(), bytes);
    assert_eq!(
        std::fs::read_dir(images::images_dir(&vault))
            .unwrap()
            .count(),
        1
    );

    std::fs::write(&path, png(640, 480, 8)).unwrap();
    assert_eq!(
        images::read_image(&vault, &file),
        Err(QuickNoteImageError::Unavailable)
    );
    // Importing the same bytes repairs a file whose contents no longer match its name, whether
    // or not its length still matches.
    images::import_image(&vault, &bytes).unwrap();
    assert_eq!(images::read_image(&vault, &file).unwrap(), bytes);
    std::fs::write(&path, b"short").unwrap();
    images::import_image(&vault, &bytes).unwrap();
    assert_eq!(images::read_image(&vault, &file).unwrap(), bytes);

    let missing = file_of(&image(9));
    assert_eq!(
        images::read_image(&vault, &missing),
        Err(QuickNoteImageError::Unavailable)
    );
    assert_eq!(
        images::import_image(&vault, b"not an image"),
        Err(QuickNoteImageError::Unsupported)
    );
    assert_eq!(
        images::import_image(&vault, &png(20_000, 10, 1)),
        Err(QuickNoteImageError::TooLarge)
    );
    assert_eq!(
        images::import_image(&vault, &png(10_000, 10_000, 1)),
        Err(QuickNoteImageError::TooLarge)
    );
    assert!(
        QuickNoteImageFile::new("../escape".into(), "image/png".into())
            .file_name()
            .is_err()
    );
    std::fs::remove_dir_all(&vault).unwrap();
}

#[test]
fn cleanup_keeps_referenced_retained_and_recent_files() {
    block_on(async {
        let pool = pool().await;
        let vault = vault_dir("cleanup");
        let [live, retained, orphan] = [1, 2, 3].map(|seed| {
            images::import_image(&vault, &png(40 + u32::from(seed), 30, seed)).unwrap()
        });
        let mut created = note("note", "body");
        created.images = vec![live.clone()];
        create_from_pool(&pool, created).await.unwrap();

        // A version that an open conflict or recovery offer still holds.
        let encoded = ganbaru_sync::domains::quick_notes::encode_images(&[
            ganbaru_sync::domains::quick_notes::NoteImage::from(&retained),
        ])
        .unwrap();
        let value = Value::new(vec![Field::Blob(encoded)]).unwrap();
        sqlx::query(
            "INSERT INTO sync_rows (table_id, row_key, state) VALUES (2, 'gone', 'tombstoned')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sync_register_versions
                 (table_id, row_key, group_id, writer_id, seq, clock, value_hash, value)
             VALUES (2, 'gone', ?, zeroblob(16), 1, 0, zeroblob(32), ?)",
        )
        .bind(i64::from(note_group::IMAGES.get()))
        .bind(value.encode())
        .execute(&pool)
        .await
        .unwrap();
        let dir = images::images_dir(&vault);
        std::fs::write(dir.join(".stale.png.0000.tmp"), b"partial").unwrap();

        let now = std::time::SystemTime::now();
        assert_eq!(
            images::collect_unreferenced_images_before(&pool, &vault, now)
                .await
                .unwrap(),
            0
        );
        let later = now + std::time::Duration::from_secs(2 * 24 * 60 * 60);
        assert_eq!(
            images::collect_unreferenced_images_before(&pool, &vault, later)
                .await
                .unwrap(),
            2
        );
        for kept in [&live, &retained] {
            assert!(images::read_image(&vault, &file_of(kept)).is_ok());
        }
        assert_eq!(
            images::read_image(&vault, &file_of(&orphan)),
            Err(QuickNoteImageError::Unavailable)
        );

        trash_from_pool(&pool, &revision("note", 1)).await.unwrap();
        delete_permanently_from_pool(&pool, "note").await.unwrap();
        assert_eq!(
            images::collect_unreferenced_images_before(&pool, &vault, later)
                .await
                .unwrap(),
            1
        );
        std::fs::remove_dir_all(&vault).unwrap();
    });
}

#[test]
fn restoring_a_recovered_note_keeps_its_images() {
    block_on(async {
        let pool = pool().await;
        let encoded = ganbaru_sync::domains::quick_notes::encode_images(&[
            ganbaru_sync::domains::quick_notes::NoteImage::from(&image(4)),
            ganbaru_sync::domains::quick_notes::NoteImage::from(&image(5)),
        ])
        .unwrap();
        let values = [(
            note_group::IMAGES,
            Value::new(vec![Field::Blob(encoded)]).unwrap(),
        )];
        let id = restore_recovered(&pool, &values).await.unwrap();
        let restored = load_note_from_pool(&pool, &id).await.unwrap();
        assert_eq!(restored.images, [image(4), image(5)]);
    });
}
