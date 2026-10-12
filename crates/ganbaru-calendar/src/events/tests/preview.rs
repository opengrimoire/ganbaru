use crate::events::{commit, edit, preview, scope};
use crate::reads::native_window::{self, WindowPurpose};
use crate::recurrence::canonical::{EditScope, ScopeClock, Window, parse_date};
use serde_json::{Value, json};

fn clock() -> ScopeClock {
    ScopeClock {
        epoch_ms: chrono::DateTime::parse_from_rfc3339("2099-05-08T00:00:00Z")
            .unwrap()
            .timestamp_millis(),
        floating_today: Some(parse_date("2099-05-08").unwrap()),
    }
}

fn window() -> Window {
    Window::new(
        "2099-05-08",
        "2099-05-20",
        &ganbaru_civil_time::zone("UTC").unwrap(),
    )
    .unwrap()
}

#[test]
fn deletion_preview_matches_committed_retained_and_removed_families_with_moved_identities() {
    crate::test_support::block_on(async {
        for standalone in [false, true] {
            for started in [false, true] {
                for selected_scope in [EditScope::This, EditScope::Following, EditScope::All] {
                    let pool = super::metadata::seed().await;
                    if standalone {
                        sqlx::query("UPDATE calendar_events SET rrule=NULL WHERE id='source'")
                            .execute(&pool)
                            .await
                            .unwrap();
                    } else {
                        sqlx::query("UPDATE calendar_event_overrides SET start_time='2099-05-12T16:00:00Z', end_time='2099-05-12T17:00:00Z' WHERE id='override'")
                            .execute(&pool).await.unwrap();
                    }
                    let selected = parse_date(if standalone {
                        "2099-05-09"
                    } else {
                        "2099-05-10"
                    })
                    .unwrap();
                    let mut now = clock();
                    if started {
                        now.epoch_ms = chrono::DateTime::parse_from_rfc3339("2099-05-13T00:00:00Z")
                            .unwrap()
                            .timestamp_millis();
                    }
                    let mut tx = pool.begin().await.unwrap();
                    let preview = preview::project_delete(
                        scope::read_snapshot(&mut tx, "source").await.unwrap(),
                        selected,
                        selected_scope,
                        now,
                        "delete-preview",
                        &window(),
                    )
                    .unwrap();
                    assert!(preview.window.events.iter().all(|row| row.id == "source"));
                    assert!(
                        preview
                            .window
                            .occurrences
                            .iter()
                            .all(|row| row.template_id == "source")
                    );
                    let before: i64 =
                        sqlx::query_scalar("SELECT COUNT(*) FROM calendar_event_archives")
                            .fetch_one(&mut *tx)
                            .await
                            .unwrap();
                    assert_eq!(before, 0);
                    let prepared = crate::events::deletion::prepare_commit(
                        scope::read_snapshot(&mut tx, "source").await.unwrap(),
                        selected,
                        selected_scope,
                        now,
                        "delete-preview",
                        &preview.review_revision,
                        false,
                    )
                    .unwrap();
                    prepared.write(&mut tx).await.unwrap();
                    tx.commit().await.unwrap();
                    let mut tx = pool.begin().await.unwrap();
                    let mut actual =
                        native_window::read(&mut tx, &window(), WindowPurpose::Render, false)
                            .await
                            .unwrap()
                            .expand(&window())
                            .unwrap();
                    actual.events.retain(|row| row.id == "source");
                    actual
                        .overrides
                        .retain(|row| row.parent_event_id == "source");
                    actual.attendees.retain(|row| row.event_id == "source");
                    actual.occurrences.retain(|row| row.template_id == "source");
                    assert_eq!(
                        serde_json::to_value(actual).unwrap(),
                        serde_json::to_value(&preview.window).unwrap(),
                        "standalone={standalone}, started={started}, scope={selected_scope:?}"
                    );
                    if standalone {
                        assert!(preview.window.events.is_empty());
                        assert!(preview.window.occurrences.is_empty());
                    }
                    tx.commit().await.unwrap();
                }
            }
        }
    });
}

