use super::helpers::stale_event_error;
use super::*;

pub(super) struct GenerationEventSink {
    pub(super) thread_id: ChatThreadId,
    pub(super) generation: u64,
    pub(super) current_generation: Arc<AtomicU64>,
    pub(super) snapshot: Arc<Mutex<ThreadRuntimeSnapshot>>,
    pub(super) inner: Arc<dyn ProviderEventSink>,
}

impl ProviderEventSink for GenerationEventSink {
    fn emit<'a>(&'a self, event: CanonicalRuntimeEvent) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            if self.current_generation.load(Ordering::Acquire) != self.generation {
                self.inner
                    .emit(late_event_warning(
                        event,
                        &self.thread_id,
                        "inactive_session_generation",
                    ))
                    .await?;
                return Err(stale_event_error());
            }
            if event.thread_id != self.thread_id {
                self.inner
                    .emit(late_event_warning(
                        event,
                        &self.thread_id,
                        "mismatched_thread",
                    ))
                    .await?;
                return Err(stale_event_error());
            }
            if !event_matches_active_turn(&self.snapshot, &event) {
                self.inner
                    .emit(late_event_warning(
                        event,
                        &self.thread_id,
                        "settled_or_mismatched_turn",
                    ))
                    .await?;
                return Err(stale_event_error());
            }
            self.inner.emit(event.clone()).await?;
            if self.current_generation.load(Ordering::Acquire) == self.generation {
                update_snapshot_for_event(&self.snapshot, &event);
            }
            Ok(())
        })
    }

    fn flush(&self) -> DriverFuture<'_, ()> {
        self.inner.flush()
    }
}

pub(super) fn event_matches_active_turn(
    snapshot: &Arc<Mutex<ThreadRuntimeSnapshot>>,
    event: &CanonicalRuntimeEvent,
) -> bool {
    if event.turn_id.is_none()
        && matches!(
            &event.event,
            CanonicalEvent::ItemStarted(item)
                | CanonicalEvent::ItemUpdated(item)
                | CanonicalEvent::ItemCompleted(item)
                if item.kind == crate::models::CanonicalItemKind::ContextCompaction
        )
    {
        return true;
    }
    let requires_active_turn = matches!(
        event.event,
        CanonicalEvent::TurnStarted(_)
            | CanonicalEvent::TurnCompleted(_)
            | CanonicalEvent::TurnAborted(_)
            | CanonicalEvent::PlanUpdated(_)
            | CanonicalEvent::ProposedPlanDelta(_)
            | CanonicalEvent::ProposedPlanCompleted(_)
            | CanonicalEvent::DiffUpdated(_)
            | CanonicalEvent::ItemStarted(_)
            | CanonicalEvent::ItemUpdated(_)
            | CanonicalEvent::ItemCompleted(_)
            | CanonicalEvent::ContentDelta(_)
            | CanonicalEvent::RequestOpened(_)
            | CanonicalEvent::RequestResolved(_)
            | CanonicalEvent::UserInputRequested(_)
            | CanonicalEvent::UserInputResolved(_)
            | CanonicalEvent::TaskLifecycle(_)
            | CanonicalEvent::HookLifecycle(_)
            | CanonicalEvent::ToolProgress(_)
            | CanonicalEvent::FilesPersisted(_)
    );
    if !requires_active_turn {
        return true;
    }
    snapshot
        .lock()
        .map(|state| event.turn_id.as_ref() == state.active_turn_id.as_ref())
        .unwrap_or(false)
}

fn late_event_warning(
    mut event: CanonicalRuntimeEvent,
    thread_id: &ChatThreadId,
    reason: &'static str,
) -> CanonicalRuntimeEvent {
    event.thread_id = thread_id.clone();
    event.turn_id = None;
    event.provider_turn_id = None;
    event.provider_item_id = None;
    event.provider_request_id = None;
    event.provider_task_id = None;
    event.provider_reference = None;
    event.redacted_diagnostic = None;
    event.event = CanonicalEvent::RuntimeWarning(NotificationEvent {
        code: "late_provider_event".to_string(),
        title: "Late provider event ignored".to_string(),
        detail: Some(reason.to_string()),
    });
    event
}

fn update_snapshot_for_event(
    snapshot: &Arc<Mutex<ThreadRuntimeSnapshot>>,
    event: &CanonicalRuntimeEvent,
) {
    let Ok(mut state) = snapshot.lock() else {
        return;
    };
    state.last_activity = Instant::now();
    match &event.event {
        CanonicalEvent::SessionStarted(event) => {
            state.session_id = Some(event.session_id.clone());
            state.session_state = event.state;
            state.capabilities = event.capability_overrides.clone();
        }
        CanonicalEvent::SessionStateChanged(event) => state.session_state = event.state,
        CanonicalEvent::SessionExited(event) => {
            state.session_id = None;
            state.session_state = if event.expected {
                ProviderSessionState::Stopped
            } else {
                ProviderSessionState::Failed
            };
            state.active_turn_id = None;
            state.turn_active = false;
            state.pending_request = false;
            state.capabilities = crate::models::ProviderCapabilities::default();
        }
        CanonicalEvent::TurnStarted(_) => {
            state.active_turn_id = event.turn_id.clone();
            state.turn_active = true;
            state.session_state = ProviderSessionState::Active;
        }
        CanonicalEvent::TurnCompleted(_) | CanonicalEvent::TurnAborted(_) => {
            state.active_turn_id = None;
            state.turn_active = false;
            state.pending_request = false;
            state.session_state = ProviderSessionState::Ready;
        }
        CanonicalEvent::RequestOpened(_) => {
            state.pending_request = true;
            state.session_state = ProviderSessionState::WaitingForApproval;
        }
        CanonicalEvent::UserInputRequested(_) => {
            state.pending_request = true;
            state.session_state = ProviderSessionState::WaitingForUserInput;
        }
        CanonicalEvent::RequestResolved(_) | CanonicalEvent::UserInputResolved(_) => {
            state.pending_request = false;
            state.session_state = ProviderSessionState::Active;
        }
        CanonicalEvent::RuntimeError(event) if !event.recoverable => {
            state.session_state = ProviderSessionState::Failed;
        }
        _ => {}
    }
}
