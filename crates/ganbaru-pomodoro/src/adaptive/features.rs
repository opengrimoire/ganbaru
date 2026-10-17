use std::collections::{BTreeMap, BTreeSet};

use crate::adaptive::models::*;
use crate::adaptive::policy::push_unique;

const FOCUS_EDGE_RATIO: f64 = 0.25;
const FOCUS_EDGE_SECONDS: f64 = 300.0;
const EARLY_MANUAL_END_TOLERANCE_MS: i64 = 1000;
const EXPLICIT_EVENT_MATCH_TOLERANCE_MS: i64 = 1500;
const BLOCK_BURST_WINDOW_MS: i64 = 600_000;
const BLOCK_BURST_MIN_ATTEMPTS: usize = 3;

/// Extract deterministic adaptive features from bounded canonical evidence.
pub fn extract_adaptive_features(input: &FeatureInput) -> FeatureVector {
    let mut totals = FeatureVector::default();
    for flag in &input.data_quality_flags {
        push_unique(&mut totals.data_quality_flags, flag);
    }
    let observed_end = input.observation_ended_at.as_deref().map(parse_time_ms);
    let mut focus_windows: Vec<(i64, i64)> = input
        .segments
        .iter()
        .filter(|segment| segment.phase == "focus")
        .map(|segment| {
            let start = segment_start_ms(segment);
            let planned_end = parse_time_ms(&segment.planned_end);
            let end = segment
                .actual_end
                .as_deref()
                .map(parse_time_ms)
                .unwrap_or(observed_end.unwrap_or(planned_end));
            (start, start.max(planned_end).max(end))
        })
        .filter(|(start, end)| end > start)
        .collect();
    focus_windows.sort_by_key(|(start, _)| *start);
    let mut break_windows: Vec<(&str, i64, i64)> = input
        .segments
        .iter()
        .filter(|segment| is_break(&segment.phase))
        .map(|segment| {
            let planned_end = parse_time_ms(&segment.planned_end);
            let end = segment
                .actual_end
                .as_deref()
                .map(parse_time_ms)
                .unwrap_or(observed_end.unwrap_or(planned_end));
            (segment.phase.as_str(), planned_end, planned_end.max(end))
        })
        .filter(|(_, planned, actual)| actual > planned)
        .collect();
    break_windows.sort_by_key(|(_, planned, _)| *planned);
    for segment in &input.segments {
        apply_segment(&mut totals, segment, observed_end);
    }
    for event in &input.run_events {
        apply_event(&mut totals, event, &input.segments, &focus_windows);
    }
    infer_manual_transitions(
        &mut totals,
        &input.segments,
        &input.run_events,
        &focus_windows,
    );
    apply_skipped_break_outcomes(&mut totals, &input.segments, &input.run_events);
    apply_block_events(
        &mut totals,
        &input.block_events,
        &focus_windows,
        &break_windows,
    );
    totals.comparable_opportunity_count = totals.comparable_opportunity_count.max(
        totals.completed_focus_segments
            + totals.interrupted_focus_segments
            + totals.break_started_count,
    );
    totals
}

/// Bucket a decision context. The platform supplies the local hour so no implicit UTC
/// conversion happens.
pub fn derive_context_bucket(
    hour: u8,
    planned_event_minutes: f64,
    session_index_today: usize,
    clean_focus_minutes_today: f64,
    energy_level: Option<f64>,
    environment_id: Option<String>,
) -> ContextBucket {
    ContextBucket {
        time_of_day: match hour {
            5..=10 => "morning",
            11..=13 => "midday",
            14..=17 => "afternoon",
            18..=21 => "evening",
            _ => "late",
        }
        .to_owned(),
        session_position: if session_index_today <= 1 {
            "first"
        } else if session_index_today <= 3 {
            "middle"
        } else {
            "late"
        }
        .to_owned(),
        event_length: if planned_event_minutes < 60.0 {
            "short"
        } else if planned_event_minutes <= 150.0 {
            "medium"
        } else {
            "long"
        }
        .to_owned(),
        workload: if clean_focus_minutes_today < 90.0 {
            "low"
        } else if clean_focus_minutes_today <= 240.0 {
            "normal"
        } else {
            "high"
        }
        .to_owned(),
        energy: match energy_level {
            None => "unknown",
            Some(value) if value <= 2.0 => "low",
            Some(value) if value >= 4.0 => "high",
            _ => "normal",
        }
        .to_owned(),
        environment_id,
    }
}

