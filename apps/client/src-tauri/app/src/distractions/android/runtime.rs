//! One native Android shared-accounting publisher above the independent Guardian observer.

use super::{
    GUARDIAN_SYNC, MAX_DRAIN_BATCHES,
    projection::{GuardianCapture, LastGoodUsage, NotificationCopy, fallback_sources},
    synchronize_locked,
};
use crate::{distractions::limits, vault};
use ganbaru_mobile_distractions::MobileDistractionsExt;
use serde_json::Value;
use std::{
    sync::{
        Arc, Mutex, MutexGuard, TryLockError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{Manager, Runtime};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const MAX_VIEW_AGE: Duration = Duration::from_secs(10);
const DRAIN_WAIT: Duration = Duration::from_secs(5);

struct Frame {
    generation: u64,
    active: bool,
    copy: Option<NotificationCopy>,
    view: Option<(Instant, Value)>,
}

#[derive(Default)]
struct UsageReadState {
    last_good: Option<LastGoodUsage>,
    diagnostics: limits::UsageReadDiagnostics,
}

struct Control {
    frame: Mutex<Frame>,
    usage: Mutex<UsageReadState>,
    wake: tokio::sync::Notify,
    stopped: AtomicBool,
}

#[derive(Clone)]
struct AndroidUsageState(Arc<Control>);

fn control<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<Arc<Control>> {
    app.try_state::<AndroidUsageState>()
        .map(|state| Arc::clone(&state.0))
}

/// Avoid waiting for a Guardian IPC call while reading, editing, or invalidating native context.
fn current_frame(control: &Control) -> Result<MutexGuard<'_, Frame>, String> {
    control.frame.try_lock().map_err(|error| match error {
        TryLockError::WouldBlock => "Guardian publication is busy; retry this operation".into(),
        TryLockError::Poisoned(_) => "Guardian publication control is unavailable".into(),
    })
}

fn revise(frame: &mut Frame) -> Result<(), String> {
    frame.generation = frame
        .generation
        .checked_add(1)
        .ok_or("Guardian publication generation exhausted")?;
    frame.view = None;
    Ok(())
}

/// Start one serialized native loop. Guardian continues observing when the application process is absent.
pub(crate) fn setup<R: Runtime>(app: &tauri::AppHandle<R>) {
    let control = Arc::new(Control {
        frame: Mutex::new(Frame {
            generation: 1,
            active: true,
            copy: None,
            view: None,
        }),
        usage: Mutex::new(UsageReadState::default()),
        wake: tokio::sync::Notify::new(),
        stopped: AtomicBool::new(false),
    });
    app.manage(AndroidUsageState(Arc::clone(&control)));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut previous_error = None;
        loop {
            if control.stopped.load(Ordering::Acquire) {
                return;
            }
            let context = match control.frame.try_lock() {
                Ok(frame) => Some((frame.generation, frame.active)),
                Err(TryLockError::WouldBlock) => None,
                Err(TryLockError::Poisoned(_)) => {
                    eprintln!("Guardian publication control is unavailable");
                    return;
                }
            };
            let Some((generation, active)) = context else {
                tokio::select! { _ = control.wake.notified() => {}, _ = tokio::time::sleep(REFRESH_INTERVAL) => {} }
                continue;
            };
            if active {
                let result = refresh(&app, &control, generation).await;
                if let Err(error) = result {
                    if previous_error.as_ref() != Some(&error) {
                        eprintln!("native Guardian publication deferred: {error}");
                    }
                    previous_error = Some(error);
                    if let Err(error) = revoke_failed_publication(&app, &control, generation).await
                    {
                        eprintln!("native Guardian publication could not be revoked: {error}");
                    }
                } else {
                    previous_error = None;
                }
            }
            tokio::select! { _ = control.wake.notified() => {}, _ = tokio::time::sleep(REFRESH_INTERVAL) => {} }
        }
    });
}

/// Revoke a failed generation without clearing a newer policy or the stopped owner's background rules.
async fn revoke_failed_publication<R: Runtime>(
    app: &tauri::AppHandle<R>,
    control: &Arc<Control>,
    generation: u64,
) -> Result<(), String> {
    let app = app.clone();
    let control = Arc::clone(control);
    tauri::async_runtime::spawn_blocking(move || {
        let mut frame = current_frame(&control)?;
        if frame.generation == generation
            && frame.active
            && !control.stopped.load(Ordering::Acquire)
        {
            frame.view = None;
            app.mobile_distractions().invalidate_rules()?;
        }
        Ok(())
    })
    .await
    .map_err(|error| format!("Guardian invalidation worker: {error}"))?
}

/// Native lifecycle and persisted configuration changes wake the existing owner.
pub(crate) fn wake<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(control) = control(app) {
        control.wake.notify_one();
    }
}

