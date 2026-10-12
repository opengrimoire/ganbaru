//! Evidence-bounded elapsed accounting with civil-midnight allocation.

use crate::contracts::{DistractionsUsageSampleInput, DistractionsUsageSampleRow};
use crate::usage;
use jiff::tz::TimeZone;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

const CLOCK_TOLERANCE_MS: i64 = 1_000;
const MAX_EVIDENCE_GAP_MS: u64 = 15_000;
const MAX_SOURCES: usize = 2_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageSource {
    pub key: String,
    pub label: String,
}

#[derive(Clone)]
pub struct Observation {
    pub wall_ms: i64,
    pub monotonic: Instant,
    pub sources: Vec<UsageSource>,
    pub zone: TimeZone,
}

#[derive(Default)]
pub struct Accounting {
    previous: Option<Observation>,
    fractions: HashMap<(String, String), u64>,
}

/// Divide elapsed evidence at actual local midnights, including gaps and offset changes.
fn split_interval(
    start: i64,
    end: i64,
    elapsed: u64,
    zone: &TimeZone,
) -> Result<Vec<(String, i64, u64)>, String> {
    let mut result = Vec::new();
    let mut cursor = start;
    let mut allocated = 0;
    while cursor < end {
        if result.len() >= 3 {
            return Err("desktop usage interval crosses too many local dates".into());
        }
        let local = ganbaru_civil_time::instant_to_local(cursor, zone)?;
        let next_date = local
            .date()
            .succ_opt()
            .ok_or("desktop usage date overflow")?;
        let midnight = next_date
            .and_hms_opt(0, 0, 0)
            .ok_or("desktop usage midnight is invalid")?;
        let boundary = ganbaru_civil_time::explicit_instant(midnight, zone)?.min(end);
        if boundary <= cursor {
            return Err("desktop usage midnight did not advance".into());
        }
        let cumulative = if boundary == end {
            elapsed
        } else {
            ((u128::from(elapsed) * (boundary - start) as u128) / (end - start) as u128) as u64
        };
        result.push((
            local.date().format("%Y-%m-%d").to_string(),
            cursor,
            cumulative - allocated,
        ));
        allocated = cumulative;
        cursor = boundary;
    }
    Ok(result)
}

impl Accounting {
    pub fn clear(&mut self) {
        self.previous = None;
        self.fractions.clear();
    }

    /// Consume each observation once. Long gaps and clock discontinuities create no usage.
    pub fn observe(
        &mut self,
        next: Observation,
        identity: &str,
    ) -> Result<Vec<DistractionsUsageSampleRow>, String> {
        if next.sources.len() > MAX_SOURCES {
            self.clear();
            return Err("desktop usage sources exceed their limit".into());
        }
        let previous = self.previous.replace(next.clone());
        let Some(previous) = previous else {
            return Ok(Vec::new());
        };
        let Some(duration) = next.monotonic.checked_duration_since(previous.monotonic) else {
            self.fractions.clear();
            return Ok(Vec::new());
        };
        let elapsed =
            u64::try_from(duration.as_millis()).map_err(|_| "desktop elapsed clock overflow")?;
        let wall_gap = next
            .wall_ms
            .checked_sub(previous.wall_ms)
            .ok_or("desktop wall clock overflow")?;
        if elapsed > MAX_EVIDENCE_GAP_MS
            || wall_gap <= 0
            || wall_gap.abs_diff(elapsed as i64) > CLOCK_TOLERANCE_MS as u64
        {
            self.fractions.clear();
            return Ok(Vec::new());
        }
        let windows = split_interval(previous.wall_ms, next.wall_ms, elapsed, &previous.zone)?;
        let mut samples = Vec::new();
        let mut seen = HashSet::new();
        for source in previous.sources {
            if !seen.insert(source.key.clone()) {
                continue;
            }
            for (date, started_at_ms, milliseconds) in &windows {
                let key = (source.key.clone(), date.clone());
                let total = self.fractions.get(&key).copied().unwrap_or(0) + milliseconds;
                self.fractions.insert(key, total % 1_000);
                if total < 1_000 {
                    continue;
                }
                let digest = Sha256::digest(
                    format!("{identity}|{}|{date}|{started_at_ms}", source.key).as_bytes(),
                );
                samples.push(usage::normalize_usage_sample(
                    DistractionsUsageSampleInput {
                        id: Some(format!("desktop-{:x}", digest)),
                        source_type: "desktop-app".into(),
                        source_key: source.key.clone(),
                        display_name: Some(source.label.clone()),
                        started_at_ms: *started_at_ms,
                        elapsed_seconds: (total / 1_000) as i64,
                        local_date: date.clone(),
                    },
                    "desktop",
                )?);
            }
        }
        let retained = next
            .sources
            .iter()
            .map(|source| source.key.as_str())
            .collect::<HashSet<_>>();
        let date = ganbaru_civil_time::instant_to_local(next.wall_ms, &next.zone)?
            .date()
            .format("%Y-%m-%d")
            .to_string();
        self.fractions
            .retain(|(source, day), _| retained.contains(source.as_str()) && *day == date);
        Ok(samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits;
    use std::time::Duration;

    fn observation(wall: &str, monotonic: Instant, zone: &str) -> Observation {
        Observation {
            wall_ms: wall.parse::<jiff::Timestamp>().unwrap().as_millisecond(),
            monotonic,
            zone: ganbaru_civil_time::zone(zone).unwrap(),
            sources: vec![UsageSource {
                key: "game".into(),
                label: "Game".into(),
            }],
        }
    }

    #[test]
    fn splits_midnight_and_week_boundaries_without_losing_elapsed_seconds() {
        let clock = Instant::now();
        let mut accounting = Accounting::default();
        accounting
            .observe(
                observation("2026-10-05T03:59:58Z", clock, "America/New_York"),
                "device",
            )
            .unwrap();
        let rows = accounting
            .observe(
                observation(
                    "2026-10-05T04:00:03Z",
                    clock + Duration::from_secs(5),
                    "America/New_York",
                ),
                "device",
            )
            .unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| (row.local_date.as_str(), row.elapsed_seconds))
                .collect::<Vec<_>>(),
            [("2026-10-04", 2), ("2026-10-05", 3)]
        );
        assert_ne!(rows[0].id, rows[1].id);
        assert_eq!(
            limits::week_start(&rows[1].local_date).unwrap(),
            "2026-10-05"
        );
    }

