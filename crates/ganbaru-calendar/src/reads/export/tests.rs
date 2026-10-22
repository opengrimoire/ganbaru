use super::*;

async fn pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
    seed(&pool).await;
    pool
}

async fn seed(pool: &SqlitePool) {
    sqlx::raw_sql(r#"
        INSERT INTO calendars (id, name, source) VALUES ('export', 'Current name', 'local'), ('other', 'Other', 'local');
        INSERT INTO icalendar_objects (id, calendar_id, source_kind, source_fingerprint, method, created_at, updated_at)
            VALUES ('object', 'export', 'import-file', 'source', 'request', 'now', 'now');
        INSERT INTO icalendar_components (id, object_id, parent_component_id, calendar_id, component_type, preservation_status, created_at, updated_at)
            VALUES ('root', 'object', NULL, 'export', 'vcalendar', 'lossless', 'now', 'now'),
                   ('live', 'object', 'root', 'export', 'vevent', 'lossless', 'now', 'now'),
                   ('deleted', 'object', 'root', 'export', 'vevent', 'lossless', 'now', 'now'),
                   ('zone', 'object', 'root', 'export', 'vtimezone', 'lossless', 'now', 'now'),
                   ('standard', 'object', 'zone', 'export', 'standard', 'lossless', 'now', 'now'),
                   ('todo', 'object', 'root', 'export', 'vtodo', 'unsupported', 'now', 'now'),
                   ('exception', 'object', 'root', 'export', 'vevent', 'lossless', 'now', 'now');
        INSERT INTO calendar_events (id, title, start_time, end_time, calendar_id, icalendar_component_id)
            VALUES ('event', 'Before', '2026-10-02T09:00:00Z', '2026-10-02T10:00:00Z', 'export', 'live'),
                   ('unrelated', 'Other', '2026-10-02T09:00:00Z', '2026-10-02T10:00:00Z', 'other', NULL);
        INSERT INTO calendar_event_attendees (id, event_id, email, role, status, sort_order)
            VALUES ('attendee', 'event', 'guest@example.test', 'req-participant', 'accepted', 0);
        INSERT INTO calendar_event_alarms (id, event_id, action, trigger_type, trigger_value)
            VALUES ('alarm', 'event', 'display', 'relative', '-PT10M');
        INSERT INTO calendar_event_notifications (id, event_id, offset_minutes) VALUES ('notice', 'event', 10);
        INSERT INTO calendar_event_categories (id, event_id, category, sort_order)
            VALUES ('category-b', 'event', 'Second', 1), ('category-a', 'event', 'First', 0);
        INSERT INTO calendar_event_exdates (id, event_id, occurrence_date) VALUES ('exdate', 'event', '2026-10-03');
        INSERT INTO calendar_event_rdates (id, event_id, occurrence_start) VALUES ('rdate', 'event', '2026-10-04T09:00:00Z');
        INSERT INTO calendar_event_extended_properties (id, event_id, property_key, property_value)
            VALUES ('extension', 'event', 'X-PROJECTED', 'current');
        INSERT INTO calendar_event_organizers (event_id, name, email) VALUES ('event', 'Organizer', 'owner@example.test');
        INSERT INTO calendar_event_overrides (id, parent_event_id, recurrence_id, title, icalendar_component_id)
            VALUES ('override', 'event', '2026-10-05', 'Moved', 'exception');
        INSERT INTO calendar_event_override_extended_properties (id, override_id, property_key, property_value)
            VALUES ('override-extension', 'override', 'X-OVERRIDE', 'yes');
        INSERT INTO icalendar_component_properties (id, component_id, name, value_type)
            VALUES ('property', 'live', 'x-preserved', 'text'), ('todo-property', 'todo', 'summary', 'text'),
                   ('zone-property', 'zone', 'tzid', 'text');
        INSERT INTO icalendar_property_parameters (id, property_id, name) VALUES ('parameter', 'property', 'x-param');
        INSERT INTO icalendar_value_nodes (id, property_id, parameter_id, value_kind, text_value)
            VALUES ('text', 'property', NULL, 'text', 'original'), ('param-text', NULL, 'parameter', 'text', 'kept'),
                   ('todo-text', 'todo-property', NULL, 'text', 'Task'), ('zone-text', 'zone-property', NULL, 'text', 'Custom/Zone');
        INSERT INTO icalendar_component_projection_warnings (id, component_id, message) VALUES ('warning', 'live', 'Preserved extension');
    "#).execute(pool).await.unwrap();
}

#[test]
fn archive_storage_custody_does_not_publish_its_components_or_method_in_local_exports() {
    crate::test_support::block_on(async {
        {
            let pool = pool().await;
            sqlx::raw_sql(r#"
                INSERT INTO calendar_event_archives
                    (id, source_event_id, archived_at, title, start_time, end_time, calendar_id, icalendar_component_id, created_at, updated_at)
                    SELECT id, id, '2026-10-02T10:00:00Z', title, start_time, end_time, calendar_id, icalendar_component_id, created_at, updated_at
                    FROM calendar_events WHERE id = 'event';
                INSERT INTO calendar_event_archive_import_objects (archive_event_id, object_id) VALUES ('event', 'object');
                DELETE FROM calendar_events WHERE id = 'event';
                INSERT INTO icalendar_objects (id, calendar_id, source_kind, source_fingerprint, method, created_at, updated_at)
                    VALUES ('standalone', 'local', 'import-file', 'standalone', 'publish', 'now', 'now');
                INSERT INTO icalendar_components (id, object_id, parent_component_id, calendar_id, component_type, preservation_status, created_at, updated_at)
                    VALUES ('standalone-root', 'standalone', NULL, 'local', 'vcalendar', 'lossless', 'now', 'now'),
                           ('standalone-journal', 'standalone', 'standalone-root', 'local', 'vjournal', 'unsupported', 'now', 'now');
            "#).execute(&pool).await.unwrap();
            let mut tx = pool.begin().await.unwrap();
            crate::calendars::retain_archived_imports_tx(&mut tx, "export")
                .await
                .unwrap();
            sqlx::query("DELETE FROM calendars WHERE id = 'export'")
                .execute(&mut *tx)
                .await
                .unwrap();
            tx.commit().await.unwrap();

            let snapshot = load_export_snapshot(&pool, "local").await.unwrap();
            assert!(snapshot.events.is_empty());
            assert!(snapshot.timezones.is_empty());
            assert_eq!(snapshot.passthrough_components.len(), 1);
            assert_eq!(snapshot.passthrough_components[0][0], "vjournal");
            assert_eq!(snapshot.metadata.method.as_deref(), Some("PUBLISH"));
            assert!(!snapshot.metadata.mixed_methods);
            let retained: (String, String, i64) = sqlx::query_as(
                "SELECT archive.calendar_id, object.calendar_id, (SELECT COUNT(*) FROM icalendar_components WHERE object_id = object.id)
                 FROM calendar_event_archives archive JOIN icalendar_components component ON component.id = archive.icalendar_component_id
                 JOIN icalendar_objects object ON object.id = component.object_id WHERE archive.id = 'event'"
            ).fetch_one(&pool).await.unwrap();
            assert_eq!(retained, ("export".into(), "local".into(), 7));

            // Live projections take precedence over archive ownership. Their
            // shared preservation becomes exportable without deleting history.
            sqlx::query("INSERT INTO calendar_events (id, title, start_time, end_time, calendar_id, icalendar_component_id)
                         VALUES ('restored', 'Restored', '2026-10-02T09:00:00Z', '2026-10-02T10:00:00Z', 'local', 'live')")
                .execute(&pool).await.unwrap();
            let restored = load_export_snapshot(&pool, "local").await.unwrap();
            assert_eq!(restored.events.len(), 1);
            assert_eq!(restored.timezones.len(), 1);
            assert_eq!(restored.passthrough_components.len(), 2);
            assert!(restored.metadata.mixed_methods);
        }
    });
}

#[test]
fn serialized_export_budget_counts_escaped_json_bytes() {
    let mut writer = ExportSizeWriter { remaining: 8 };
    serde_json::to_writer(&mut writer, "abc").unwrap();
    assert_eq!(writer.remaining, 3);
    assert!(serde_json::to_writer(&mut writer, "\n\n").is_err());
}

#[test]
fn export_snapshot_batches_current_projection_and_selected_preservation() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        let snapshot = load_export_snapshot(&pool, "export").await.unwrap();
        assert_eq!(snapshot.calendar.name, "Current name");
        assert_eq!(snapshot.events.len(), 1);
        let rows = &snapshot.events[0];
        let event = rows.event.as_ref().unwrap();
        assert_eq!(event.title, "Before");
        assert_eq!(event.notifications.as_deref(), Some("[10]"));
        assert_eq!(event.categories.as_deref(), Some("[\"First\",\"Second\"]"));
        assert_eq!(event.exceptions.as_deref(), Some("[\"2026-10-03\"]"));
        assert_eq!(event.rdate.as_deref(), Some("[\"2026-10-04T09:00:00Z\"]"));
        assert_eq!(
            event.extended_properties.as_deref(),
            Some("{\"X-PROJECTED\":\"current\"}")
        );
        assert_eq!(
            serde_json::from_str::<Value>(event.organizer.as_ref().unwrap()).unwrap(),
            serde_json::json!({"email":"owner@example.test", "name":"Organizer"})
        );
        assert_eq!(rows.attendees.len(), 1);
        assert_eq!(rows.alarms.len(), 1);
        assert_eq!(rows.overrides.len(), 1);
        assert_eq!(
            rows.overrides[0].extended_properties.as_deref(),
            Some("{\"X-OVERRIDE\":\"yes\"}")
        );
        assert_eq!(
            rows.overrides[0].icalendar_raw_jcal.as_deref(),
            Some("[\"vevent\",[],[]]")
        );
        let jcal: Value = serde_json::from_str(event.icalendar_raw_jcal.as_ref().unwrap()).unwrap();
        assert_eq!(
            jcal,
            serde_json::json!(["vevent", [["x-preserved", {"x-param":"kept"}, "text", "original"]], []])
        );
        assert_eq!(
            snapshot.timezones,
            vec![serde_json::json!([
                "vtimezone",
                [["tzid", {}, "text", "Custom/Zone"]],
                [["standard", [], []]]
            ])]
        );
        assert_eq!(
            snapshot.passthrough_components,
            vec![serde_json::json!([
                "vtodo",
                [["summary", {}, "text", "Task"]],
                []
            ])]
        );
        assert_eq!(snapshot.metadata.method.as_deref(), Some("REQUEST"));
        assert!(!snapshot.metadata.mixed_methods);
        assert!(read_export_rows(&pool, "missing").await.is_err());
    });
}

