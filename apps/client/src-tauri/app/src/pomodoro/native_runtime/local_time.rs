//! Historical native time facts for accepted adaptive decisions.

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;

use ganbaru_pomodoro::adaptive::models::LocalTimeFacts;
use ganbaru_pomodoro::{
    FocusExecutionError, FocusLocalTimeResolver, PomodoroAdaptivePlannedBlockWrite,
};

pub(super) struct NativeLocalTime(pub tauri::AppHandle);

impl FocusLocalTimeResolver for NativeLocalTime {
    fn resolve(
        &self,
        instants: BTreeSet<i64>,
    ) -> Pin<Box<dyn Future<Output = Result<LocalTimeFacts, FocusExecutionError>> + Send + '_>>
    {
        Box::pin(async move {
            let app = self.0.clone();
            let instants = instants.into_iter().collect::<Vec<_>>();
            tauri::async_runtime::spawn_blocking(move || resolve(&app, &instants))
                .await
                .map_err(|error| format!("Join native adaptive local-time query: {error}"))?
                .map_err(Into::into)
        })
    }
}

#[cfg(not(target_os = "android"))]
const MAX_LOCAL_TIME_FACTS: usize = 4_096;

#[cfg(not(target_os = "android"))]
fn resolve(_app: &tauri::AppHandle, instants: &[i64]) -> Result<LocalTimeFacts, String> {
    local_time_facts(instants, &ganbaru_civil_time::system_zone()?)
}

/// Map each instant to its local date key, hour, and an English date string in
/// JavaScript `Date.toDateString` format, used as a seed.
#[cfg(not(target_os = "android"))]
fn local_time_facts(instants: &[i64], zone: &jiff::tz::TimeZone) -> Result<LocalTimeFacts, String> {
    use chrono::Timelike;
    use ganbaru_pomodoro::adaptive::models::LocalTimeFact;
    if instants.len() > MAX_LOCAL_TIME_FACTS {
        return Err("Focus local-time query exceeds 4096 instants".into());
    }
    instants
        .iter()
        .map(|&epoch_ms| {
            let local = ganbaru_civil_time::instant_to_local(epoch_ms, zone)?;
            Ok((
                epoch_ms,
                LocalTimeFact {
                    epoch_ms,
                    date_key: local.format("%Y-%m-%d").to_string(),
                    date_string: local.format("%a %b %d %Y").to_string(),
                    hour: local.hour() as u8,
                },
            ))
        })
        .collect()
}

#[cfg(target_os = "android")]
fn resolve(app: &tauri::AppHandle, instants: &[i64]) -> Result<LocalTimeFacts, String> {
    use ganbaru_mobile_notifications::MobileNotificationsExt;
    use ganbaru_pomodoro::adaptive::models::LocalTimeFact;
    let values = app
        .mobile_notifications()
        .device_local_time_facts(instants)?;
    if values.len() != instants.len() {
        return Err("Android returned incomplete adaptive local-time facts".to_owned());
    }
    let mut facts = LocalTimeFacts::new();
    for value in values {
        let fact = LocalTimeFact {
            epoch_ms: value.epoch_ms,
            date_key: value.date_key,
            date_string: value.date_string,
            hour: value.hour,
        };
        if facts.insert(fact.epoch_ms, fact).is_some() {
            return Err("Android returned duplicate adaptive local-time facts".to_owned());
        }
    }
    Ok(facts)
}

pub(super) fn planned_block(
    value: ganbaru_calendar::reads::focus_context::FocusPlannedBlock,
) -> PomodoroAdaptivePlannedBlockWrite {
    PomodoroAdaptivePlannedBlockWrite {
        event_date: value.event_date,
        event_id: Some(value.event_id),
        original_event_id: value.original_event_id,
        planned_start: value.planned_start,
        planned_end: value.planned_end,
        source_kind: value.source_kind.to_owned(),
    }
}

#[cfg(all(test, not(target_os = "android")))]
mod tests {
    use super::*;

    #[test]
    fn adaptive_seed_facts_use_the_selected_local_day_and_english_spelling() {
        let timestamp = "2024-03-10T04:30:00Z"
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_millisecond();
        let zone = ganbaru_civil_time::zone("America/New_York").unwrap();
        let facts = local_time_facts(&[timestamp, timestamp], &zone).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[&timestamp].date_key, "2024-03-09");
        assert_eq!(facts[&timestamp].date_string, "Sat Mar 09 2024");
        assert_eq!(facts[&timestamp].hour, 23);
        assert!(
            local_time_facts(
                &vec![timestamp; MAX_LOCAL_TIME_FACTS + 1],
                &jiff::tz::TimeZone::UTC
            )
            .is_err()
        );
    }
}