fn apply_segment(totals: &mut FeatureVector, segment: &SegmentInput, observed_end: Option<i64>) {
    let actual_start = segment.actual_start.as_deref().map(parse_time_ms);
    let actual_end = segment
        .actual_end
        .as_deref()
        .map(parse_time_ms)
        .or(observed_end);
    let planned_start = parse_time_ms(&segment.planned_start);
    let planned_end = parse_time_ms(&segment.planned_end);
    if segment.phase == "focus" {
        totals.planned_focus_seconds += bounded_seconds(planned_start, planned_end);
        let late = segment.rhythm_position.unwrap_or(1) >= 3;
        if late {
            totals.late_focus_segment_count += 1.0;
        }
        if segment.status == "completed" {
            totals.completed_focus_segments += 1.0;
        }
        if segment.status == "interrupted" {
            totals.interrupted_focus_segments += 1.0;
        }
        if segment.end_reason.as_deref() == Some("focus_failed") {
            totals.focus_failure_count += 1.0;
            if late {
                totals.late_focus_failure_count += 1.0;
            }
        }
        if let (Some(start), Some(end)) = (actual_start, actual_end) {
            totals.clean_focus_seconds += clean_segment_seconds(&segment.pause_log, start, end);
        }
    } else {
        if segment.status != "skipped" {
            totals.break_started_count += 1.0;
        }
        if segment.status == "completed" {
            totals.break_completed_count += 1.0;
        }
        if segment.status == "skipped" || segment.end_reason.as_deref() == Some("skipped_by_user") {
            totals.break_skipped_count += 1.0;
        }
        if let Some(end) = actual_end {
            let overtime = bounded_seconds(planned_end, end);
            if segment.phase == "short_break" {
                totals.short_break_overtime_seconds += overtime;
            } else {
                totals.long_break_overtime_seconds += overtime;
            }
        }
    }
    for pause in &segment.pause_log {
        apply_pause(totals, pause, actual_start, actual_end);
        if segment.phase == "focus" {
            apply_focus_idle(
                totals,
                pause,
                actual_start.unwrap_or(planned_start),
                actual_end.unwrap_or(planned_end),
            );
        }
    }
}

fn apply_pause(
    totals: &mut FeatureVector,
    pause: &PauseInput,
    actual_start: Option<i64>,
    actual_end: Option<i64>,
) {
    let start = parse_time_ms(&pause.started_at);
    let Some(end) = pause.ended_at.as_deref().map(parse_time_ms).or(actual_end) else {
        return;
    };
    let seconds = bounded_seconds(start.max(actual_start.unwrap_or(start)), end);
    match pause.reason.as_str() {
        "idle" => {
            totals.idle_pause_count += 1.0;
            totals.idle_pause_seconds += seconds;
        }
        "manual" => {
            totals.manual_pause_count += 1.0;
            totals.manual_pause_seconds += seconds;
        }
        _ => {
            totals.suspend_pause_count += 1.0;
            totals.suspend_pause_seconds += seconds;
        }
    }
}