/// Compare every slim field and occurrence to the persisted native read, while
/// retaining unrelated events in the database to prove the preview's scope.
async fn assert_preview_save(
    pool: &sqlx::SqlitePool,
    draft: Value,
    scope: EditScope,
) -> preview::EditPreview {
    let now = clock();
    let window = window();
    let selected = parse_date("2099-05-10").unwrap();
    let mut tx = pool.begin().await.unwrap();
    let prepared = edit::prepare(
        scope::read_snapshot(&mut tx, "source").await.unwrap(),
        selected,
        scope,
        now,
        serde_json::from_value(draft.clone()).unwrap(),
    )
    .unwrap();
    tx.commit().await.unwrap();
    let preview = preview::project(prepared, "preview-save", &window, now.epoch_ms).unwrap();
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_events")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(before, 2, "preview must not stage writes");
    assert!(
        !preview
            .window
            .events
            .iter()
            .any(|event| event.id == "target")
    );
    assert!(
        preview.previewed_ids.iter().all(|id| preview
            .window
            .occurrences
            .iter()
            .any(|row| &row.id == id))
    );
    assert!(
        preview.editing_id.as_ref().is_none_or(|id| preview
            .window
            .occurrences
            .iter()
            .any(|row| &row.id == id))
    );
    let mut tx = pool.begin().await.unwrap();
    let prepared = commit::prepare_rows(
        scope::read_snapshot(&mut tx, "source").await.unwrap(),
        selected,
        scope,
        now,
        serde_json::from_value(draft).unwrap(),
        "preview-save",
        &preview.review_revision,
    )
    .unwrap();
    let receipt = prepared.write(&mut tx).await.unwrap();
    assert_eq!(receipt.edited_id, preview.edited_id);
    tx.commit().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    let mut actual = native_window::read(&mut tx, &window, WindowPurpose::Render, false)
        .await
        .unwrap()
        .expand(&window)
        .unwrap();
    tx.commit().await.unwrap();
    actual.events.retain(|row| row.id != "target");
    actual
        .overrides
        .retain(|row| row.parent_event_id != "target");
    actual.attendees.retain(|row| row.event_id != "target");
    actual.occurrences.retain(|row| row.template_id != "target");
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(&preview.window).unwrap()
    );
    preview
}

#[test]
fn preview_matches_persisted_detach_split_update_and_noop_including_children() {
    crate::test_support::block_on(async {
        for (scope, draft) in [
            (
                EditScope::This,
                json!({"fields":[{"field":"title","value":"Detached"}]}),
            ),
            (
                EditScope::Following,
                json!({"fields":[{"field":"title","value":"Following"}]}),
            ),
            (
                EditScope::All,
                json!({"fields":[{"field":"title","value":"Entire"}]}),
            ),
            (EditScope::This, json!({})),
        ] {
            let pool = super::metadata::seed().await;
            // Explicit sort order differs from lexical identity order.
            sqlx::query("INSERT INTO calendar_event_notifications(id, event_id, offset_minutes, sort_order) VALUES ('aaa-notice', 'source', 15, 20)")
                .execute(&pool).await.unwrap();
            let preview = assert_preview_save(&pool, draft, scope).await;
            assert!(preview.editing_id.is_some());
        }
    });
}

#[test]
fn preview_preserves_hidden_history_and_uses_materialized_selected_identity() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        super::fixtures::insert_test_completed_pomodoro_history(
            &pool,
            "source",
            "source::2099-05-10",
            "2099-05-09",
        )
        .await;
        let preview = assert_preview_save(
            &pool,
            json!({"fields":[{"field":"title","value":"Mutable future"}]}),
            EditScope::All,
        )
        .await;
        assert!(preview.scope.selected_has_history);
        assert_ne!(preview.editing_id, Some(preview.edited_id.clone()));
        assert!(
            preview
                .window
                .occurrences
                .iter()
                .any(|row| Some(&row.id) == preview.editing_id.as_ref())
        );
    });
}

