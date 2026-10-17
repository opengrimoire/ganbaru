use crate::hlc::{CLOCK_WARNING_AHEAD_MS, MAX_ADOPTION_AHEAD_MS, is_far_ahead};
use crate::{Hlc, HlcClock};

const NOW_MS: u64 = 1_790_000_000_000;

#[test]
fn packing_splits_physical_time_and_counter() {
    let clock = Hlc::new(NOW_MS, 7);
    assert_eq!(clock.physical_ms(), NOW_MS);
    assert_eq!(clock.counter(), 7);
    assert_eq!(Hlc::from_u64(clock.as_u64()), clock);
    assert!(Hlc::new(NOW_MS, 8) > clock);
    assert!(Hlc::new(NOW_MS + 1, 0) > Hlc::new(NOW_MS, u16::MAX));
    assert_eq!(Hlc::new(u64::MAX, 0).physical_ms(), (1u64 << 48) - 1);
}

#[test]
fn tick_uses_physical_time_when_ahead() {
    let mut clock = HlcClock::new(Hlc::new(NOW_MS - 10, 3));
    assert_eq!(clock.tick(NOW_MS), Hlc::new(NOW_MS, 0));
}

#[test]
fn tick_is_monotonic_when_physical_time_goes_backwards() {
    let mut clock = HlcClock::new(Hlc::ZERO);
    let first = clock.tick(NOW_MS);
    let second = clock.tick(NOW_MS - 5_000);
    let third = clock.tick(NOW_MS);
    assert!(first < second && second < third);
    assert_eq!(second, Hlc::new(NOW_MS, 1));
}

#[test]
fn counter_overflow_advances_the_physical_part() {
    let mut clock = HlcClock::new(Hlc::new(NOW_MS, u16::MAX));
    assert_eq!(clock.tick(NOW_MS - 1), Hlc::new(NOW_MS + 1, 0));
}

#[test]
fn observe_adopts_remote_clocks_up_to_the_cap() {
    let mut clock = HlcClock::new(Hlc::new(NOW_MS, 0));
    clock.observe(Hlc::new(NOW_MS + 30_000, 4), NOW_MS);
    assert_eq!(clock.last(), Hlc::new(NOW_MS + 30_000, 4));
    assert_eq!(clock.tick(NOW_MS), Hlc::new(NOW_MS + 30_000, 5));

    let mut clock = HlcClock::new(Hlc::new(NOW_MS, 0));
    clock.observe(Hlc::new(NOW_MS + 86_400_000, 0), NOW_MS);
    assert_eq!(
        clock.last(),
        Hlc::new(NOW_MS + MAX_ADOPTION_AHEAD_MS, u16::MAX)
    );
    assert_eq!(
        clock.tick(NOW_MS),
        Hlc::new(NOW_MS + MAX_ADOPTION_AHEAD_MS + 1, 0)
    );
}

#[test]
fn observe_never_moves_the_clock_backwards() {
    let mut clock = HlcClock::new(Hlc::new(NOW_MS, 9));
    clock.observe(Hlc::new(NOW_MS - 1, 0), NOW_MS);
    assert_eq!(clock.last(), Hlc::new(NOW_MS, 9));
}

#[test]
fn far_ahead_clocks_raise_a_warning_only_past_the_threshold() {
    assert!(!is_far_ahead(
        Hlc::new(NOW_MS + CLOCK_WARNING_AHEAD_MS, u16::MAX),
        NOW_MS
    ));
    assert!(is_far_ahead(
        Hlc::new(NOW_MS + CLOCK_WARNING_AHEAD_MS + 1, 0),
        NOW_MS
    ));
    assert!(!is_far_ahead(Hlc::new(NOW_MS - 86_400_000, 0), NOW_MS));
}