fn apply_focus_idle(
    totals: &mut FeatureVector,
    pause: &PauseInput,
    focus_start: i64,
    focus_end: i64,
) {
    if pause.reason != "idle" || focus_end <= focus_start {
        return;
    }
    let pause_start = parse_time_ms(&pause.started_at);
    let pause_end = pause
        .ended_at
        .as_deref()
        .map(parse_time_ms)
        .unwrap_or(focus_end);
    let seconds = bounded_seconds(pause_start.max(focus_start), pause_end.min(focus_end));
    if seconds <= 0.0 {
        return;
    }
    totals.focus_idle_pause_count += 1.0;
    totals.focus_idle_pause_seconds += seconds;
    match focus_timing(pause_start, &[(focus_start, focus_end)]) {
        Some(Timing::Early) => totals.early_focus_idle_pause_count += 1.0,
        Some(Timing::Late) => totals.late_focus_idle_pause_count += 1.0,
        None => {}
    }
}

fn apply_event(
    totals: &mut FeatureVector,
    event: &RunEventInput,
    segments: &[SegmentInput],
    windows: &[(i64, i64)],
) {
    match event.event_type.as_str() {
        "extend_focus" => totals.extension_count += 1.0,
        "go_to_break_now" => {
            totals.go_to_break_now_count += 1.0;
            if event.phase.as_deref() == Some("focus") {
                apply_go_to_break_timing(totals, parse_time_ms(&event.occurred_at), windows);
            }
        }
        "start_focus_now" => {
            totals.start_focus_now_count += 1.0;
            if let Some(next) = next_focus(segments, parse_time_ms(&event.occurred_at)) {
                apply_early_break_outcome(totals, next);
            }
        }
        "skip_break" => totals.break_skipped_count += 1.0,
        "focus_failed" => totals.focus_failure_count += 1.0,
        "stop" => totals.stop_count += 1.0,
        "crash_recovery" => push_unique(&mut totals.data_quality_flags, "crash_recovered"),
        _ => {}
    }
}

fn apply_go_to_break_timing(totals: &mut FeatureVector, at_ms: i64, windows: &[(i64, i64)]) {
    match focus_timing(at_ms, windows) {
        Some(Timing::Early) => totals.early_go_to_break_now_count += 1.0,
        Some(Timing::Late) => totals.late_go_to_break_now_count += 1.0,
        None => {}
    }
}

fn apply_early_break_outcome(totals: &mut FeatureVector, next: &SegmentInput) {
    if next.status == "completed" {
        totals.start_focus_now_success_count += 1.0;
    } else if next.status == "interrupted" || next.end_reason.as_deref() == Some("focus_failed") {
        totals.start_focus_now_failure_count += 1.0;
    }
}

fn infer_manual_transitions(
    totals: &mut FeatureVector,
    segments: &[SegmentInput],
    events: &[RunEventInput],
    windows: &[(i64, i64)],
) {
    let mut ordered: Vec<&SegmentInput> = segments.iter().collect();
    ordered.sort_by_key(|segment| segment_start_ms(segment));
    for segment in &ordered {
        let Some(end) = segment.actual_end.as_deref().map(parse_time_ms) else {
            continue;
        };
        if parse_time_ms(&segment.planned_end).saturating_sub(end) < EARLY_MANUAL_END_TOLERANCE_MS {
            continue;
        }
        if segment.phase == "focus" {
            if segment.status != "completed" || segment.end_reason.as_deref() != Some("completed") {
                continue;
            }
            let next = ordered.iter().find(|next| segment_start_ms(next) >= end);
            if next.is_none_or(|next| !is_break(&next.phase))
                || explicit_event_near(events, "go_to_break_now", end)
            {
                continue;
            }
            totals.go_to_break_now_count += 1.0;
            apply_go_to_break_timing(totals, end, windows);
        } else if is_break(&segment.phase) {
            let manual = segment.status == "skipped"
                || segment.end_reason.as_deref() == Some("skipped_by_user")
                || (segment.status == "completed"
                    && segment.end_reason.as_deref() == Some("completed"));
            if !manual || explicit_event_near(events, "start_focus_now", end) {
                continue;
            }
            if let Some(next) = next_focus(segments, end) {
                totals.start_focus_now_count += 1.0;
                apply_early_break_outcome(totals, next);
            }
        }
    }
}

