//! Size bound of an event's timing value.
//!
//! Sync replicates an event's start, end, zone, recurrence rule, excluded and added dates, and
//! every override row as one coupled value. That value must fit a fixed byte budget, so Calendar
//! refuses an edit or import that would exceed it instead of letting sync fail later.
//!
//! [`TimingSize`] is a deterministic upper bound of the encoded size: it charges every text
//! field its UTF-8 length plus the widest tag and length prefix the sync value encoding writes,
//! and every row a fixed structure allowance. The bound is therefore never exceeded by the real
//! encoding, and it is the single definition Calendar enforces.

/// Largest encoded timing value in bytes: 256 KiB.
pub const MAX_TIMING_VALUE_BYTES: usize = 256 * 1024;

/// Stable prefix of the error and import warning raised when the timing value is too large.
///
/// The client recognizes it to show a localized "too many changed occurrences" message.
pub const TIMING_TOO_LARGE_CODE: &str = "calendar-timing-too-large";

/// Widest tag plus length prefix of one text field (one tag byte and a four byte length).
const TEXT_FIELD_OVERHEAD_BYTES: usize = 5;
/// Width of one integer or boolean field (one tag byte and an eight byte value).
const INTEGER_FIELD_BYTES: usize = 9;
/// Width of one absent optional field (the tag byte).
const NULL_FIELD_BYTES: usize = 1;
/// Allowance for a row's field count, nested row counts, and key ordinal.
const ROW_OVERHEAD_BYTES: usize = 16;
/// Allowance for the value's own framing.
const VALUE_OVERHEAD_BYTES: usize = 64;
/// Length charged for a row identifier generated when the row is written (a prefix plus a
/// SHA-256 hex digest is the longest derived form).
pub(crate) const GENERATED_ID_ALLOWANCE_BYTES: usize = 96;
/// Length charged for a timestamp column (RFC 3339 with milliseconds and an offset).
pub(crate) const TIMESTAMP_ALLOWANCE_BYTES: usize = 40;

/// Borrowed columns of one override row, in the order the timing value carries them.
pub(crate) struct OverrideColumns<'a> {
    pub(crate) id: &'a str,
    pub(crate) recurrence_id: &'a str,
    pub(crate) recurrence_range: Option<&'a str>,
    pub(crate) title: Option<&'a str>,
    pub(crate) start_time: Option<&'a str>,
    pub(crate) end_time: Option<&'a str>,
    pub(crate) description: Option<&'a str>,
    pub(crate) location: Option<&'a str>,
    pub(crate) url: Option<&'a str>,
    pub(crate) color: Option<i64>,
    pub(crate) status: Option<&'a str>,
    pub(crate) transparency: Option<&'a str>,
    pub(crate) visibility: Option<&'a str>,
    pub(crate) icalendar_component_id: Option<&'a str>,
}

/// Running upper bound of one event's encoded timing value.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TimingSize {
    bytes: usize,
}

impl Default for TimingSize {
    fn default() -> Self {
        Self {
            bytes: VALUE_OVERHEAD_BYTES,
        }
    }
}

impl TimingSize {
    fn text(&mut self, value: &str) {
        self.charge(TEXT_FIELD_OVERHEAD_BYTES.saturating_add(value.len()));
    }

    fn optional_text(&mut self, value: Option<&str>) {
        match value {
            Some(value) => self.text(value),
            None => self.charge(NULL_FIELD_BYTES),
        }
    }

    fn integer(&mut self) {
        self.charge(INTEGER_FIELD_BYTES);
    }

    fn optional_integer(&mut self, value: Option<i64>) {
        match value {
            Some(_) => self.integer(),
            None => self.charge(NULL_FIELD_BYTES),
        }
    }

    fn row(&mut self) {
        self.charge(ROW_OVERHEAD_BYTES);
    }

    fn charge(&mut self, bytes: usize) {
        self.bytes = self.bytes.saturating_add(bytes);
    }

    /// Charges the event row's own timing columns.
    pub(crate) fn add_event(
        &mut self,
        start_time: &str,
        end_time: &str,
        timezone: &str,
        rrule: Option<&str>,
        repeat_until: Option<&str>,
    ) {
        self.text(start_time);
        self.text(end_time);
        self.text(timezone);
        self.integer();
        self.optional_text(rrule);
        self.optional_text(repeat_until);
    }

    /// Charges one excluded date or added date row; `id` is the stored identifier when known.
    pub(crate) fn add_date_row(&mut self, id: Option<&str>, value: &str) {
        self.row();
        self.text(id.unwrap_or(&"x".repeat(GENERATED_ID_ALLOWANCE_BYTES)));
        self.text(value);
        self.integer();
    }

    /// Charges one override row. `created_at` and `updated_at` are charged at the timestamp
    /// allowance.
    pub(crate) fn add_override(&mut self, columns: &OverrideColumns<'_>) {
        self.row();
        self.text(columns.id);
        self.text(columns.recurrence_id);
        self.optional_text(columns.recurrence_range);
        self.optional_text(columns.title);
        self.optional_text(columns.start_time);
        self.optional_text(columns.end_time);
        self.optional_text(columns.description);
        self.optional_text(columns.location);
        self.optional_text(columns.url);
        self.optional_integer(columns.color);
        self.optional_text(columns.status);
        self.optional_text(columns.transparency);
        self.optional_text(columns.visibility);
        self.optional_text(columns.icalendar_component_id);
        self.charge(2 * (TEXT_FIELD_OVERHEAD_BYTES + TIMESTAMP_ALLOWANCE_BYTES));
    }

