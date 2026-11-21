use super::helpers::stale_event_error;
use super::*;

/// Forwards one provider session's events to the durable sink and applies them to the runtime
/// phase. Events from an earlier session, another thread, or a turn the phase no longer holds
/// are recorded as late warnings instead.
pub(super) struct GenerationEventSink {
    pub(super) thread_id: ChatThreadId,
    pub(super) generation: u64,
    pub(super) snapshot: Arc<Mutex<ThreadRuntimeSnapshot>>,
    pub(super) inner: Arc<dyn ProviderEventSink>,
}

impl ProviderEventSink for GenerationEventSink {
    fn emit<'a>(&'a self, event: CanonicalRuntimeEvent) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            if let Some(reason) = self.late_reason(&event) {
                self.inner
                    .emit(late_event_warning(event, &self.thread_id, reason))
                    .await?;
                return Err(stale_event_error());
            }
            self.inner.emit(event.clone()).await?;
            // The generation is checked again in the same critical section that applies the
            // event, so a session that ended while the event was persisted is never changed.
            if let Ok(mut state) = self.snapshot.lock()
                && state.generation == self.generation
            {
                apply_event(&mut state, &event);
            }
            Ok(())
        })
    }

    fn flush(&self) -> DriverFuture<'_, ()> {
        self.inner.flush()
    }
}

impl GenerationEventSink {
    fn late_reason(&self, event: &CanonicalRuntimeEvent) -> Option<&'static str> {
        let Ok(state) = self.snapshot.lock() else {
            return Some("inactive_session_generation");
        };
        if state.generation != self.generation {
            Some("inactive_session_generation")
        } else if event.thread_id != self.thread_id {
            Some("mismatched_thread")
        } else if !event_matches_turn(&state, event) {
            Some("settled_or_mismatched_turn")
        } else {
            None
        }
    }
}

#[cfg(test)]
pub(super) fn event_matches_active_turn(
    snapshot: &Arc<Mutex<ThreadRuntimeSnapshot>>,
    event: &CanonicalRuntimeEvent,
) -> bool {
    snapshot
        .lock()
        .is_ok_and(|state| event_matches_turn(&state, event))
}

fn event_matches_turn(state: &ThreadRuntimeSnapshot, event: &CanonicalRuntimeEvent) -> bool {
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
    !requires_active_turn || event.turn_id.as_ref() == state.active_turn_id()
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

/// Applies a persisted provider event to the snapshot.
fn apply_event(state: &mut ThreadRuntimeSnapshot, event: &CanonicalRuntimeEvent) {
    state.last_activity = Instant::now();
    if let Some(input) = phase_input(event)
        && let Err(rejection) = state.apply(input)
    {
        eprintln!(
            "Chat runtime ignored a provider {} event while the session was {:?}",
            rejection.input, rejection.from
        );
    }
    match &event.event {
        CanonicalEvent::SessionStarted(started) => {
            state.session_id = Some(started.session_id.clone());
            state.capabilities = started.capability_overrides.clone();
        }
        CanonicalEvent::SessionExited(_) => {
            state.session_id = None;
            state.capabilities = crate::models::ProviderCapabilities::default();
        }
        _ => {}
    }
}

/// The phase input a provider event reports, if any. A provider's own `Ready`, `Active`, or
/// waiting state is ignored: the runtime derives those from turns and requests.
fn phase_input(event: &CanonicalRuntimeEvent) -> Option<RuntimeInput> {
    match &event.event {
        CanonicalEvent::SessionStateChanged(changed) => match changed.state {
            ProviderSessionState::Stopped => Some(RuntimeInput::ProviderStopped),
            ProviderSessionState::Failed => Some(RuntimeInput::ProviderFailed),
            _ => None,
        },
        CanonicalEvent::SessionExited(exited) => Some(RuntimeInput::ProviderExited {
            expected: exited.expected,
        }),
        CanonicalEvent::TurnStarted(_) => event.turn_id.clone().map(RuntimeInput::TurnStarted),
        CanonicalEvent::TurnCompleted(_) | CanonicalEvent::TurnAborted(_) => {
            event.turn_id.clone().map(RuntimeInput::TurnSettled)
        }
        CanonicalEvent::RequestOpened(opened) => Some(RuntimeInput::RequestOpened {
            request_id: opened.request_id.clone(),
            kind: OpenRequestKind::Approval,
        }),
        CanonicalEvent::UserInputRequested(requested) => Some(RuntimeInput::RequestOpened {
            request_id: requested.request_id.clone(),
            kind: OpenRequestKind::UserInput,
        }),
        CanonicalEvent::RequestResolved(resolved) => {
            Some(RuntimeInput::RequestClosed(resolved.request_id.clone()))
        }
        CanonicalEvent::UserInputResolved(resolved) => {
            Some(RuntimeInput::RequestClosed(resolved.request_id.clone()))
        }
        CanonicalEvent::RuntimeError(error) if !error.recoverable => {
            Some(RuntimeInput::ProviderFailed)
        }
        _ => None,
    }
}