fn explicit_event_near(events: &[RunEventInput], event_type: &str, at_ms: i64) -> bool {
    events
        .iter()
        .filter(|event| event.event_type == event_type)
        .any(|event| {
            parse_time_ms(&event.occurred_at).abs_diff(at_ms)
                <= EXPLICIT_EVENT_MATCH_TOLERANCE_MS as u64
        })
}

fn apply_skipped_break_outcomes(
    totals: &mut FeatureVector,
    segments: &[SegmentInput],
    events: &[RunEventInput],
) {
    let mut markers = BTreeSet::new();
    for segment in segments.iter().filter(|segment| {
        is_break(&segment.phase)
            && (segment.status == "skipped"
                || segment.end_reason.as_deref() == Some("skipped_by_user"))
    }) {
        let at_ms = parse_time_ms(
            segment
                .actual_end
                .as_deref()
                .or(segment.actual_start.as_deref())
                .unwrap_or(&segment.planned_start),
        );
        if at_ms > 0 {
            markers.insert((at_ms, segment.phase.as_str()));
        }
    }
    for event in events
        .iter()
        .filter(|event| event.event_type == "skip_break")
    {
        if let Some(phase) = event.phase.as_deref().filter(|phase| is_break(phase)) {
            let at_ms = parse_time_ms(&event.occurred_at);
            if at_ms > 0 {
                markers.insert((at_ms, phase));
            }
        }
    }
    for (at_ms, phase) in markers {
        let Some(next) = next_focus(segments, at_ms) else {
            continue;
        };
        if next.status == "completed" {
            totals.skipped_break_next_focus_success_count += 1.0;
        } else if next.status == "interrupted" || next.end_reason.as_deref() == Some("focus_failed")
        {
            totals.skipped_break_next_focus_failure_count += 1.0;
            if phase == "short_break" {
                totals.skipped_short_break_next_focus_failure_count += 1.0;
            } else {
                totals.skipped_long_break_next_focus_failure_count += 1.0;
            }
        }
    }
}

fn next_focus(segments: &[SegmentInput], at_ms: i64) -> Option<&SegmentInput> {
    segments
        .iter()
        .filter(|segment| segment.phase == "focus" && segment_start_ms(segment) >= at_ms)
        .min_by_key(|segment| segment_start_ms(segment))
}

fn apply_block_events(
    totals: &mut FeatureVector,
    events: &[BlockEventInput],
    focus_windows: &[(i64, i64)],
    break_windows: &[(&str, i64, i64)],
) {
    let mut blocked: Vec<&BlockEventInput> = events
        .iter()
        .filter(|event| matches!(event.decision.as_str(), "blocked" | "limit_exhausted"))
        .collect();
    blocked.sort_by_key(|event| parse_time_ms(&event.occurred_at));
    totals.blocked_attempt_count += blocked.len() as f64;
    totals.repeated_blocked_source_attempt_count += repeated_source_attempt_count(&blocked);
    totals.focus_repeated_blocked_source_attempt_count += repeated_source_attempt_count(
        &blocked
            .iter()
            .copied()
            .filter(|event| event.phase.as_deref() == Some("focus"))
            .collect::<Vec<_>>(),
    );
    totals.break_repeated_blocked_source_attempt_count += repeated_source_attempt_count(
        &blocked
            .iter()
            .copied()
            .filter(|event| event.phase.as_deref().is_some_and(is_break))
            .collect::<Vec<_>>(),
    );
    for event in &blocked {
        let at_ms = parse_time_ms(&event.occurred_at);
        if event.phase.as_deref() == Some("focus") {
            totals.focus_blocked_attempt_count += 1.0;
            match focus_timing(at_ms, focus_windows) {
                Some(Timing::Early) => totals.early_focus_blocked_attempt_count += 1.0,
                Some(Timing::Late) => totals.late_focus_blocked_attempt_count += 1.0,
                None => {}
            }
        } else if event.phase.as_deref().is_some_and(is_break) {
            totals.break_blocked_attempt_count += 1.0;
            if let Some((phase, _, _)) = break_windows.iter().find(|(phase, planned, actual)| {
                Some(*phase) == event.phase.as_deref() && at_ms > *planned && at_ms <= *actual
            }) {
                totals.break_overtime_blocked_attempt_count += 1.0;
                if *phase == "short_break" {
                    totals.short_break_overtime_blocked_attempt_count += 1.0;
                } else {
                    totals.long_break_overtime_blocked_attempt_count += 1.0;
                }
            }
        }
    }
    let mut window_start = 0;
    for index in 0..blocked.len() {
        let at_ms = parse_time_ms(&blocked[index].occurred_at);
        while window_start < index
            && at_ms - parse_time_ms(&blocked[window_start].occurred_at) > BLOCK_BURST_WINDOW_MS
        {
            window_start += 1;
        }
        if index + 1 - window_start == BLOCK_BURST_MIN_ATTEMPTS {
            totals.blocked_burst_count += 1.0;
            window_start = index + 1;
        }
    }
}