#[test]
fn export_snapshot_keeps_all_children_and_stable_order_for_a_large_calendar() {
    crate::test_support::block_on(async {
        const EVENT_COUNT: i64 = 2_500;
        let pool = pool().await;
        sqlx::query(
            "WITH RECURSIVE numbered(n) AS (
                VALUES(1) UNION ALL SELECT n + 1 FROM numbered WHERE n < ?
            ) INSERT INTO calendar_events (id, title, start_time, end_time, calendar_id)
              SELECT printf('large-%05d', n), printf('Meeting %d', n),
                     '2026-10-02T09:00:00Z', '2026-10-02T10:00:00Z', 'export'
              FROM numbered",
        )
        .bind(EVENT_COUNT)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(
            "INSERT INTO calendar_event_attendees (id, event_id, email, role, status)
                SELECT 'guest-' || id, id, id || '@example.test', 'req-participant', 'accepted'
                FROM calendar_events WHERE id LIKE 'large-%';
             INSERT INTO calendar_event_alarms (id, event_id, action, trigger_type, trigger_value)
                SELECT 'alarm-' || id, id, 'display', 'relative', '-PT10M'
                FROM calendar_events WHERE id LIKE 'large-%';
             INSERT INTO calendar_event_overrides (id, parent_event_id, recurrence_id, title)
                SELECT 'override-' || id, id, '2026-10-03', 'Moved ' || id
                FROM calendar_events WHERE id LIKE 'large-%';",
        )
        .execute(&pool)
        .await
        .unwrap();
        let first = load_export_snapshot(&pool, "export").await.unwrap();
        assert_eq!(first.events.len(), EVENT_COUNT as usize + 1);
        for rows in &first.events {
            let event = rows.event.as_ref().unwrap();
            assert_eq!(rows.attendees.len(), 1, "{} attendees", event.id);
            assert_eq!(rows.alarms.len(), 1, "{} alarms", event.id);
            assert_eq!(rows.overrides.len(), 1, "{} overrides", event.id);
            assert_eq!(rows.attendees[0].event_id, event.id);
            assert_eq!(rows.alarms[0].event_id, event.id);
            assert_eq!(rows.overrides[0].parent_event_id, event.id);
            if event.id.starts_with("large-") {
                assert_eq!(
                    rows.attendees[0].email,
                    format!("{}@example.test", event.id)
                );
                assert_eq!(rows.overrides[0].title, Some(format!("Moved {}", event.id)));
            }
        }
        let second = load_export_snapshot(&pool, "export").await.unwrap();
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(second).unwrap()
        );
    });
}

