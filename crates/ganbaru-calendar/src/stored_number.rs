//! Preserved iCalendar numbers, stored as their JSON literal text so no replicated column is REAL.

/// Encodes a JSON number as the text stored in `icalendar_value_nodes.number_value`.
pub(crate) fn encode(number: &serde_json::Number) -> String {
    number.to_string()
}

/// Parses a stored number literal, refusing text that is not a finite JSON number.
pub(crate) fn parse(text: &str) -> Result<serde_json::Number, String> {
    serde_json::from_str::<serde_json::Number>(text).map_err(|error| {
        format!("Calendar preservation contains an invalid numeric value: {error}")
    })
}

#[cfg(test)]
mod tests {
    use super::{encode, parse};

    #[test]
    fn stored_numbers_round_trip_integers_and_fractions_exactly() {
        for literal in ["0", "-3", "9007199254740993", "1.5", "-0.000123", "1e+20"] {
            let number = parse(literal).unwrap();
            assert_eq!(parse(&encode(&number)).unwrap(), number, "{literal}");
        }
        assert_eq!(encode(&parse("42").unwrap()), "42");
    }

    #[test]
    fn stored_numbers_refuse_non_numeric_and_out_of_range_text() {
        for literal in ["", "abc", "1e999", "NaN", "\"1\"", "1 2"] {
            assert!(parse(literal).is_err(), "{literal}");
        }
    }
}
