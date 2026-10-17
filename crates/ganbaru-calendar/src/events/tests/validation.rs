use super::fixtures::*;

#[test]
fn validates_event_color_and_priority_ranges() {
    assert!(validate_color(Some(0), "color").is_ok());
    assert!(validate_color(Some(31), "color").is_ok());
    assert!(validate_color(Some(32), "color").is_err());
    assert!(validate_priority(Some(9)).is_ok());
    assert!(validate_priority(Some(10)).is_err());
}

#[test]
fn validates_duration_ranges() {
    assert!(validate_positive(1, "focus_duration_minutes").is_ok());
    assert!(validate_positive(0, "focus_duration_minutes").is_err());
    assert!(validate_non_negative(0, "idle_timeout_minutes").is_ok());
    assert!(validate_non_negative(-1, "idle_timeout_minutes").is_err());
}

#[test]
fn accepts_empty_event_titles() {
    let mut event = event_create();
    event.title = String::new();
    assert!(validate_event_create(&event).is_ok());
    assert!(validate_update_field(&CalendarEventUpdateField::Title(String::new())).is_ok());
}

#[test]
fn all_day_event_create_rejects_pomodoro_config() {
    let mut event = event_create();
    event.all_day = true;
    event.pomodoro_config = Some(pomodoro_config());
    assert!(validate_event_create(&event).is_err());
}

#[test]
fn rejects_confidential_event_visibility() {
    let mut event = event_create();
    event.visibility = "confidential".to_string();
    assert!(validate_event_create(&event).is_err());
    assert!(
        validate_update_field(&CalendarEventUpdateField::Visibility(
            "confidential".to_string()
        ))
        .is_err()
    );
}