#[test]
fn export_snapshot_holds_one_revision_while_another_connection_commits() {
    crate::test_support::block_on(async {
        let path = std::env::temp_dir().join(format!(
            "ganbaru-calendar-export-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .foreign_keys(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        ganbaru_db::run_migrations(&pool).await.unwrap();
        seed(&pool).await;
        let mut transaction = pool.begin().await.unwrap();
        // Establish the same snapshot that the command's first header read establishes.
        sqlx::query("SELECT name FROM calendars WHERE id = 'export'")
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
        sqlx::raw_sql(
            "BEGIN; UPDATE calendars SET name = 'After' WHERE id = 'export';
            UPDATE calendar_events SET title = 'After' WHERE id = 'event';
            UPDATE calendar_event_attendees SET email = 'after@example.test' WHERE id = 'attendee';
            UPDATE icalendar_value_nodes SET text_value = 'after' WHERE id = 'text';
            UPDATE icalendar_objects SET method = 'PUBLISH' WHERE id = 'object'; COMMIT;",
        )
        .execute(&pool)
        .await
        .unwrap();
        let rows = read_export_rows_on_connection(&mut transaction, "export")
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        let before = rows.assemble().unwrap();
        assert_eq!(before.calendar.name, "Current name");
        assert_eq!(before.events[0].event.as_ref().unwrap().title, "Before");
        assert_eq!(before.events[0].attendees[0].email, "guest@example.test");
        assert!(
            before.events[0]
                .event
                .as_ref()
                .unwrap()
                .icalendar_raw_jcal
                .as_ref()
                .unwrap()
                .contains("original")
        );
        assert_eq!(before.metadata.method.as_deref(), Some("REQUEST"));
        let after = load_export_snapshot(&pool, "export").await.unwrap();
        assert_eq!(after.calendar.name, "After");
        assert_eq!(after.events[0].event.as_ref().unwrap().title, "After");
        assert_eq!(after.metadata.method.as_deref(), Some("PUBLISH"));
        pool.close().await;
        std::fs::remove_file(path).unwrap();
    });
}

#[test]
fn export_preflight_rejects_excess_rows_and_bytes_before_fetching() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        let mut connection = pool.acquire().await.unwrap();
        let sql = "SELECT id, name, color, source, source_url FROM calendars WHERE id = ?1";
        for (records_left, bytes_left, expected) in
            [(0, MAX_EXPORT_BYTES, "record limit"), (10, 1, "byte limit")]
        {
            let mut budget = ExportBudget {
                records_left,
                bytes_left,
            };
            let result = budget
                .read::<CalendarExportHeader>(&mut connection, "export", sql, &["name"])
                .await;
            assert!(result.err().unwrap().contains(expected));
        }
    });
}

