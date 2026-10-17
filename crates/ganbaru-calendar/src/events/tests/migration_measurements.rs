//! Isolated CPU diagnostics for the fixture in `docs/performance/calendar-migration.md`.
//! Run explicitly with one Cargo job and test thread; these are not UI timings.

use super::fixtures::{in_memory_pool, insert_test_event_at};
use crate::events::{edit, preview, scope};
use crate::recurrence::canonical::{
    EditScope, ScopeClock, StoredTemplate, Template, Window, expand_templates, parse_date,
};
use serde_json::{Value, json};
use std::hint::black_box;
use std::time::Instant;

const TEMPLATE_COUNT: usize = 64;
const WINDOW_COUNT: usize = 12;
const WINDOW_DAYS: usize = 7;
const WARMUPS: usize = 5;
const SAMPLES: usize = 30;
const START: &str = "2026-05-01T09:00:00Z";
const END: &str = "2026-05-01T10:00:00Z";
const RULE: &str = "FREQ=DAILY;COUNT=365";

fn summarize(initial: f64, mut samples: Vec<f64>) -> Value {
    assert_eq!(samples.len(), SAMPLES);
    samples.sort_by(f64::total_cmp);
    json!({"initialMs": initial, "p50Ms": samples[SAMPLES / 2], "p95Ms": samples[28]})
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

#[test]
#[ignore = "isolated migration CPU diagnostic, run explicitly without other validation"]
fn calendar_migration_cpu_measurements() {
    crate::test_support::block_on(async {
        let identities: Vec<_> = (0..TEMPLATE_COUNT)
            .map(|index| format!("baseline-{index}"))
            .collect();
        let templates: Vec<_> = identities
            .iter()
            .map(|id| {
                Template::from_stored(StoredTemplate {
                    id,
                    start: START,
                    end: END,
                    home_zone: "UTC",
                    all_day: false,
                    rrule: Some(RULE),
                    repeat_until: None,
                    exceptions: &[],
                    rdates: &[],
                    overrides: Vec::new(),
                })
                .unwrap()
            })
            .collect();
        let first = parse_date("2026-05-11").unwrap();
        let windows: Vec<_> = (0..WINDOW_COUNT)
            .map(|index| {
                let start = first + chrono::Duration::days((index * WINDOW_DAYS) as i64);
                let end = start + chrono::Duration::days((WINDOW_DAYS - 1) as i64);
                Window::new(
                    &start.to_string(),
                    &end.to_string(),
                    &jiff::tz::TimeZone::UTC,
                )
                .unwrap()
            })
            .collect();
        let navigation = || {
            let started = Instant::now();
            let count: usize = windows
                .iter()
                .map(|window| {
                    expand_templates(black_box(&templates), window)
                        .unwrap()
                        .iter()
                        .map(Vec::len)
                        .sum::<usize>()
                })
                .sum();
            let elapsed = elapsed_ms(started);
            assert_eq!(count, TEMPLATE_COUNT * WINDOW_DAYS * WINDOW_COUNT);
            elapsed
        };
        let navigation_initial = navigation();
        for _ in 0..WARMUPS {
            navigation();
        }
        let navigation_samples = (0..SAMPLES).map(|_| navigation()).collect();

        let pool = in_memory_pool().await;
        for id in &identities {
            insert_test_event_at(&pool, id, START, END, Some(RULE)).await;
        }
        let selected = parse_date("2026-05-15").unwrap();
        let clock = ScopeClock {
            epoch_ms: chrono::DateTime::parse_from_rfc3339("2026-05-08T00:00:00Z")
                .unwrap()
                .timestamp_millis(),
            floating_today: Some(parse_date("2026-05-08").unwrap()),
        };
        let mut cpu = Vec::new();
        let mut reads = Vec::new();
        let mut reply_bytes = None;
        for _ in 0..(1 + WARMUPS + SAMPLES) {
            let read_started = Instant::now();
            let mut tx = pool.begin().await.unwrap();
            let snapshot = scope::read_snapshot(&mut tx, &identities[0]).await.unwrap();
            tx.commit().await.unwrap();
            reads.push(elapsed_ms(read_started));
            let started = Instant::now();
            let prepared = edit::prepare(
                snapshot,
                selected,
                EditScope::All,
                clock,
                serde_json::from_value(json!({"fields":[{"field":"title", "value":"Edited"}]}))
                    .unwrap(),
            )
            .unwrap();
            let projection =
                preview::project(prepared, "measurement-preview", &windows[0], clock.epoch_ms)
                    .unwrap();
            cpu.push(elapsed_ms(started));
            assert_eq!(projection.window.occurrences.len(), WINDOW_DAYS);
            // The remaining cached families stay in the frontend; one native
            // source family plus those families still produces 448 cards.
            assert_eq!(
                projection.window.occurrences.len() + (TEMPLATE_COUNT - 1) * WINDOW_DAYS,
                TEMPLATE_COUNT * WINDOW_DAYS
            );
            let bytes = serde_json::to_vec(&projection).unwrap().len();
            assert_eq!(*reply_bytes.get_or_insert(bytes), bytes);
            black_box(projection);
        }
        let cpu_initial = cpu[0];
        let read_initial = reads[0];
        println!(
            "CALENDAR_MIGRATION_MEASUREMENTS {}",
            json!({
                "recordedAt": chrono::Utc::now().to_rfc3339(),
                "navigation": summarize(navigation_initial, navigation_samples),
                "boundedPreviewCpu": summarize(cpu_initial, cpu.into_iter().skip(1 + WARMUPS).collect()),
                "previewSqlSnapshot": summarize(read_initial, reads.into_iter().skip(1 + WARMUPS).collect()),
                "previewSerializedBytes": reply_bytes.unwrap(),
                "profile": "Cargo test profile, not a release UI benchmark",
            })
        );
    });
}
