//! Listing and resolving Calendar conflicts through the domain commands between a hub and a
//! phone.

use std::collections::HashMap;

use super::calendar_recovery::{event_sql, publish_local_calendar};
use super::projects_recovery::Pair;
use super::{HUB_DEVICE, NOW_MS, PHONE_DEVICE, Replica, VAULT_ID, block_on};
use ganbaru_calendar::conflicts::{
    CalendarConflictResolution, conflicts_from_pool, resolve_from_pool,
};
use ganbaru_sync::DeviceNames;
use serde_json::{Value, json};

/// The Calendar conflicts on `replica`, as the frontend receives them.
async fn conflicts(replica: &Replica, device: &str) -> Value {
    let read = conflicts_from_pool(
        &replica.pool,
        VAULT_ID,
        &DeviceNames::new(device, HashMap::new()),
    )
    .await
    .expect("read conflicts");
    serde_json::to_value(read).expect("serialize conflicts")
}

/// Resolves on `replica` through the command entry point, as the frontend requests it, and
/// seals the resolution.
async fn resolve(replica: &mut Replica, request: Value) {
    let request: CalendarConflictResolution =
        serde_json::from_value(request).expect("resolution request");
    resolve_from_pool(&replica.pool, VAULT_ID, request, NOW_MS + 1)
        .await
        .expect("resolve conflict");
    replica.seal().await;
}

/// The single open conflict on both replicas, which must agree on its displayed version.
async fn single_conflict(pair: &Pair) -> Value {
    let hub = conflicts(&pair.hub, HUB_DEVICE).await;
    let phone = conflicts(&pair.phone, PHONE_DEVICE).await;
    let conflicts = phone["conflicts"].as_array().expect("conflicts");
    assert_eq!(conflicts.len(), 1, "{phone}");
    let displayed = |read: &Value| {
        read["conflicts"][0]["versions"]
            .as_array()
            .expect("versions")
            .iter()
            .find(|version| version["displayed"] == true)
            .map(|version| version["version"].clone())
    };
    assert_eq!(displayed(&hub), displayed(&phone));
    conflicts[0].clone()
}

async fn assert_no_conflicts(pair: &Pair) {
    for (replica, device) in [(&pair.hub, HUB_DEVICE), (&pair.phone, PHONE_DEVICE)] {
        assert_eq!(conflicts(replica, device).await["conflicts"], json!([]));
    }
}

/// A pair sharing `event-1` in the published Local calendar.
async fn pair_with_event(label: &str) -> Pair {
    let mut pair = Pair::new(label).await;
    publish_local_calendar(&mut pair).await;
    pair.hub_edit(&event_sql("event-1", "local")).await;
    pair.converge().await;
    pair
}

/// Recurring timing the phone writes, with one excluded date.
const RECURRING_START: &str = "2026-10-24T15:00:00Z";

/// Single-event timing the hub writes.
const SINGLE_START: &str = "2026-10-24T11:00:00Z";