#[test]
fn export_rejects_cyclic_preserved_values_instead_of_dropping_them() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        sqlx::query("UPDATE icalendar_value_nodes SET parent_node_id = id, value_kind = 'array' WHERE id = 'text'")
            .execute(&pool).await.unwrap();
        assert!(
            load_export_snapshot(&pool, "export")
                .await
                .err()
                .unwrap()
                .contains("depth limit")
        );
    });
}

#[test]
fn export_bounds_repeated_preserved_components_during_assembly() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        sqlx::query("UPDATE icalendar_value_nodes SET text_value = ? WHERE id = 'text'")
            .bind("x".repeat(100_000))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("WITH RECURSIVE copies(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM copies WHERE n < 1000)
            INSERT INTO calendar_events (id, start_time, end_time, calendar_id, icalendar_component_id)
            SELECT 'copy-' || n, '2026-10-02T09:00:00Z', '2026-10-02T10:00:00Z', 'export', 'live' FROM copies")
            .execute(&pool).await.unwrap();
        // The source is well within the read budget. Repeated output still has its own shared limit.
        let rows = read_export_rows(&pool, "export").await.unwrap();
        assert!(rows.assemble().err().unwrap().contains("byte limit"));
    });
}

#[test]
fn export_rejects_concurrent_preparation_without_queuing_another_snapshot() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        let permit = EXPORT_GATE.try_acquire().unwrap();
        assert!(
            load_export_snapshot(&pool, "export")
                .await
                .err()
                .unwrap()
                .contains("already being prepared")
        );
        drop(permit);
        assert!(load_export_snapshot(&pool, "export").await.is_ok());
    });
}