/// Revoke old publication and Guardian enforcement before a configuration write can become visible.
pub(crate) fn commit_configuration<R: Runtime>(
    app: &tauri::AppHandle<R>,
    write: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let Some(control) = control(app) else {
        return write();
    };
    let mut frame = current_frame(&control)?;
    revise(&mut frame)?;
    let result = app
        .mobile_distractions()
        .invalidate_rules()
        .and_then(|()| write());
    drop(frame);
    control.wake.notify_one();
    result
}

/// Freeze native publication and revoke Guardian rules before a vault pointer change.
pub(crate) fn freeze_for_selection<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<(), String> {
    let Some(control) = control(app) else {
        return Ok(());
    };
    let mut frame = current_frame(&control)?;
    revise(&mut frame)?;
    let previously_active = frame.active;
    frame.active = false;
    let result = app.mobile_distractions().invalidate_rules();
    if result.is_err() {
        frame.active = previously_active;
        drop(frame);
        control.wake.notify_one();
    }
    result
}

/// Wait for the sole capture/import operation before SQLite is fenced for handoff.
pub(crate) async fn stop_for_vault_handoff<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    let freeze_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || freeze_for_selection(&freeze_app))
        .await
        .map_err(|error| format!("Guardian handoff invalidation worker: {error}"))??;
    let guard = tokio::time::timeout(DRAIN_WAIT, GUARDIAN_SYNC.lock())
        .await
        .map_err(|_| "Guardian accounting did not drain before vault handoff")?;
    drop(guard);
    Ok(())
}

/// Reactivate from the newly selected native context after database/write fences release.
pub(crate) fn resume_after_vault_handoff<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    let Some(control) = control(app) else {
        return Ok(());
    };
    let mut frame = current_frame(&control)?;
    if !control.stopped.load(Ordering::Acquire) {
        revise(&mut frame)?;
        frame.active = true;
    }
    drop(frame);
    control.wake.notify_one();
    Ok(())
}

/// Stop application publication while retaining Guardian's independent background policy.
pub(crate) fn stop<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(control) = control(app) {
        control.stopped.store(true, Ordering::Release);
        if let Ok(mut frame) = current_frame(&control) {
            frame.active = false;
            frame.view = None;
        }
        control.wake.notify_one();
    }
}

/// Accept localized notification copy without accepting frontend rules, totals, clocks, or vault IDs.
pub(super) fn update_copy<R: Runtime>(
    app: tauri::AppHandle<R>,
    copy: NotificationCopy,
) -> Result<(), String> {
    copy.validate()?;
    let control = control(&app).ok_or("native Guardian publisher is unavailable")?;
    let mut frame = current_frame(&control)?;
    if frame.copy.as_ref() != Some(&copy) {
        revise(&mut frame)?;
        frame.copy = Some(copy);
    }
    drop(frame);
    control.wake.notify_one();
    Ok(())
}

/// Read only the last native projection; presentation cannot schedule policy calculation or enforcement.
pub(super) fn usage_projection<R: Runtime>(app: tauri::AppHandle<R>) -> Result<Value, String> {
    let control = control(&app).ok_or("native Guardian publisher is unavailable")?;
    let frame = current_frame(&control)?;
    if !frame.active || control.stopped.load(Ordering::Acquire) {
        return Err("native Guardian publisher is frozen".into());
    }
    let (at, view) = frame
        .view
        .as_ref()
        .filter(|(at, _)| at.elapsed() <= MAX_VIEW_AGE)
        .ok_or("native Guardian usage is not ready or has expired")?;
    let _ = at;
    if view["vaultId"].as_str() != Some(vault::active_vault_id(&app)?.as_str()) {
        return Err("native Guardian usage belongs to another vault".into());
    }
    Ok(view.clone())
}

/// Reads the budget sources and proves they can be totaled, so a window the engine would refuse
/// takes the same fallback as a failed read.
async fn read_usage_window(
    pool: &sqlx::SqlitePool,
    device_id: &str,
    capture: &GuardianCapture,
    config: limits::LimitsConfig,
) -> Result<limits::UsageWindow, String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin Guardian native accounting: {error}"))?;
    let scope = limits::UsageBudgetScope::from_rules(&config);
    let read = limits::store::read_source_days(
        &mut tx,
        device_id,
        &capture.week_start_local_date,
        &capture.local_date,
        &scope,
    )
    .await?;
    let already_imported = super::canonical_pending_ids(&mut tx, capture).await?;
    let merged = super::projection::include_pending(read.sources, capture, &already_imported)?;
    tx.commit()
        .await
        .map_err(|error| format!("finish Guardian native accounting: {error}"))?;
    let mut window = limits::UsageWindow {
        truncated: read.truncated || merged.truncated,
        sources: merged.sources,
    };
    let local_date = capture.local_date.clone();
    tauri::async_runtime::spawn_blocking(move || {
        window.truncated |= limits::totals(&config, &window.sources, &local_date)?.truncated;
        Ok(window)
    })
    .await
    .map_err(|error| format!("Guardian accounting worker: {error}"))?
}