    /// Charges one override extended property row; `id` is the stored identifier when known.
    pub(crate) fn add_override_property(&mut self, id: Option<&str>, key: &str, value: &str) {
        self.row();
        self.text(id.unwrap_or(&"x".repeat(GENERATED_ID_ALLOWANCE_BYTES)));
        self.text(key);
        self.text(value);
        self.integer();
    }

    /// The estimated encoded size in bytes.
    #[cfg(test)]
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    /// Refuses a timing value over [`MAX_TIMING_VALUE_BYTES`].
    pub(crate) fn check(&self) -> Result<(), String> {
        if self.bytes > MAX_TIMING_VALUE_BYTES {
            return Err(timing_too_large_message());
        }
        Ok(())
    }
}

/// The refusal for an edit whose timing value is over the bound, led by
/// [`TIMING_TOO_LARGE_CODE`].
pub(crate) fn timing_too_large_message() -> String {
    format!(
        "{TIMING_TOO_LARGE_CODE}: this event has too many changed occurrences; its schedule exceeds {MAX_TIMING_VALUE_BYTES} bytes"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_value_is_far_below_the_bound() {
        let mut size = TimingSize::default();
        size.add_event(
            "2026-01-01T00:00:00Z",
            "2026-01-01T01:00:00Z",
            "UTC",
            None,
            None,
        );
        assert!(size.bytes() < 512);
        assert!(size.check().is_ok());
    }

    #[test]
    fn estimate_never_undercounts_text_lengths() {
        let mut size = TimingSize::default();
        let before = size.bytes();
        size.add_date_row(Some("id"), "2026-01-01");
        assert!(size.bytes() - before >= "id".len() + "2026-01-01".len());
    }

    #[test]
    fn generated_ids_are_charged_at_the_allowance() {
        let mut known = TimingSize::default();
        known.add_date_row(Some("short"), "2026-01-01");
        let mut generated = TimingSize::default();
        generated.add_date_row(None, "2026-01-01");
        assert!(generated.bytes() > known.bytes());
    }

    #[test]
    fn refusal_names_the_stable_code() {
        let message = timing_too_large_message();
        assert!(message.starts_with(TIMING_TOO_LARGE_CODE));
        assert!(message.contains("too many changed occurrences"));
    }

    fn sized_with_title(title_bytes: usize) -> TimingSize {
        let mut size = TimingSize::default();
        size.add_event("a", "b", "UTC", Some("FREQ=DAILY"), None);
        let title = "t".repeat(title_bytes);
        size.add_override(&OverrideColumns {
            id: "o",
            recurrence_id: "2027-01-01",
            recurrence_range: None,
            title: Some(&title),
            start_time: None,
            end_time: None,
            description: None,
            location: None,
            url: None,
            color: None,
            status: None,
            transparency: None,
            visibility: None,
            icalendar_component_id: None,
        });
        size
    }

    #[test]
    fn value_exactly_at_the_bound_fits_and_one_byte_more_is_refused() {
        let base = sized_with_title(0).bytes();
        let at_bound = MAX_TIMING_VALUE_BYTES - base;
        assert_eq!(sized_with_title(at_bound).bytes(), MAX_TIMING_VALUE_BYTES);
        assert!(sized_with_title(at_bound).check().is_ok());
        assert!(sized_with_title(at_bound + 1).check().is_err());
    }

    fn fixture_event_blocks() -> Vec<(String, String)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/client/test-fixtures/ics");
        let mut files = Vec::new();
        for directory in [root.clone(), root.join("rfc5545")] {
            for entry in std::fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|extension| extension == "ics") {
                    files.push(path);
                }
            }
        }
        files.sort();
        let mut blocks = Vec::new();
        for path in files {
            let text = std::fs::read_to_string(&path).unwrap();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let mut rest = text.as_str();
            while let Some(start) = rest.find("BEGIN:VEVENT") {
                let end = rest[start..]
                    .find("END:VEVENT")
                    .map_or(rest.len(), |at| start + at);
                blocks.push((name.clone(), rest[start..end].to_string()));
                rest = &rest[end..];
            }
        }
        blocks
    }

    /// Charges a whole source VEVENT block as one text field on top of the event frame, which is
    /// an upper bound of the event's timing value because the value holds a subset of the block.
    #[test]
    fn repository_fixture_events_are_far_below_the_bound() {
        let blocks = fixture_event_blocks();
        assert!(!blocks.is_empty());
        let mut largest = ("", 0);
        for (name, block) in &blocks {
            let mut size = TimingSize::default();
            size.add_event("", "", "", None, None);
            size.add_override(&OverrideColumns {
                id: "",
                recurrence_id: block,
                recurrence_range: None,
                title: None,
                start_time: None,
                end_time: None,
                description: None,
                location: None,
                url: None,
                color: None,
                status: None,
                transparency: None,
                visibility: None,
                icalendar_component_id: None,
            });
            assert!(size.check().is_ok(), "{name}");
            if size.bytes() > largest.1 {
                largest = (name, size.bytes());
            }
        }
        eprintln!(
            "largest fixture timing upper bound: {} bytes ({})",
            largest.1, largest.0
        );
        assert!(largest.1 < MAX_TIMING_VALUE_BYTES / 64);
    }

    #[test]
    fn saturating_sizes_stay_over_the_bound() {
        let mut size = TimingSize::default();
        size.charge(usize::MAX);
        size.charge(10);
        assert!(size.check().is_err());
    }
}
