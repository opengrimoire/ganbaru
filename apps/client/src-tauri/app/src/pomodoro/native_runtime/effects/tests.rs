use super::*;

#[test]
fn failed_delivery_cannot_postpone_an_earlier_semantic_deadline() {
    let now = Instant::now();
    let mut retry = DeliveryRetry::default();
    retry.failed(1, 10, now, "Overlay unavailable".into());
    assert_eq!(
        retry.next_delay(Duration::from_millis(200), None, now),
        Duration::from_millis(200)
    );
    assert!(!retry.ready(1, 10, now + Duration::from_millis(200)));
    assert!(retry.ready(1, 11, now + Duration::from_millis(200)));
    assert_eq!(retry.message(), Some("Overlay unavailable"));
}

#[test]
fn overdue_alerts_do_not_spin_during_failed_delivery_backoff() {
    let now = Instant::now();
    let mut retry = DeliveryRetry::default();
    retry.failed(1, 10, now, "Sound delivery unavailable".into());
    assert_eq!(
        retry.next_delay(Duration::from_secs(60), Some(Duration::ZERO), now),
        Duration::from_millis(super::super::ERROR_RETRY_INTERVAL_MS as u64)
    );
    let due = now + Duration::from_millis(super::super::ERROR_RETRY_INTERVAL_MS as u64);
    assert!(retry.ready(1, 10, due));
    retry.failed(1, 10, due, "Sound still unavailable".into());
    assert!(!retry.ready(1, 10, due));
}

#[test]
fn new_vault_generation_and_success_release_old_delivery_backoff() {
    let now = Instant::now();
    let mut retry = DeliveryRetry::default();
    retry.failed(1, 10, now, "Old vault publication failed".into());
    assert!(retry.ready(2, 10, now));
    retry.clear();
    assert_eq!(retry.message(), None);
    assert!(retry.ready(2, 10, now));
    assert_eq!(
        retry.next_delay(Duration::from_secs(60), Some(Duration::from_secs(1)), now),
        Duration::from_secs(1)
    );
}
