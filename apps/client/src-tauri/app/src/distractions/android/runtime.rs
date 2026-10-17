//! One native Android shared-accounting publisher above the independent Guardian observer.

use super::{
    GUARDIAN_SYNC, MAX_DRAIN_BATCHES,
    projection::{GuardianCapture, NotificationCopy},
    synchronize_locked,
};
use crate::{db::connect_sqlite, vault};
use ganbaru_mobile_distractions::MobileDistractionsExt;
use serde_json::Value;
use std::{
    io::Read,
    path::Path,
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
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

struct Frame {
    generation: u64,
    active: bool,
    copy: Option<NotificationCopy>,
    view: Option<(Instant, Value)>,
}

struct Control {
    frame: Mutex<Frame>,
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

fn read_config(path: &Path) -> Result<Value, String> {
    if !path
        .try_exists()
        .map_err(|error| format!("inspect Guardian config: {error}"))?
    {
        return Ok(serde_json::json!({}));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|error| format!("open Guardian config: {error}"))?
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read Guardian config: {error}"))?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err("Guardian configuration exceeds its byte limit".into());
    }
    let root: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("decode Guardian config: {error}"))?;
    if !root.is_object() {
        return Err("Guardian configuration root must be an object".into());
    }
    Ok(root)
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

/// Freeze native publication and revoke Guardian rules before a vault pointer or ownership change.
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

async fn refresh<R: Runtime>(
    app: &tauri::AppHandle<R>,
    control: &Arc<Control>,
    generation: u64,
) -> Result<(), String> {
    let _sync = GUARDIAN_SYNC.lock().await;
    for _ in 0..MAX_DRAIN_BATCHES {
        match synchronize_locked(app.clone()).await {
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
    let status = app
        .state::<vault::ownership::VaultOwnershipManager>()
        .status(&vault_id)?;
    let capture_app = app.clone();
    let capture_vault = vault_id.clone();
    let config_path = vault::config_path(&path);
    let read_path = config_path.clone();
    let (capture, root) = tauri::async_runtime::spawn_blocking(move || {
        let encoded = capture_app
            .mobile_distractions()
            .accounting_snapshot(&capture_vault)?;
        let capture = GuardianCapture::decode(
            &encoded,
            &capture_vault,
            jiff::Timestamp::now().as_millisecond(),
        )?;
        Ok::<_, String>((capture, read_config(&read_path)?))
    })
    .await
    .map_err(|error| format!("Guardian capture worker: {error}"))??;
    let copy = current_frame(control)?
        .copy
        .clone()
        .or_else(|| capture.copy.clone())
        .ok_or("Guardian notification language is not ready")?;
    let sources = if status.can_write {
        let _permit = app
            .state::<vault::ownership::VaultOwnershipManager>()
            .acquire_managed_write(&vault_id)?;
        if vault::active_vault_path(app)? != path {
            return Err("Guardian vault changed before accounting".into());
        }
        let pool = connect_sqlite(app.clone(), super::ACTIVE_DB_URL.into()).await?;
        let mut tx = pool
            .begin()
            .await
            .map_err(|error| format!("begin Guardian native accounting: {error}"))?;
        let sources = crate::distractions::limits::store::read_source_days(
            &mut tx,
            &capture.week_start_local_date,
            &capture.local_date,
        )
        .await?;
        // Retained peer receipts still cover Guardian identities after this device becomes the writer.
        let retained = crate::distractions::linked::guardian_acknowledgements(
            app,
            &vault_id,
            &status.device_id,
        )
        .await?;
        let already_imported = super::canonical_pending_ids(&mut tx, &capture, retained).await?;
        let sources = super::projection::include_pending(sources, &capture, &already_imported)?;
        tx.commit()
            .await
            .map_err(|error| format!("finish Guardian native accounting: {error}"))?;
        sources
    } else {
        let sources = crate::distractions::linked::accounting_source_days(
            app,
            &vault_id,
            &status.device_id,
            &capture.week_start_local_date,
            &capture.local_date,
        )
        .await?;
        let accepted = crate::distractions::linked::guardian_acknowledgements(
            app,
            &vault_id,
            &status.device_id,
        )
        .await?
        .into_iter()
        .collect();
        super::projection::include_pending(sources, &capture, &accepted)?
    };
    // The pure engine enforces the complete bounded source window, including newly pending rows.
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
            || publication_app
                .state::<vault::ownership::VaultOwnershipManager>()
                .status(&vault_id)?
                != status
            || read_config(&config_path)?.get("distractions") != root.get("distractions")
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
