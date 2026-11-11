use super::*;
use crate::events::{CANONICAL_EVENT_SCHEMA_VERSION, TurnAbortedEvent};
use crate::models::{ChatEventId, ChatTurnState, ProviderFamilyId, ProviderInstanceId};

pub(super) async fn run_driver_operation<T>(
    context: &DriverOperationContext,
    future: DriverFuture<'_, T>,
) -> ChatResult<T> {
    tokio::time::timeout_at(tokio::time::Instant::from_std(context.deadline), future)
        .await
        .map_err(|_| runtime_timeout_error())?
}

pub(super) async fn run_driver_operation_with_flush<T>(
    context: &DriverOperationContext,
    mut future: DriverFuture<'_, T>,
    event_sink: Option<Arc<dyn ProviderEventSink>>,
) -> ChatResult<T> {
    loop {
        let now = Instant::now();
        if now >= context.deadline {
            return Err(runtime_timeout_error());
        }
        let tick_deadline = (now + STOP_FLUSH_INTERVAL).min(context.deadline);
        match tokio::time::timeout_at(tokio::time::Instant::from_std(tick_deadline), &mut future)
            .await
        {
            Ok(result) => return result,
            Err(_) if tick_deadline == context.deadline => return Err(runtime_timeout_error()),
            Err(_) => {
                if let Some(sink) = event_sink.as_ref() {
                    run_driver_operation(context, sink.flush()).await?;
                }
            }
        }
    }
}

pub(super) fn operation_context(operation_id: &str, deadline: Instant) -> DriverOperationContext {
    DriverOperationContext {
        operation_id: operation_id.to_string(),
        deadline,
        cancellation: DriverCancellation::default(),
    }
}

pub(super) async fn receive_response<T>(
    receiver: oneshot::Receiver<ChatResult<T>>,
) -> ChatResult<T> {
    receiver.await.map_err(|_| runtime_unavailable_error())?
}

pub(super) fn runtime_state_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Chat runtime registry is unavailable",
        false,
    )
}

pub(super) fn runtime_unavailable_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::DriverUnavailable,
        "Chat thread session is not accepting commands",
        true,
    )
}

pub(super) fn runtime_timeout_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::DriverUnavailable,
        "Chat provider operation exceeded its deadline",
        true,
    )
}

pub(super) fn runtime_invalid_state_error(message: &'static str) -> ChatError {
    ChatError::new(ChatErrorCode::InvalidStateTransition, message, true)
}

pub(super) fn runtime_identifier_error<T>(_error: T) -> ChatError {
    runtime_state_error()
}

pub(super) fn stale_event_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Conflict,
        "Chat provider event belongs to an inactive session generation",
        false,
    )
}

/// The warning and the interrupted abort that settle a turn whose provider never confirmed an
/// accepted interrupt. Both ids derive from the thread and turn, so recording them twice stores
/// them once.
pub(super) fn unconfirmed_interrupt_events(
    thread_id: &ChatThreadId,
    turn_id: &ChatTurnId,
    family_id: &ProviderFamilyId,
    instance_id: &ProviderInstanceId,
) -> ChatResult<[CanonicalRuntimeEvent; 2]> {
    let created_at = crate::driver_operations::now_timestamp()?;
    let event = |prefix: &str,
                 turn: Option<ChatTurnId>,
                 event: CanonicalEvent|
     -> ChatResult<CanonicalRuntimeEvent> {
        Ok(CanonicalRuntimeEvent {
            schema_version: CANONICAL_EVENT_SCHEMA_VERSION,
            event_id: ChatEventId::new(format!(
                "{prefix}:{}:{}",
                thread_id.as_str(),
                turn_id.as_str()
            ))
            .map_err(runtime_identifier_error)?,
            provider_family_id: family_id.clone(),
            provider_instance_id: instance_id.clone(),
            thread_id: thread_id.clone(),
            created_at: created_at.clone(),
            turn_id: turn,
            provider_turn_id: None,
            provider_item_id: None,
            provider_request_id: None,
            provider_task_id: None,
            provider_reference: None,
            event,
            redacted_diagnostic: None,
        })
    };
    Ok([
        event(
            "interrupt-timeout-warning",
            Some(turn_id.clone()),
            CanonicalEvent::RuntimeWarning(NotificationEvent {
                code: "provider_interrupt_unconfirmed".to_string(),
                title: "The provider did not confirm Stop".to_string(),
                detail: Some(
                    "The turn was settled as interrupted and the provider session was stopped"
                        .to_string(),
                ),
            }),
        )?,
        event(
            "interrupt-timeout",
            Some(turn_id.clone()),
            CanonicalEvent::TurnAborted(TurnAbortedEvent {
                state: ChatTurnState::Interrupted,
                reason: "The provider did not confirm Stop in time".to_string(),
                recoverable: true,
            }),
        )?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupt_timeout_events_settle_the_turn_with_deterministic_ids() {
        let thread_id = ChatThreadId::new("thread-1".to_string()).unwrap();
        let turn_id = ChatTurnId::new("turn-1".to_string()).unwrap();
        let family_id = ProviderFamilyId::new("fake".to_string()).unwrap();
        let instance_id = ProviderInstanceId::new("fake-instance".to_string()).unwrap();
        let [warning, aborted] =
            unconfirmed_interrupt_events(&thread_id, &turn_id, &family_id, &instance_id).unwrap();
        let [again, _] =
            unconfirmed_interrupt_events(&thread_id, &turn_id, &family_id, &instance_id).unwrap();

        assert_eq!(
            warning.event_id.as_str(),
            "interrupt-timeout-warning:thread-1:turn-1"
        );
        assert_eq!(warning.event_id, again.event_id);
        assert!(matches!(
            &warning.event,
            CanonicalEvent::RuntimeWarning(notice) if notice.code == "provider_interrupt_unconfirmed"
        ));
        assert_eq!(
            aborted.event_id.as_str(),
            "interrupt-timeout:thread-1:turn-1"
        );
        assert_eq!(aborted.turn_id.as_ref(), Some(&turn_id));
        assert_eq!(warning.turn_id.as_ref(), Some(&turn_id));
        assert_eq!(aborted.provider_instance_id, instance_id);
        assert!(matches!(
            &aborted.event,
            CanonicalEvent::TurnAborted(event)
                if event.state == ChatTurnState::Interrupted && event.recoverable
        ));
    }
}
