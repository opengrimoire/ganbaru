use chrono::DateTime;

pub(super) fn iso_seconds_between(start: &str, end: &str) -> Option<i64> {
    let start = DateTime::parse_from_rfc3339(start).ok()?;
    let end = DateTime::parse_from_rfc3339(end).ok()?;
    Some((end - start).num_seconds())
}

pub(super) fn iso_is_before(left: &str, right: &str) -> bool {
    let Some(left) = DateTime::parse_from_rfc3339(left).ok() else {
        return false;
    };
    let Some(right) = DateTime::parse_from_rfc3339(right).ok() else {
        return false;
    };
    left < right
}

pub(super) fn bounded_overlap_seconds(start: &str, end: &str, lower: &str, upper: &str) -> i64 {
    let Some(start) = DateTime::parse_from_rfc3339(start).ok() else {
        return 0;
    };
    let Some(end) = DateTime::parse_from_rfc3339(end).ok() else {
        return 0;
    };
    let Some(lower) = DateTime::parse_from_rfc3339(lower).ok() else {
        return 0;
    };
    let Some(upper) = DateTime::parse_from_rfc3339(upper).ok() else {
        return 0;
    };
    let clipped_start = start.max(lower);
    let clipped_end = end.min(upper);
    (clipped_end - clipped_start).num_seconds().max(0)
}
