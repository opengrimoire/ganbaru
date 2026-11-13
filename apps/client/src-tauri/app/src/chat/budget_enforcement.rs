//! Budget enforcement on the device that executes Chat work.
//!
//! A provider event that reaches a budget limit fails its assignment in the event's transaction
//! and queues the assignment here, so its provider session stops after commit. Working time
//! passes without provider events, so the loop also checks the budgets of this device's work
//! every [`BUDGET_TICK_SECONDS`].

use ganbaru_chat::coordination::budgets::{BUDGET_TICK_SECONDS, check_local_budgets};
use ganbaru_chat_contracts::models::{ChatProjectionKey, ChatThreadId, ChatWorkAssignmentId};
use sqlx::SqlitePool;
use std::sync::{Mutex, PoisonError};
use tauri::Manager;

/// Command id prefix of the session stop that follows a budget stop.
const BUDGET_STOP_LABEL: &str = "assignment-budget-stop";

/// Assignments stopped at a budget limit whose provider sessions still have to stop.
#[derive(Default)]
pub(crate) struct BudgetStops {
    queued: Mutex<Vec<ChatWorkAssignmentId>>,
    wake: tokio::sync::Notify,
}

impl BudgetStops {
    fn queue(&self, assignment_id: ChatWorkAssignmentId) {
        self.queued
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(assignment_id);
        self.wake.notify_one();
    }

    fn take(&self) -> Vec<ChatWorkAssignmentId> {
        std::mem::take(&mut *self.queued.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// Queues the session stop of `assignment_id`, which a committed provider event stopped at its
/// budget limit. Does nothing before the loop starts.
pub(crate) fn queue_stop<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    assignment_id: &ChatWorkAssignmentId,
) {
    if let Some(stops) = app.try_state::<BudgetStops>() {
        stops.queue(assignment_id.clone());
    }
}

/// Starts the enforcement loop: a queued stop ends its session right away, and every tick
/// checks the budgets of the work this device runs.
pub(crate) fn start(app: &tauri::AppHandle) {
    app.manage(BudgetStops::default());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(BUDGET_TICK_SECONDS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let stops = app.state::<BudgetStops>();
        loop {
            let tick = tokio::select! {
                _ = interval.tick() => true,
                () = stops.wake.notified() => false,
            };
            enforce_active_vault(&app, stops.take(), tick).await;
        }
    });
}

/// Ends the sessions of `queued` stops and, on a `tick`, checks local budgets, logging failures
/// by code only.
async fn enforce_active_vault(
    app: &tauri::AppHandle,
    queued: Vec<ChatWorkAssignmentId>,
    tick: bool,
) {
    if crate::vault::active_vault_path(app).is_err() {
        return;
    }
    let db_url = format!("sqlite:{}", crate::vault::APP_SQLITE_FILE);
    let pool = match crate::db::connect_sqlite(app.clone(), db_url.clone()).await {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("Chat budget enforcement could not open the database: {error}");
            return;
        }
    };
    // A stop from a provider event already announced its rows with the event.
    for assignment_id in queued {
        end_stopped_work(app, &db_url, &pool, &assignment_id).await;
    }
    if !tick {
        return;
    }
    let stopped = match super::command_support::now_timestamp() {
        Ok(now) => check_local_budgets(&pool, now.as_str()).await,
        Err(error) => Err(error),
    };
    match stopped {
        Ok(stopped) => {
            for assignment_id in stopped {
                announce_stop(app, &pool, &assignment_id).await;
                end_stopped_work(app, &db_url, &pool, &assignment_id).await;
            }
        }
        Err(error) => eprintln!("Chat budget check failed with code {:?}", error.code),
    }
}

async fn end_stopped_work(
    app: &tauri::AppHandle,
    db_url: &str,
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) {
    if let Err(error) =
        super::organization::end_settled_work(app, db_url, pool, assignment_id, BUDGET_STOP_LABEL)
            .await
    {
        eprintln!("Chat budget stop failed with code {:?}", error.code);
    }
}

/// Tells views about a stop the timer made, which no provider event announced, and wakes the
/// usage summaries for the run that stopped working.
async fn announce_stop(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) {
    super::usage_summaries::wake(app);
    let thread_id: Option<String> = match sqlx::query_scalar(
        "SELECT provider_thread_id FROM chat_agent_runs
         WHERE assignment_id = ? AND provider_thread_id IS NOT NULL
         ORDER BY run_ordinal DESC LIMIT 1",
    )
    .bind(assignment_id.as_str())
    .fetch_optional(pool)
    .await
    {
        Ok(thread_id) => thread_id.flatten(),
        Err(_) => {
            eprintln!("Chat budget stop could not read the stopped run");
            return;
        }
    };
    let Some(thread_id) = thread_id.and_then(|thread_id| ChatThreadId::new(thread_id).ok()) else {
        return;
    };
    super::ingestion::notify_rows_changed(
        app,
        pool,
        &thread_id,
        &[ChatProjectionKey::Organizational],
    )
    .await;
}
