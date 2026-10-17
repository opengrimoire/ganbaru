use super::fixtures::*;
use crate::events::archive::{is_protected_event, load_mutation_context};

fn target(id: &str) -> CalendarEventMutationTarget {
    CalendarEventMutationTarget { id: id.into() }
}

#[test]
fn canonical_protection_prevents_deleting_a_past_occurrence_or_root() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2000-05-09T10:00:00Z",
            "2000-05-09T11:00:00Z",
            Some("FREQ=DAILY"),
        )
        .await;
        for id in ["event-1", "event-1::2000-05-10"] {
            let mut tx = pool.begin().await.unwrap();
            let error = delete_calendar_event_tx(&mut tx, &target(id))
                .await
                .unwrap_err();
            assert!(error.contains("archive it instead"), "{error}");
            tx.rollback().await.unwrap();
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        let exceptions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_event_exdates")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!((count, exceptions), (1, 0));
    });
}

#[test]
fn moved_override_uses_original_identity_and_canonical_archive_geometry() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2099-05-09T10:00:00Z",
            "2099-05-09T11:00:00Z",
            Some("FREQ=DAILY;COUNT=2"),
        )
        .await;
        sqlx::query("INSERT INTO calendar_event_overrides
            (id, parent_event_id, recurrence_id, start_time, end_time)
            VALUES ('moved', 'event-1', '2099-05-10', '2000-05-12T12:00:00Z', '2000-05-12T13:00:00Z')")
            .execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        let context = load_mutation_context(&mut tx, &target("event-1::2099-05-10"))
            .await
            .unwrap();
        assert_eq!(context.occurrence_date.as_deref(), Some("2099-05-10"));
        assert_eq!(context.start_time, "2000-05-12T12:00:00.000Z");
        assert!(
            is_protected_event(&mut tx, &context, "2026-10-03T00:00:00Z")
                .await
                .unwrap()
        );
        archive_calendar_event_tx(&mut tx, &target("event-1::2099-05-10"))
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let archived: (String, String) = sqlx::query_as(
            "SELECT start_time, end_time FROM calendar_event_archives WHERE id = 'event-1::2099-05-10'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(
            archived,
            (
                "2000-05-12T12:00:00.000Z".into(),
                "2000-05-12T13:00:00.000Z".into()
            )
        );
    });
}

#[test]
fn excluded_cancelled_and_count_exhausted_identities_cannot_be_mutated() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2099-05-09T10:00:00Z",
            "2099-05-09T11:00:00Z",
            Some("FREQ=DAILY;COUNT=3"),
        )
        .await;
        sqlx::query(
            "INSERT INTO calendar_event_exdates (id, event_id, occurrence_date)
            VALUES ('excluded', 'event-1', '2099-05-10')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO calendar_event_overrides (id, parent_event_id, recurrence_id, status)
            VALUES ('cancelled', 'event-1', '2099-05-11', 'cancelled')",
        )
        .execute(&pool)
        .await
        .unwrap();
        for date in ["2099-05-08", "2099-05-10", "2099-05-11", "2099-05-12"] {
            let mut tx = pool.begin().await.unwrap();
            let result = load_mutation_context(&mut tx, &target(&format!("event-1::{date}"))).await;
            assert!(result.err().unwrap().contains("no longer exists"));
            tx.rollback().await.unwrap();
        }
    });
}

#[test]
fn rdate_outside_count_is_resolved_without_frontend_timestamps() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2099-05-09T02:00:00Z",
            "2099-05-09T03:00:00Z",
            Some("FREQ=DAILY;COUNT=1"),
        )
        .await;
        sqlx::query(
            "INSERT INTO calendar_event_rdates (id, event_id, occurrence_start)
            VALUES ('extra', 'event-1', '2099-05-20')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let context = load_mutation_context(
            &mut tx,
            &CalendarEventMutationTarget {
                id: "event-1::2099-05-20".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(context.start_time, "2099-05-21T02:00:00.000Z");
        assert_eq!(context.occurrence_date.as_deref(), Some("2099-05-20"));
        tx.rollback().await.unwrap();
    });
}

#[test]
fn canonical_protection_failure_rolls_back_an_earlier_deletion() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "future",
            "2099-05-09T10:00:00Z",
            "2099-05-09T11:00:00Z",
            None,
        )
        .await;
        insert_test_event_at(
            &pool,
            "past",
            "2000-05-09T10:00:00Z",
            "2000-05-09T11:00:00Z",
            Some("FREQ=DAILY"),
        )
        .await;
        let mut tx = pool.begin().await.unwrap();
        delete_calendar_event_tx(&mut tx, &target("future"))
            .await
            .unwrap();
        let error = delete_calendar_event_tx(&mut tx, &target("past::2000-05-10"))
            .await
            .unwrap_err();
        assert!(error.contains("archive it instead"));
        tx.rollback().await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendar_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 2);
    });
}

#[test]
fn oversized_recurrence_source_fails_before_loading_its_text() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2099-05-09T10:00:00Z",
            "2099-05-09T11:00:00Z",
            Some("FREQ=DAILY"),
        )
        .await;
        sqlx::query(
            "INSERT INTO calendar_event_rdates (id, event_id, occurrence_start)
            VALUES ('huge', 'event-1', hex(zeroblob(4194304)))",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let result = load_mutation_context(&mut tx, &target("event-1::2099-05-10")).await;
        assert!(result.err().unwrap().contains("record or byte budget"));
        tx.rollback().await.unwrap();
    });
}

#[test]
fn offset_timestamps_are_compared_as_instants_at_the_protection_boundary() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event_at(
            &pool,
            "event-1",
            "2026-10-03T01:00:00+09:00",
            "2026-10-03T02:00:00+09:00",
            None,
        )
        .await;
        let mut tx = pool.begin().await.unwrap();
        let context = load_mutation_context(&mut tx, &target("event-1"))
            .await
            .unwrap();
        assert!(
            is_protected_event(&mut tx, &context, "2026-10-02T17:00:00Z")
                .await
                .unwrap()
        );
        assert!(
            !is_protected_event(&mut tx, &context, "2026-10-02T15:59:59.999Z")
                .await
                .unwrap()
        );
        assert!(
            is_protected_event(&mut tx, &context, "2026-10-02T16:00:00Z")
                .await
                .unwrap()
        );
        tx.rollback().await.unwrap();
    });
}
