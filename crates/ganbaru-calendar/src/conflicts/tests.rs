use std::collections::HashMap;

use ganbaru_sync::domains::calendar::encode_rows;

use super::*;

fn text(value: &str) -> Field {
    Field::Text(value.to_string())
}

fn blob(table: &OwnedTable, rows: Vec<Vec<Field>>) -> Field {
    Field::Blob(encode_rows(table, rows).expect("encode owned rows"))
}

/// A recurring timing value with two excluded dates and no added dates or overrides.
fn timing_value() -> Value {
    Value::new(vec![
        text("2026-10-24T09:00:00Z"),
        text("2026-10-24T10:00:00Z"),
        text("America/Monterrey"),
        Field::Integer(0),
        text("FREQ=DAILY;COUNT=5"),
        Field::Null,
        blob(
            &EXDATES,
            vec![
                vec![text("2026-10-25"), text("exdate-1")],
                vec![text("2026-10-26"), text("exdate-2")],
            ],
        ),
        blob(&RDATES, Vec::new()),
        blob(&OVERRIDES, Vec::new()),
        blob(&OVERRIDE_PROPERTIES, Vec::new()),
    ])
    .expect("a timing value fits the bounds")
}

#[test]
fn a_timing_value_reads_as_its_columns_and_owned_row_counts() {
    assert_eq!(
        value_read(CalendarConflictField::Timing, &timing_value()),
        Ok(CalendarConflictValue::Timing(CalendarTimingSummary {
            start_time: "2026-10-24T09:00:00Z".to_string(),
            end_time: "2026-10-24T10:00:00Z".to_string(),
            timezone: "America/Monterrey".to_string(),
            all_day: false,
            rrule: Some("FREQ=DAILY;COUNT=5".to_string()),
            repeat_until: None,
            excluded_dates: 2,
            added_dates: 0,
            overrides: 0,
        }))
    );
}

#[test]
fn text_fields_read_one_text_and_refuse_a_timing_value() {
    let title = Value::new(vec![text("Review")]).unwrap();
    assert_eq!(
        value_read(CalendarConflictField::Title, &title),
        Ok(CalendarConflictValue::Text {
            text: "Review".to_string()
        })
    );
    assert!(value_read(CalendarConflictField::Description, &timing_value()).is_err());
    assert!(value_read(CalendarConflictField::Timing, &title).is_err());
}

#[test]
fn only_title_description_and_timing_groups_are_conflict_fields() {
    for field in CalendarConflictField::ALL {
        assert_eq!(
            CalendarConflictField::from_group(field.group()),
            Some(field)
        );
    }
    assert_eq!(CalendarConflictField::from_group(event_group::COLOR), None);
}

#[test]
fn a_vault_that_does_not_sync_reads_no_conflicts_and_refuses_resolutions() {
    crate::test_support::block_on(async {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        ganbaru_db::test_support::apply_migrated_template(&pool)
            .await
            .unwrap();
        let devices = DeviceNames::new("device", HashMap::new());
        let read = conflicts_from_pool(&pool, "vault", &devices)
            .await
            .expect("read conflicts of an unlinked vault");
        assert!(read.conflicts.is_empty());

        let refused = resolve_from_pool(
            &pool,
            "vault",
            CalendarConflictResolution {
                event_id: "event-1".to_string(),
                field: CalendarConflictField::Timing,
                choice: CalendarConflictChoice::Displayed,
            },
            1,
        )
        .await;
        assert!(matches!(refused, Err(CalendarConflictError::Resolved(_))));
    });
}
