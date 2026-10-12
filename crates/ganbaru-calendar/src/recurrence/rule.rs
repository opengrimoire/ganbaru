//! Bounded parsing of the projected, date-based RFC 5545 rule subset.

use std::collections::BTreeSet;

use chrono::{NaiveDate, NaiveDateTime, Weekday};

const MAX_RULE_BYTES: usize = 8_192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ByDay {
    pub weekday: Weekday,
    pub ordinal: Option<i8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Until {
    Date(NaiveDate),
    Local(NaiveDateTime),
    Instant(i64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Rule {
    pub frequency: Frequency,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<Until>,
    pub by_day: Vec<ByDay>,
    pub by_month_day: Vec<i16>,
    pub by_year_day: Vec<i16>,
    pub by_week_no: Vec<i16>,
    pub by_month: Vec<i16>,
    pub by_set_pos: Vec<i16>,
    pub week_start: Weekday,
}

fn weekday_name(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "MO",
        Weekday::Tue => "TU",
        Weekday::Wed => "WE",
        Weekday::Thu => "TH",
        Weekday::Fri => "FR",
        Weekday::Sat => "SA",
        Weekday::Sun => "SU",
    }
}

impl Rule {
    /// Serialize the supported native subset without dropping selectors or termination.
    pub(super) fn encode(&self) -> Result<String, String> {
        let frequency = match self.frequency {
            Frequency::Daily => "DAILY",
            Frequency::Weekly => "WEEKLY",
            Frequency::Monthly => "MONTHLY",
            Frequency::Yearly => "YEARLY",
        };
        let mut parts = vec![
            format!("FREQ={frequency}"),
            format!("INTERVAL={}", self.interval),
        ];
        if let Some(count) = self.count {
            parts.push(format!("COUNT={count}"));
        }
        if let Some(until) = self.until {
            let value = match until {
                Until::Date(date) => date.format("%Y%m%d").to_string(),
                Until::Local(date) => date.format("%Y%m%dT%H%M%S").to_string(),
                Until::Instant(ms) => chrono::DateTime::from_timestamp_millis(ms)
                    .ok_or("Calendar UNTIL exceeds its supported instant range")?
                    .format("%Y%m%dT%H%M%SZ")
                    .to_string(),
            };
            parts.push(format!("UNTIL={value}"));
        }
        if !self.by_day.is_empty() {
            let days = self
                .by_day
                .iter()
                .map(|day| {
                    format!(
                        "{}{}",
                        day.ordinal
                            .map(|value| value.to_string())
                            .unwrap_or_default(),
                        weekday_name(day.weekday)
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            parts.push(format!("BYDAY={days}"));
        }
        for (name, values) in [
            ("BYMONTHDAY", &self.by_month_day),
            ("BYYEARDAY", &self.by_year_day),
            ("BYWEEKNO", &self.by_week_no),
            ("BYMONTH", &self.by_month),
            ("BYSETPOS", &self.by_set_pos),
        ] {
            if !values.is_empty() {
                parts.push(format!(
                    "{name}={}",
                    values
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                ));
            }
        }
        parts.push(format!("WKST={}", weekday_name(self.week_start)));
        let encoded = parts.join(";");
        // Internal rewrites must satisfy the same contract as imported rules.
        if parse(&encoded)? != *self {
            return Err(
                "Calendar recurrence cannot be serialized without losing information".into(),
            );
        }
        Ok(encoded)
    }
}

fn weekday(value: &str) -> Result<Weekday, String> {
    match value {
        "MO" => Ok(Weekday::Mon),
        "TU" => Ok(Weekday::Tue),
        "WE" => Ok(Weekday::Wed),
        "TH" => Ok(Weekday::Thu),
        "FR" => Ok(Weekday::Fri),
        "SA" => Ok(Weekday::Sat),
        "SU" => Ok(Weekday::Sun),
        _ => Err(format!("invalid Calendar recurrence weekday: {value}")),
    }
}

fn integer_list(value: &str, min: i16, max: i16) -> Result<Vec<i16>, String> {
    let mut values = BTreeSet::new();
    for part in value.split(',') {
        let number = part
            .parse::<i16>()
            .map_err(|_| "invalid Calendar recurrence integer")?;
        if number == 0 || number < min || number > max {
            return Err(format!(
                "Calendar recurrence integer must be nonzero and within {min}..={max}"
            ));
        }
        values.insert(number);
    }
    Ok(values.into_iter().collect())
}

fn parse_by_day(value: &str) -> Result<Vec<ByDay>, String> {
    let mut values = Vec::new();
    for part in value.split(',') {
        if !part.is_ascii() || part.len() < 2 || part.len() > 5 {
            return Err("invalid Calendar recurrence BYDAY".into());
        }
        let (ordinal, day) = part.split_at(part.len() - 2);
        let ordinal = if ordinal.is_empty() {
            None
        } else {
            let parsed = ordinal
                .parse::<i8>()
                .map_err(|_| "invalid Calendar recurrence BYDAY ordinal")?;
            if parsed == 0 || !(-53..=53).contains(&parsed) {
                return Err(
                    "Calendar recurrence BYDAY ordinal must be nonzero and within -53..=53".into(),
                );
            }
            Some(parsed)
        };
        let value = ByDay {
            weekday: weekday(day)?,
            ordinal,
        };
        if !values.contains(&value) {
            values.push(value);
        }
    }
    Ok(values)
}

fn positive_integer(value: &str) -> Result<u32, String> {
    let number = value
        .parse::<u32>()
        .map_err(|_| "invalid Calendar recurrence positive integer")?;
    if number == 0 || number > i32::MAX as u32 {
        return Err("Calendar recurrence value must be a positive 32-bit integer".into());
    }
    Ok(number)
}

fn parse_until(value: &str) -> Result<Until, String> {
    if value.len() == 8 {
        return NaiveDate::parse_from_str(value, "%Y%m%d")
            .map(Until::Date)
            .map_err(|error| format!("invalid Calendar recurrence UNTIL date: {error}"));
    }
    let (civil, utc) = value
        .strip_suffix('Z')
        .map_or((value, false), |value| (value, true));
    let parsed = NaiveDateTime::parse_from_str(civil, "%Y%m%dT%H%M%S")
        .map_err(|error| format!("invalid Calendar recurrence UNTIL datetime: {error}"))?;
    Ok(if utc {
        Until::Instant(parsed.and_utc().timestamp_millis())
    } else {
        Until::Local(parsed)
    })
}

/// Reject malformed or preservation-only rule parts instead of silently changing their meaning.
pub(super) fn parse(value: &str) -> Result<Rule, String> {
    if value.is_empty() || value.len() > MAX_RULE_BYTES || !value.is_ascii() {
        return Err("Calendar recurrence rule is empty, non-ASCII, or exceeds 8192 bytes".into());
    }
    let value = value.strip_prefix("RRULE:").unwrap_or(value);
    let mut seen = BTreeSet::new();
    let mut frequency = None;
    let mut rule = Rule {
        frequency: Frequency::Daily,
        interval: 1,
        count: None,
        until: None,
        by_day: Vec::new(),
        by_month_day: Vec::new(),
        by_year_day: Vec::new(),
        by_week_no: Vec::new(),
        by_month: Vec::new(),
        by_set_pos: Vec::new(),
        week_start: Weekday::Mon,
    };
    for part in value.split(';') {
        let (key, value) = part
            .split_once('=')
            .ok_or("invalid Calendar recurrence rule part")?;
        if !seen.insert(key) {
            return Err(format!("duplicate Calendar recurrence rule part: {key}"));
        }
        match key {
            "FREQ" => {
                frequency = Some(match value {
                    "DAILY" => Frequency::Daily,
                    "WEEKLY" => Frequency::Weekly,
                    "MONTHLY" => Frequency::Monthly,
                    "YEARLY" => Frequency::Yearly,
                    _ => {
                        return Err(format!(
                            "Calendar recurrence frequency is preservation-only: {value}"
                        ));
                    }
                })
            }
            "INTERVAL" => rule.interval = positive_integer(value)?,
            "COUNT" => rule.count = Some(positive_integer(value)?),
            "UNTIL" => rule.until = Some(parse_until(value)?),
            "BYDAY" => rule.by_day = parse_by_day(value)?,
            "BYMONTHDAY" => rule.by_month_day = integer_list(value, -31, 31)?,
            "BYYEARDAY" => rule.by_year_day = integer_list(value, -366, 366)?,
            "BYWEEKNO" => rule.by_week_no = integer_list(value, -53, 53)?,
            "BYMONTH" => rule.by_month = integer_list(value, 1, 12)?,
            "BYSETPOS" => rule.by_set_pos = integer_list(value, -366, 366)?,
            "WKST" => rule.week_start = weekday(value)?,
            _ => {
                return Err(format!(
                    "Calendar recurrence rule part is preservation-only: {key}"
                ));
            }
        }
    }
    rule.frequency = frequency.ok_or("Calendar recurrence FREQ is required")?;
    if rule.count.is_some() && rule.until.is_some() {
        return Err("Calendar recurrence cannot combine COUNT and UNTIL".into());
    }
    if !rule.by_week_no.is_empty() && rule.frequency != Frequency::Yearly {
        return Err("Calendar recurrence BYWEEKNO requires YEARLY".into());
    }
    if !rule.by_year_day.is_empty() && rule.frequency != Frequency::Yearly {
        return Err(
            "Calendar recurrence BYYEARDAY requires YEARLY for date-based frequencies".into(),
        );
    }
    if !rule.by_month_day.is_empty() && rule.frequency == Frequency::Weekly {
        return Err("Calendar recurrence BYMONTHDAY cannot be combined with WEEKLY".into());
    }
    if rule.by_day.iter().any(|day| day.ordinal.is_some())
        && (!matches!(rule.frequency, Frequency::Monthly | Frequency::Yearly)
            || !rule.by_week_no.is_empty())
    {
        return Err(
            "Calendar recurrence ordinal BYDAY requires MONTHLY or YEARLY without BYWEEKNO".into(),
        );
    }
    if !rule.by_set_pos.is_empty()
        && rule.by_day.is_empty()
        && rule.by_month_day.is_empty()
        && rule.by_year_day.is_empty()
        && rule.by_week_no.is_empty()
        && rule.by_month.is_empty()
    {
        return Err("Calendar recurrence BYSETPOS requires another BY rule part".into());
    }
    Ok(rule)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_rules_instead_of_dropping_unsupported_or_contradictory_parts() {
        for value in [
            "FREQ=DAILY;INTERVAL=0",
            "FREQ=DAILY;COUNT=-1",
            "FREQ=DAILY;COUNT=3;UNTIL=20260101",
            "FREQ=DAILY;BYDAY=1MO",
            "FREQ=WEEKLY;BYMONTHDAY=1",
            "FREQ=MONTHLY;BYWEEKNO=2",
            "FREQ=YEARLY;BYWEEKNO=2;BYDAY=1MO",
            "FREQ=DAILY;BYHOUR=9",
            "FREQ=HOURLY",
            "FREQ=DAILY;BYSETPOS=1",
            "FREQ=DAILY;COUNT=1;COUNT=2",
            "FREQ=MONTHLY;BYMONTHDAY=32",
            "FREQ=MONTHLY;BYDAY=0MO",
            "FREQ=MONTHLY;BYDAY=54MO",
            "FREQ=YEARLY;BYMONTH=0",
        ] {
            assert!(parse(value).is_err(), "{value}");
        }
    }

    #[test]
    fn preserves_time_value_kinds_and_normalizes_duplicate_values() {
        let rule = parse(
            "FREQ=MONTHLY;BYMONTHDAY=-1,15,15;BYDAY=MO,MO;BYSETPOS=-1;UNTIL=20261231T150000Z",
        )
        .unwrap();
        assert_eq!(rule.by_month_day, [-1, 15]);
        assert_eq!(rule.by_day.len(), 1);
        assert!(matches!(rule.until, Some(Until::Instant(_))));
        assert!(matches!(
            parse("FREQ=DAILY;UNTIL=20261231").unwrap().until,
            Some(Until::Date(_))
        ));
        assert!(matches!(
            parse("FREQ=DAILY;UNTIL=20261231T150000").unwrap().until,
            Some(Until::Local(_))
        ));
    }
}
