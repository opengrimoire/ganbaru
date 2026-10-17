use super::*;

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