    #[test]
    fn skipped_dates_and_dst_changes_use_instant_duration() {
        let clock = Instant::now();
        for (start, end, zone, dates) in [
            (
                "2011-12-30T09:59:58Z",
                "2011-12-30T10:00:03Z",
                "Pacific/Apia",
                vec!["2011-12-29", "2011-12-31"],
            ),
            (
                "2024-11-03T05:59:58Z",
                "2024-11-03T06:00:03Z",
                "America/New_York",
                vec!["2024-11-03"],
            ),
            (
                "2024-03-10T06:59:58Z",
                "2024-03-10T07:00:03Z",
                "America/New_York",
                vec!["2024-03-10"],
            ),
        ] {
            let mut accounting = Accounting::default();
            accounting
                .observe(observation(start, clock, zone), "device")
                .unwrap();
            let rows = accounting
                .observe(
                    observation(end, clock + Duration::from_secs(5), zone),
                    "device",
                )
                .unwrap();
            assert_eq!(rows.iter().map(|row| row.elapsed_seconds).sum::<i64>(), 5);
            assert_eq!(
                rows.iter()
                    .map(|row| row.local_date.as_str())
                    .collect::<Vec<_>>(),
                dates
            );
        }
    }

    #[test]
    fn suspend_backward_clock_and_forward_clock_changes_do_not_fabricate_usage() {
        let clock = Instant::now();
        for (end, duration) in [
            ("2026-10-02T12:00:20Z", 20),
            ("2026-10-02T11:59:55Z", 5),
            ("2026-10-02T13:00:05Z", 5),
        ] {
            let mut accounting = Accounting::default();
            accounting
                .observe(observation("2026-10-02T12:00:00Z", clock, "UTC"), "device")
                .unwrap();
            assert!(
                accounting
                    .observe(
                        observation(end, clock + Duration::from_secs(duration), "UTC"),
                        "device"
                    )
                    .unwrap()
                    .is_empty()
            );
        }
    }

    #[test]
    fn fractional_evidence_accumulates_and_duplicate_process_sources_count_once() {
        let clock = Instant::now();
        let mut accounting = Accounting::default();
        let mut first = observation("2026-10-02T12:00:00Z", clock, "UTC");
        first.sources.push(first.sources[0].clone());
        accounting.observe(first, "device").unwrap();
        assert!(
            accounting
                .observe(
                    observation(
                        "2026-10-02T12:00:00.600Z",
                        clock + Duration::from_millis(600),
                        "UTC"
                    ),
                    "device"
                )
                .unwrap()
                .is_empty()
        );
        let rows = accounting
            .observe(
                observation(
                    "2026-10-02T12:00:01.200Z",
                    clock + Duration::from_millis(1200),
                    "UTC",
                ),
                "device",
            )
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].elapsed_seconds, 1);
        accounting.clear();
        assert!(
            accounting
                .observe(
                    observation(
                        "2026-10-02T12:00:10Z",
                        clock + Duration::from_secs(10),
                        "UTC"
                    ),
                    "new-vault"
                )
                .unwrap()
                .is_empty()
        );
    }
}
