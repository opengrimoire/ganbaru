use super::fixtures::*;

#[test]
fn update_description_field_sanitizes_before_persistence() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event(&pool, "event-1", "").await;
        let mut tx = pool.begin().await.unwrap();

        apply_update_field(
            &mut tx,
            "event-1",
            &CalendarEventUpdateField::Description(
                "<p onclick=\"alert(1)\">Safe <a href=\"javascript:alert(1)\">bad</a></p>"
                    .to_string(),
            ),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let description: String =
            sqlx::query_scalar("SELECT description FROM calendar_events WHERE id = 'event-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(description, "<p>Safe <a>bad</a></p>");
    });
}

#[test]
fn copied_event_description_is_sanitized_after_split_or_detach_insert() {
    crate::test_support::block_on(async {
        let pool = in_memory_pool().await;
        insert_test_event(
            &pool,
            "event-1",
            "<div><img src=\"x\"><strong>Safe</strong></div>",
        )
        .await;
        let mut tx = pool.begin().await.unwrap();

        sanitize_stored_event_description(&mut tx, "event-1")
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let description: String =
            sqlx::query_scalar("SELECT description FROM calendar_events WHERE id = 'event-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(description, "<div><strong>Safe</strong></div>");
    });
}