#[test]
fn preview_outside_window_has_no_invisible_contour_or_panel_identity() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let mut tx = pool.begin().await.unwrap();
        let prepared = edit::prepare(
            scope::read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::This,
            clock(),
            serde_json::from_value(json!({"fields":[{"field":"title","value":"Detached"}]}))
                .unwrap(),
        )
        .unwrap();
        tx.commit().await.unwrap();
        let window = Window::new(
            "2099-06-01",
            "2099-06-07",
            &ganbaru_civil_time::zone("UTC").unwrap(),
        )
        .unwrap();
        let preview =
            preview::project(prepared, "preview-outside", &window, clock().epoch_ms).unwrap();
        assert!(preview.window.occurrences.is_empty());
        assert!(preview.previewed_ids.is_empty());
        assert!(preview.editing_id.is_none());
    });
}

#[test]
fn scope_changes_without_edits_highlight_only_the_native_mutable_occurrences() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        for (scope, expected) in [
            (EditScope::This, 1),
            (EditScope::Following, 2),
            (EditScope::All, 3),
        ] {
            let mut tx = pool.begin().await.unwrap();
            let prepared = edit::prepare(
                scope::read_snapshot(&mut tx, "source").await.unwrap(),
                parse_date("2099-05-10").unwrap(),
                scope,
                clock(),
                serde_json::from_value(json!({})).unwrap(),
            )
            .unwrap();
            tx.commit().await.unwrap();
            let result =
                preview::project(prepared, "scope-review", &window(), clock().epoch_ms).unwrap();
            assert!(!result.changed);
            assert_eq!(result.previewed_ids.len(), expected);
            assert_eq!(result.editing_id.as_deref(), Some("source::2099-05-10"));
        }
    });
}

#[test]
fn preview_keeps_the_selected_override_when_another_occurrence_has_identical_geometry() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        sqlx::query("UPDATE calendar_event_overrides SET recurrence_id = '2099-05-10', start_time = '2099-05-09T10:00:00Z', end_time = '2099-05-09T11:00:00Z' WHERE parent_event_id = 'source'")
            .execute(&pool).await.unwrap();
        let result = assert_preview_save(
            &pool,
            json!({"fields":[{"field":"title","value":"Edited master"}]}),
            EditScope::All,
        )
        .await;
        assert_eq!(result.editing_id.as_deref(), Some("source::2099-05-10"));
        assert_eq!(
            result
                .window
                .occurrences
                .iter()
                .filter(|row| row.start_time == "2099-05-09T10:00:00.000Z")
                .count(),
            2
        );
    });
}

#[test]
fn ongoing_untracked_selection_follows_its_detached_identity() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let now = ScopeClock {
            epoch_ms: chrono::DateTime::parse_from_rfc3339("2099-05-10T10:05:00Z")
                .unwrap()
                .timestamp_millis(),
            floating_today: None,
        };
        let mut tx = pool.begin().await.unwrap();
        let prepared = edit::prepare(
            scope::read_snapshot(&mut tx, "source").await.unwrap(),
            parse_date("2099-05-10").unwrap(),
            EditScope::This,
            now,
            serde_json::from_value(json!({"fields":[{"field":"title","value":"Ongoing"}]}))
                .unwrap(),
        )
        .unwrap();
        tx.commit().await.unwrap();
        let result = preview::project(prepared, "ongoing", &window(), now.epoch_ms).unwrap();
        assert_eq!(result.editing_id.as_ref(), Some(&result.edited_id));
        assert_ne!(result.edited_id, "source");
    });
}

#[test]
fn floating_preview_and_save_share_date_geometry_and_focus_removal() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let result = assert_preview_save(&pool, json!({"timing":{
            "allDay":true,"startTime":"2099-05-10","endTime":"2099-05-12","inputZone":"Asia/Tokyo"
        }}), EditScope::This).await;
        let edited = result
            .window
            .events
            .iter()
            .find(|row| row.id == result.edited_id)
            .unwrap();
        assert_eq!(edited.all_day, 1);
        assert!(edited.rhythm_kind.is_none());
        let selected = result
            .window
            .occurrences
            .iter()
            .find(|row| Some(&row.id) == result.editing_id.as_ref())
            .unwrap();
        assert_eq!(selected.start_time, "2099-05-10T00:00:00.000Z");
        assert_eq!(selected.end_time, "2099-05-12T00:00:00.000Z");
    });
}
