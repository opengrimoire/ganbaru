//! Fixed-point custom field numbers.
//!
//! Numbers are stored as integer millionths and cross the API as canonical decimal strings, so no value passes
//! through floating point.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Decimal places a custom field number keeps.
pub const NUMBER_DECIMAL_PLACES: usize = 6;

/// Stored units per whole number.
const UNITS_PER_WHOLE: i64 = 1_000_000;

/// A custom field number in millionths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FixedPointNumber(i64);

impl FixedPointNumber {
    /// Wraps a stored value in millionths.
    pub fn from_millionths(millionths: i64) -> Self {
        Self(millionths)
    }

    /// The stored value in millionths.
    pub fn millionths(self) -> i64 {
        self.0
    }

    /// Parses a decimal string such as `-12.5`, refusing exponents, more than six decimals, and values whose
    /// millionths leave the `i64` range.
    pub fn parse(text: &str) -> Result<Self, String> {
        let invalid = || format!("number_value must be a decimal number, received {text:?}");
        let (negative, digits) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (whole, fraction) = match digits.split_once('.') {
            Some((whole, fraction)) => (whole, Some(fraction)),
            None => (digits, None),
        };
        let whole_valid = !whole.is_empty() && whole.bytes().all(|byte| byte.is_ascii_digit());
        let fraction_valid = fraction
            .is_none_or(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
        if !whole_valid || !fraction_valid {
            return Err(invalid());
        }
        let fraction = fraction.unwrap_or("");
        if fraction.len() > NUMBER_DECIMAL_PLACES {
            return Err(format!(
                "number_value keeps at most {NUMBER_DECIMAL_PLACES} decimal places"
            ));
        }
        let out_of_range = || "number_value is out of range".to_string();
        let mut magnitude: i128 = 0;
        for byte in whole.bytes() {
            magnitude = magnitude
                .checked_mul(10)
                .and_then(|value| value.checked_add(i128::from(byte - b'0')))
                .filter(|value| *value <= i128::from(i64::MAX) + 1)
                .ok_or_else(out_of_range)?;
        }
        magnitude *= i128::from(UNITS_PER_WHOLE);
        let mut fraction_units: i128 = 0;
        for (index, byte) in fraction.bytes().enumerate() {
            let place = 10_i128.pow((NUMBER_DECIMAL_PLACES - 1 - index) as u32);
            fraction_units += i128::from(byte - b'0') * place;
        }
        magnitude += fraction_units;
        let signed = if negative { -magnitude } else { magnitude };
        i64::try_from(signed).map(Self).map_err(|_| out_of_range())
    }

    /// The value as a floating point number, for read-only aggregates.
    pub fn to_f64(self) -> f64 {
        self.0 as f64 / UNITS_PER_WHOLE as f64
    }
}

impl fmt::Display for FixedPointNumber {
    /// Canonical form: no exponent, no trailing fractional zeros, no negative zero.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let magnitude = self.0.unsigned_abs();
        let units = UNITS_PER_WHOLE.unsigned_abs();
        let whole = magnitude / units;
        let fraction = magnitude % units;
        if self.0 < 0 {
            formatter.write_str("-")?;
        }
        write!(formatter, "{whole}")?;
        if fraction != 0 {
            let digits = format!("{fraction:0width$}", width = NUMBER_DECIMAL_PLACES);
            write!(formatter, ".{}", digits.trim_end_matches('0'))?;
        }
        Ok(())
    }
}

impl Serialize for FixedPointNumber {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for FixedPointNumber {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_parse_keeps_exact_millionths_and_formats_canonically() {
        for (text, millionths, canonical) in [
            ("0", 0, "0"),
            ("-0", 0, "0"),
            ("12.5", 12_500_000, "12.5"),
            ("-12.500000", -12_500_000, "-12.5"),
            ("0.000001", 1, "0.000001"),
            ("007.10", 7_100_000, "7.1"),
            ("9223372036854.775807", i64::MAX, "9223372036854.775807"),
            ("-9223372036854.775808", i64::MIN, "-9223372036854.775808"),
        ] {
            let number = FixedPointNumber::parse(text).unwrap();
            assert_eq!(number.millionths(), millionths, "{text}");
            assert_eq!(number.to_string(), canonical, "{text}");
        }
    }

    #[test]
    fn number_parse_refuses_exponents_extra_decimals_and_out_of_range_values() {
        for text in [
            "",
            "-",
            ".5",
            "5.",
            "1e3",
            "+1",
            " 1",
            "1.0000001",
            "NaN",
            "1,5",
            "9223372036854.775808",
            "-9223372036854.775809",
            "99999999999999999999999",
        ] {
            assert!(FixedPointNumber::parse(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn number_serde_round_trips_as_a_decimal_string() {
        let number: FixedPointNumber = serde_json::from_str("\"-3.25\"").unwrap();
        assert_eq!(number.millionths(), -3_250_000);
        assert_eq!(serde_json::to_string(&number).unwrap(), "\"-3.25\"");
        assert!(serde_json::from_str::<FixedPointNumber>("3.25").is_err());
    }
}
