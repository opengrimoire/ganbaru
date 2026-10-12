//! Desktop Calendar soundtrack boundaries run independently of mounted views.

use super::*;
use ganbaru_calendar::reads::native_window::{self, NativeCalendarWindow, WindowPurpose};
use ganbaru_calendar::recurrence::canonical::Window;
use ganbaru_civil_time as civil_time;
use std::time::Instant;

const CALENDAR_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const CALENDAR_EXPANSION_TIMEOUT: Duration = Duration::from_secs(10);
static CALENDAR_WORKER_GATE: std::sync::LazyLock<std::sync::Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(tokio::sync::Semaphore::new(1)));

pub(in super::super) struct CalendarActivationState {
    key: Option<String>,
    poll_at: Instant,
}

impl Default for CalendarActivationState {
    fn default() -> Self {
        Self {
            key: None,
            poll_at: Instant::now(),
        }
    }
}

impl CalendarActivationState {
    pub(in super::super) fn admit_context_load(&mut self) -> bool {
        if Instant::now() < self.poll_at {
            return false;
        }
        self.poll_at = Instant::now() + CALENDAR_REFRESH_INTERVAL;
        true
    }

    pub(in super::super) fn retry(&mut self) {
        self.key = None;
        self.poll_at = Instant::now();
    }
}

impl Owner {
    pub(in super::super) async fn reconcile_calendar_assignment(
        &mut self,
        action: Option<(&str, &str)>,
    ) -> MusicLibraryResult<()> {
        if Instant::now() < self.calendar.poll_at || self.state.owner == SessionOwner::Review {
            return Ok(());
        }
        self.calendar.poll_at = Instant::now() + CALENDAR_REFRESH_INTERVAL;
        // Resolve the local day before taking the shared write connection.
        let captured_now = now_ms();
        let window = tauri::async_runtime::spawn_blocking(move || {
            let zone = civil_time::system_zone()?;
            let date = civil_time::instant_to_local(captured_now, &zone)?
                .date()
                .to_string();
            Window::new(&date, &date, &zone)
        })
        .await
        .map_err(|error| MusicLibraryError::runtime("capture soundtrack day", error))?
        .map_err(|error| MusicLibraryError::runtime("resolve soundtrack timezone", error))?;
        // Most polls change nothing. Decide that from a released read snapshot so the
        // periodic check neither takes the vault write permit nor holds SQLite's write lock.
        if action.is_none() && !self.calendar_change_pending(&window, captured_now).await? {
            return Ok(());
        }
        let permit = self.write_permit().await?;
        let pool = self.pool.as_ref().expect("initialized music pool").clone();
        let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await.map_err(|error| {
            MusicLibraryError::database("begin Calendar soundtrack decision", error)
        })?;
        let open_focus: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pomodoro_runs WHERE ended_at IS NULL)")
                .fetch_one(&mut *transaction)
                .await
                .map_err(|error| {
                    MusicLibraryError::database("read soundtrack Focus priority", error)
                })?;
        if open_focus {
            self.calendar.key = None;
            if action.is_some() {
                return Err(MusicLibraryError::conflict(
                    "Focus superseded the Calendar soundtrack retry",
                ));
            }
            return Ok(());
        }
        let source = native_window::read(&mut transaction, &window, WindowPurpose::Music, false)
            .await
            .map_err(|error| {
                MusicLibraryError::runtime("read Calendar soundtrack window", error)
            })?;
        let projected = expand(source, window).await?;
        let (activation, boundary) = select(&projected, captured_now)?;
        self.accept_calendar_poll(boundary)?;
        let Some(activation) = activation else {
            self.calendar.key = None;
            if self.state.owner == SessionOwner::CalendarEvent && self.state.context.is_some() {
                let mut next = self.state.clone();
                next.context = None;
                next.owner = SessionOwner::Manual;
                let transition = next.apply(SessionIntent::Refresh, now_ms());
                persistence::commit_in_transaction(
                    &mut transaction,
                    &self.device_id,
                    &next,
                    &transition,
                    action,
                    now_ms(),
                )
                .await?;
                transaction.commit().await.map_err(|error| {
                    MusicLibraryError::database("release ended Calendar soundtrack", error)
                })?;
                self.install(next, transition).await?;
            }
            return Ok(());
        };
        if self.calendar.key.as_deref() == Some(&activation.key) {
            return Ok(());
        }
        let key = activation.key.clone();
        if !self
            .apply_context_assignment(activation, transaction, permit, action)
            .await?
        {
            return Ok(());
        }
        // Mark only successful acceptance; explicit failures retry on the bounded poll.
        self.calendar.key = Some(key);
        Ok(())
    }

    /// Report whether the locked decision could change the session.
    ///
    /// The snapshot is advisory: a pending change is decided again under the write
    /// permit and an immediate transaction, so a concurrent edit is never committed
    /// from this read.
    async fn calendar_change_pending(
        &mut self,
        window: &Window,
        now: i64,
    ) -> MusicLibraryResult<bool> {
        let pool = self.pool.as_ref().expect("initialized music pool").clone();
        let mut transaction = pool.begin().await.map_err(|error| {
            MusicLibraryError::database("begin Calendar soundtrack snapshot", error)
        })?;
        let open_focus: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pomodoro_runs WHERE ended_at IS NULL)")
                .fetch_one(&mut *transaction)
                .await
                .map_err(|error| {
                    MusicLibraryError::database("read soundtrack Focus priority", error)
                })?;
        if open_focus {
            self.calendar.key = None;
            return Ok(false);
        }
        let source = native_window::read(&mut transaction, window, WindowPurpose::Music, false)
            .await
            .map_err(|error| {
                MusicLibraryError::runtime("read Calendar soundtrack window", error)
            })?;
        // Expansion is CPU work; release the shared connection before it starts.
        drop(transaction);
        let projected = expand(source, window.clone()).await?;
        let (activation, boundary) = select(&projected, now)?;
        let pending = match &activation {
            Some(activation) => self.calendar.key.as_deref() != Some(&activation.key),
            None => self.state.owner == SessionOwner::CalendarEvent && self.state.context.is_some(),
        };
        if !pending {
            if activation.is_none() {
                self.calendar.key = None;
            }
            self.accept_calendar_poll(boundary)?;
        }
        Ok(pending)
    }

    /// Clear a recovered Calendar failure and schedule the next poll before a known boundary.
    fn accept_calendar_poll(&mut self, boundary: Option<i64>) -> MusicLibraryResult<()> {
        if matches!(
            self.state.context_error,
            Some(super::super::super::policy::ContextFailure::Calendar(_))
        ) {
            self.state.context_error = None;
            self.publish(false)?;
        }
        if let Some(boundary) = boundary {
            let remaining = boundary.saturating_sub(now_ms()).max(1) as u64;
            self.calendar.poll_at =
                Instant::now() + CALENDAR_REFRESH_INTERVAL.min(Duration::from_millis(remaining));
        }
        Ok(())
    }
}