/// Chooses the sources to publish so a failed usage read keeps Guardian's rules enforcing.
fn settle_usage_read(
    control: &Control,
    read: Result<limits::UsageWindow, String>,
    capture: &GuardianCapture,
    generation: u64,
    rules_revision: u64,
) -> Result<Vec<limits::UsageSourceDay>, String> {
    let mut usage = control
        .usage
        .lock()
        .map_err(|_| "Guardian usage state is unavailable")?;
    match read {
        Ok(window) => {
            usage.diagnostics.record_success();
            if window.truncated && usage.diagnostics.record_truncation(&capture.local_date) {
                eprintln!("Guardian usage window truncated for {}", capture.local_date);
            }
            usage.last_good = Some(LastGoodUsage {
                generation,
                vault_id: capture.vault_id.clone(),
                rules_revision,
                local_date: capture.local_date.clone(),
                sources: window.sources.clone(),
                local_usage: capture.local_usage(),
            });
            Ok(window.sources)
        }
        Err(error) => {
            if usage.diagnostics.record_failure(&error) {
                eprintln!("Guardian usage read failed, keeping rules: {error}");
            }
            Ok(fallback_sources(
                usage.last_good.as_ref(),
                capture,
                generation,
                rules_revision,
            ))
        }
    }
}

async fn refresh<R: Runtime>(
    app: &tauri::AppHandle<R>,
    control: &Arc<Control>,
    generation: u64,
) -> Result<(), String> {
    let _sync = GUARDIAN_SYNC.lock().await;
    for _ in 0..MAX_DRAIN_BATCHES {
        match synchronize_locked(app.clone(), false).await {
            Ok(result) if result.full_batch => continue,
            Ok(_) => break,
            Err(error) => {
                eprintln!("Guardian journal synchronization deferred: {error}");
                break;
            }
        }
    }
    let vault_id = vault::active_vault_id(app)?;
    let path = vault::active_vault_path(app)?;
    let device_id = vault::ensure_device_id(app)?;
    let rules = crate::distractions::rule_store::snapshot(app).await?;
    if rules.vault_id != vault_id {
        return Err("Guardian vault changed while its rules were read".into());
    }
    let rules_revision = rules.revision;
    let root = rules.root;
    let capture_app = app.clone();
    let capture_vault = vault_id.clone();
    let capture = tauri::async_runtime::spawn_blocking(move || {
        let encoded = capture_app
            .mobile_distractions()
            .accounting_snapshot(&capture_vault)?;
        let capture = GuardianCapture::decode(
            &encoded,
            &capture_vault,
            jiff::Timestamp::now().as_millisecond(),
        )?;
        Ok::<_, String>(capture)
    })
    .await
    .map_err(|error| format!("Guardian capture worker: {error}"))??;
    let copy = current_frame(control)?
        .copy
        .clone()
        .or_else(|| capture.copy.clone())
        .ok_or("Guardian notification language is not ready")?;
    let config = crate::distractions::limits_or_phase_only(&root);
    // Other devices' replicated totals, this device's own journal, and Guardian events not yet
    // imported form one bounded source window; a read needs no write permit.
    let pool = crate::db::connect_vault(app).await?;
    if vault::active_vault_path(app)? != path {
        return Err("Guardian vault changed before accounting".into());
    }
    let read = read_usage_window(&pool, &device_id, &capture, config).await;
    let sources = settle_usage_read(control, read, &capture, generation, rules_revision)?;
    // The pure engine enforces the bounded source window, including newly pending rows.
    let publication_app = app.clone();
    let publication_control = Arc::clone(control);
    tauri::async_runtime::spawn_blocking(move || {
        let revision = format!("{}-{generation}", capture.observed_at_epoch_ms);
        let (rules, view) = super::projection::build(&root, &capture, &sources, &copy, &revision)?;
        let mut frame = current_frame(&publication_control)?;
        if !frame.active
            || publication_control.stopped.load(Ordering::Acquire)
            || frame.generation != generation
            || vault::active_vault_path(&publication_app)? != path
            || vault::active_vault_id(&publication_app)? != vault_id
            || !crate::distractions::rule_store::is_current(rules_revision)
        {
            return Err("Guardian publication context changed during accounting".into());
        }
        capture.ensure_current(&vault_id, jiff::Timestamp::now().as_millisecond())?;
        let encoded = serde_json::to_string(&rules).map_err(|error| error.to_string())?;
        publication_app
            .mobile_distractions()
            .apply_rules(&encoded)?;
        frame.view = Some((Instant::now(), view));
        Ok(())
    })
    .await
    .map_err(|error| format!("Guardian publication worker: {error}"))?
}
