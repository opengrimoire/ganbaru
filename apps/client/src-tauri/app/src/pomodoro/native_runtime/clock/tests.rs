use super::*;
use std::time::Duration;

mod idle;

#[test]
fn repeated_presentation_requests_cannot_defer_an_execution_tick() {
    let start = Instant::now();
    let mut wake = start + Duration::from_secs(1);
    for request_ms in [100, 250, 500, 750, 999, 1001] {
        wake = earlier_wake(
            wake,
            start + Duration::from_millis(request_ms),
            Duration::from_secs(1),
        );
    }
    assert_eq!(wake, start + Duration::from_secs(1));
    assert!(wake < start + Duration::from_millis(1001));
}

#[test]
fn a_new_nearer_deadline_advances_the_pending_wake() {
    let start = Instant::now();
    assert_eq!(
        earlier_wake(
            start + Duration::from_secs(1),
            start,
            Duration::from_millis(100)
        ),
        start + Duration::from_millis(100)
    );
}
