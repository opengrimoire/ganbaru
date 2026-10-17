pub(super) fn split_synthetic_id(id: &str) -> (&str, Option<&str>) {
    id.split_once("::")
        .map_or((id, None), |(parent, date)| (parent, Some(date)))
}
pub(super) fn date_part(value: &str) -> Option<String> {
    let date = value.get(0..10)?;
    if date.len() == 10 {
        Some(date.to_string())
    } else {
        None
    }
}