fn repeated_source_attempt_count(events: &[&BlockEventInput]) -> f64 {
    let mut counts = BTreeMap::<String, usize>::new();
    for event in events {
        *counts
            .entry(format!("{}:{}", event.source_type, event.source_key))
            .or_default() += 1;
    }
    counts
        .values()
        .map(|count| count.saturating_sub(1) as f64)
        .sum()
}

enum Timing {
    Early,
    Late,
}
fn focus_timing(at_ms: i64, windows: &[(i64, i64)]) -> Option<Timing> {
    let (start, end) = *windows
        .iter()
        .find(|(start, end)| at_ms >= *start && at_ms <= *end)?;
    let duration = bounded_seconds(start, end);
    if duration <= 0.0 {
        return None;
    }
    let elapsed = bounded_seconds(start, at_ms);
    let remaining = bounded_seconds(at_ms, end);
    let ratio = elapsed / duration;
    let early = elapsed <= FOCUS_EDGE_SECONDS || ratio <= FOCUS_EDGE_RATIO;
    let late = remaining <= FOCUS_EDGE_SECONDS || ratio >= 1.0 - FOCUS_EDGE_RATIO;
    match (early, late) {
        (true, true) => Some(if elapsed <= remaining {
            Timing::Early
        } else {
            Timing::Late
        }),
        (true, false) => Some(Timing::Early),
        (false, true) => Some(Timing::Late),
        _ => None,
    }
}

pub(super) fn clean_segment_seconds(pauses: &[PauseInput], start_ms: i64, end_ms: i64) -> f64 {
    let paused: f64 = pauses
        .iter()
        .map(|pause| {
            bounded_seconds(
                parse_time_ms(&pause.started_at).max(start_ms),
                pause
                    .ended_at
                    .as_deref()
                    .map(parse_time_ms)
                    .unwrap_or(end_ms)
                    .min(end_ms),
            )
        })
        .sum();
    (bounded_seconds(start_ms, end_ms) - paused).max(0.0)
}
pub(super) fn segment_start_ms(segment: &SegmentInput) -> i64 {
    parse_time_ms(
        segment
            .actual_start
            .as_deref()
            .unwrap_or(&segment.planned_start),
    )
}
pub(super) fn parse_time_ms(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|value| value.timestamp_millis())
        .unwrap_or(0)
}
fn bounded_seconds(start: i64, end: i64) -> f64 {
    ((end.saturating_sub(start)) as f64 / 1000.0)
        .ceil()
        .max(0.0)
}
fn is_break(phase: &str) -> bool {
    matches!(phase, "short_break" | "long_break")
}