/// Expand one Calendar source on a bounded worker that must drain before the next expansion.
async fn expand(
    source: native_window::WindowSource,
    window: Window,
) -> MusicLibraryResult<NativeCalendarWindow> {
    let worker_permit = CALENDAR_WORKER_GATE
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            MusicLibraryError::conflict(
                "An earlier Calendar soundtrack expansion is still draining",
            )
        })?;
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let _permit = worker_permit;
        source.expand(&window)
    });
    tokio::time::timeout(CALENDAR_EXPANSION_TIMEOUT, worker)
        .await
        .map_err(|_| {
            MusicLibraryError::runtime(
                "Calendar soundtrack expansion",
                "native worker timed out; it must drain before another expansion",
            )
        })?
        .map_err(|error| MusicLibraryError::runtime("expand Calendar soundtrack window", error))?
        .map_err(|error| {
            MusicLibraryError::runtime("resolve Calendar soundtrack occurrences", error)
        })
}

/// Choose a deterministic timed winner and the next known boundary in canonical instants.
fn select(
    window: &NativeCalendarWindow,
    now: i64,
) -> MusicLibraryResult<(Option<ContextActivation>, Option<i64>)> {
    let mut winner: Option<(
        &ganbaru_calendar::reads::DbCalendarEventRow,
        &native_window::NativeOccurrence,
        i64,
        i64,
    )> = None;
    let mut boundary: Option<i64> = None;
    let events: BTreeMap<_, _> = window
        .events
        .iter()
        .map(|event| (event.id.as_str(), event))
        .collect();
    for occurrence in &window.occurrences {
        let Some(event) = events.get(occurrence.template_id.as_str()).copied() else {
            return Err(MusicLibraryError::conflict(
                "Calendar soundtrack occurrence has no canonical source",
            ));
        };
        if event.all_day != 0 || event.rhythm_kind.is_some() || event.status == "cancelled" {
            continue;
        }
        let instant = |label: &str| {
            chrono::DateTime::parse_from_rfc3339(label)
                .map(|value| value.timestamp_millis())
                .map_err(|error| {
                    MusicLibraryError::runtime("parse canonical soundtrack instant", error)
                })
        };
        let start = instant(&occurrence.start_time)?;
        let end = instant(&occurrence.end_time)?;
        for value in [start, end] {
            if value > now {
                boundary = Some(boundary.map_or(value, |prior| prior.min(value)));
            }
        }
        if now < start || now >= end {
            continue;
        }
        let replaces = winner
            .as_ref()
            .is_none_or(|(prior, prior_occurrence, _, prior_end)| {
                (end, &event.created_at, &occurrence.id)
                    < (*prior_end, &prior.created_at, &prior_occurrence.id)
            });
        if replaces {
            winner = Some((event, occurrence, start, end));
        }
    }
    Ok((
        winner.map(|(event, occurrence, start_ms, end_ms)| ContextActivation {
            event_id: event.id.clone(),
            event_title: occurrence
                .override_id
                .as_ref()
                .and_then(|id| window.overrides.iter().find(|row| &row.id == id))
                .and_then(|row| row.title.clone())
                .unwrap_or_else(|| event.title.clone()),
            key: format!("calendar:{}:{}", occurrence.id, occurrence.start_time),
            phase: MusicActivityPhase::Focus,
            authority: AssignmentAuthority::Calendar { start_ms, end_ms },
        }),
        boundary,
    ))
}

#[cfg(test)]
mod tests;