/// Moves `event-1` concurrently on both devices, resolves on the hub with the version starting
/// at `chosen_start` whether or not it is displayed, and checks both replicas hold its timing
/// with its excluded dates, overrides, and override properties, and no conflict.
async fn resolve_timing_conflict(label: &str, chosen_start: &str, expected: &str) {
    let mut pair = pair_with_event(label).await;
    pair.hub_edit(
        "UPDATE calendar_events
             SET start_time = '2026-10-24T11:00:00Z', end_time = '2026-10-24T12:00:00Z'
             WHERE id = 'event-1'",
    )
    .await;
    pair.phone_edit(
        "UPDATE calendar_events
             SET start_time = '2026-10-24T15:00:00Z', end_time = '2026-10-24T16:00:00Z',
                 rrule = 'FREQ=DAILY;COUNT=3'
             WHERE id = 'event-1';
             INSERT INTO calendar_event_exdates (id, event_id, occurrence_date)
             VALUES ('exdate-1', 'event-1', '2026-10-25');
             INSERT INTO calendar_event_overrides (id, parent_event_id, recurrence_id, title)
             VALUES ('override-1', 'event-1', '2026-10-26T15:00:00Z', 'Moved review');
             INSERT INTO calendar_event_override_extended_properties
                 (event_id, override_id, property_key, id, property_value)
             VALUES ('event-1', 'override-1', 'X-NOTE', 'property-1', 'Bring notes');",
    )
    .await;
    pair.converge().await;

    let conflict = single_conflict(&pair).await;
    assert_eq!(conflict["eventId"], "event-1");
    assert_eq!(conflict["field"], "timing");
    let versions = conflict["versions"].as_array().expect("versions");
    assert_eq!(versions.len(), 2);
    let chosen = versions
        .iter()
        .find(|version| version["value"]["startTime"] == chosen_start)
        .expect("chosen version");
    assert_eq!(chosen["value"]["kind"], "timing");
    let recurring = versions
        .iter()
        .find(|version| version["value"]["startTime"] == RECURRING_START)
        .expect("recurring version");
    assert_eq!(recurring["value"]["rrule"], "FREQ=DAILY;COUNT=3");
    assert_eq!(recurring["value"]["excludedDates"], 1);
    assert_eq!(recurring["value"]["overrides"], 1);

    resolve(
        &mut pair.hub,
        json!({
            "eventId": "event-1",
            "field": "timing",
            "choice": { "kind": "version", "version": chosen["version"] },
        }),
    )
    .await;
    pair.converge().await;

    assert_no_conflicts(&pair).await;
    assert_eq!(
        pair.texts(
            "SELECT e.start_time || '|' || coalesce(e.rrule, '') || '|' || coalesce(
                     (SELECT group_concat(x.id || ':' || x.occurrence_date)
                      FROM calendar_event_exdates x WHERE x.event_id = e.id), '')
                 || '|' || coalesce(
                     (SELECT group_concat(o.id || ':' || o.recurrence_id || ':' || o.title)
                      FROM calendar_event_overrides o WHERE o.parent_event_id = e.id), '')
                 || '|' || coalesce(
                     (SELECT group_concat(p.id || ':' || p.override_id || ':' || p.property_key
                                          || ':' || p.property_value)
                      FROM calendar_event_override_extended_properties p
                      WHERE p.event_id = e.id), '')
                 FROM calendar_events e WHERE e.id = 'event-1'"
                .to_string()
        )
        .await,
        [expected]
    );
    pair.finish().await;
}

#[test]
fn a_timing_conflict_resolved_with_the_recurring_version_writes_its_excluded_dates_everywhere() {
    block_on(resolve_timing_conflict(
        "calendar-conflict-timing-recurring",
        RECURRING_START,
        "2026-10-24T15:00:00Z|FREQ=DAILY;COUNT=3|exdate-1:2026-10-25|override-1:2026-10-26T15:00:00Z:Moved review|property-1:override-1:X-NOTE:Bring notes",
    ));
}

#[test]
fn a_timing_conflict_resolved_with_the_single_version_drops_the_excluded_dates_everywhere() {
    block_on(resolve_timing_conflict(
        "calendar-conflict-timing-single",
        SINGLE_START,
        "2026-10-24T11:00:00Z||||",
    ));
}

#[test]
fn a_title_conflict_kept_as_displayed_closes_everywhere() {
    block_on(async {
        let mut pair = pair_with_event("calendar-conflict-title").await;
        pair.hub_edit("UPDATE calendar_events SET title = 'Hub title' WHERE id = 'event-1'")
            .await;
        pair.phone_edit("UPDATE calendar_events SET title = 'Phone title' WHERE id = 'event-1'")
            .await;
        pair.converge().await;

        let conflict = single_conflict(&pair).await;
        assert_eq!(conflict["field"], "title");
        let displayed = conflict["versions"]
            .as_array()
            .expect("versions")
            .iter()
            .find(|version| version["displayed"] == true)
            .expect("displayed version")["value"]["text"]
            .clone();

        resolve(
            &mut pair.phone,
            json!({ "eventId": "event-1", "field": "title", "choice": { "kind": "displayed" } }),
        )
        .await;
        pair.converge().await;

        assert_no_conflicts(&pair).await;
        assert_eq!(
            pair.texts("SELECT title FROM calendar_events WHERE id = 'event-1'".to_string())
                .await,
            [displayed.as_str().expect("title")]
        );
        pair.finish().await;
    });
}
