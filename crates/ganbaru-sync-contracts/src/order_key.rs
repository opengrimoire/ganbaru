//! Fractional order keys that sort by byte comparison.
//!
//! A key is an integer part followed by an optional fraction, in base 62 digits `0-9A-Za-z`. The
//! head character encodes the integer length: `a` to `z` give 2 to 27 characters, and `A` to
//! `Z` give 27 down to 2. A fraction never ends in `0`, and the smallest integer (`A` followed by
//! 26 zeros) without a fraction is not a key, so a key can always be placed before any other. The algorithm follows
//! the fractional indexing technique described by David Greenspan.

use crate::bounds::MAX_ORDER_KEY_BYTES;
use std::fmt;

const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const BASE: usize = DIGITS.len();
const ZERO: u8 = b'0';
const LAST: u8 = b'z';
const SMALLEST_INTEGER_LENGTH: usize = 27;
/// Head of migration rank keys, an integer part of four characters.
const RANK_HEAD: u8 = b'c';
/// Digits after the rank head.
const RANK_DIGITS: u32 = 3;
/// Number of distinct rank keys.
pub const RANK_KEY_CAPACITY: u32 = 62 * 62 * 62;

/// An order key that cannot be parsed or generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderKeyError {
    /// The key does not follow the grammar.
    Invalid,
    /// The lower bound is not below the upper bound.
    Unordered,
    /// The generated key would exceed [`MAX_ORDER_KEY_BYTES`] or the integer range; the caller
    /// re-keys the group.
    Exhausted,
}

impl fmt::Display for OrderKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid => formatter.write_str("order key is invalid"),
            Self::Unordered => formatter.write_str("order key bounds are not ascending"),
            Self::Exhausted => formatter.write_str("order key space is exhausted"),
        }
    }
}

impl std::error::Error for OrderKeyError {}

/// A validated order key.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OrderKey(String);

fn digit_value(byte: u8) -> Option<usize> {
    match byte {
        b'0'..=b'9' => Some(usize::from(byte - b'0')),
        b'A'..=b'Z' => Some(usize::from(byte - b'A') + 10),
        b'a'..=b'z' => Some(usize::from(byte - b'a') + 36),
        _ => None,
    }
}

fn integer_length(head: u8) -> Option<usize> {
    match head {
        b'a'..=b'z' => Some(usize::from(head - b'a') + 2),
        b'A'..=b'Z' => Some(usize::from(b'Z' - head) + 2),
        _ => None,
    }
}

fn is_smallest_integer(integer: &[u8]) -> bool {
    integer.len() == SMALLEST_INTEGER_LENGTH
        && integer[0] == b'A'
        && integer[1..].iter().all(|byte| *byte == ZERO)
}

/// Splits a key into its integer part and fraction after validating the grammar.
fn split(key: &[u8]) -> Result<(&[u8], &[u8]), OrderKeyError> {
    let head = *key.first().ok_or(OrderKeyError::Invalid)?;
    let length = integer_length(head).ok_or(OrderKeyError::Invalid)?;
    if key.len() < length || key.len() > MAX_ORDER_KEY_BYTES {
        return Err(OrderKeyError::Invalid);
    }
    if key[1..].iter().any(|byte| digit_value(*byte).is_none()) {
        return Err(OrderKeyError::Invalid);
    }
    let (integer, fraction) = key.split_at(length);
    if (is_smallest_integer(integer) && fraction.is_empty()) || fraction.last() == Some(&ZERO) {
        return Err(OrderKeyError::Invalid);
    }
    Ok((integer, fraction))
}

/// A key between two fractions, `low < high`, with `high` absent for an open upper bound.
fn midpoint(low: &[u8], high: Option<&[u8]>, output: &mut Vec<u8>) -> Result<(), OrderKeyError> {
    if output.len() > MAX_ORDER_KEY_BYTES {
        return Err(OrderKeyError::Exhausted);
    }
    if let Some(high) = high {
        let common = (0..high.len())
            .take_while(|index| low.get(*index).copied().unwrap_or(ZERO) == high[*index])
            .count();
        if common > 0 {
            output.extend_from_slice(&high[..common]);
            let low_rest = low.get(common..).unwrap_or(&[]);
            return midpoint(low_rest, Some(&high[common..]), output);
        }
    }
    let low_digit = match low.first() {
        Some(byte) => digit_value(*byte).ok_or(OrderKeyError::Invalid)?,
        None => 0,
    };
    let high_digit = match high.and_then(|high| high.first()) {
        Some(byte) => digit_value(*byte).ok_or(OrderKeyError::Invalid)?,
        None => BASE,
    };
    if high_digit <= low_digit {
        return Err(OrderKeyError::Unordered);
    }
    if high_digit - low_digit > 1 {
        output.push(DIGITS[(low_digit + high_digit).div_ceil(2)]);
        return Ok(());
    }
    if let Some(high) = high.filter(|high| high.len() > 1) {
        output.push(high[0]);
        return Ok(());
    }
    output.push(DIGITS[low_digit]);
    midpoint(low.get(1..).unwrap_or(&[]), None, output)
}

fn increment_integer(integer: &[u8]) -> Option<Vec<u8>> {
    let mut digits = integer[1..].to_vec();
    let head = integer[0];
    for digit in digits.iter_mut().rev() {
        if *digit == LAST {
            *digit = ZERO;
        } else {
            let value = digit_value(*digit)?;
            *digit = DIGITS[value + 1];
            return Some([&[head], digits.as_slice()].concat());
        }
    }
    match head {
        b'Z' => Some(vec![b'a', ZERO]),
        b'z' => None,
        _ => {
            let next = head + 1;
            if next > b'a' {
                digits.push(ZERO);
            } else {
                digits.pop();
            }
            Some([&[next], digits.as_slice()].concat())
        }
    }
}

fn decrement_integer(integer: &[u8]) -> Option<Vec<u8>> {
    let mut digits = integer[1..].to_vec();
    let head = integer[0];
    for digit in digits.iter_mut().rev() {
        if *digit == ZERO {
            *digit = LAST;
        } else {
            let value = digit_value(*digit)?;
            *digit = DIGITS[value - 1];
            return Some([&[head], digits.as_slice()].concat());
        }
    }
    match head {
        b'a' => Some(vec![b'Z', LAST]),
        b'A' => None,
        _ => {
            let previous = head - 1;
            if previous < b'Z' {
                digits.push(LAST);
            } else {
                digits.pop();
            }
            Some([&[previous], digits.as_slice()].concat())
        }
    }
}

fn finish(bytes: Vec<u8>) -> Result<OrderKey, OrderKeyError> {
    if bytes.len() > MAX_ORDER_KEY_BYTES {
        return Err(OrderKeyError::Exhausted);
    }
    let text = String::from_utf8(bytes).map_err(|_| OrderKeyError::Invalid)?;
    split(text.as_bytes())?;
    Ok(OrderKey(text))
}

impl OrderKey {
    /// Validates a key.
    pub fn parse(key: &str) -> Result<Self, OrderKeyError> {
        split(key.as_bytes())?;
        Ok(Self(key.to_string()))
    }

    /// Key for the first item of an empty list.
    pub fn first() -> Self {
        Self("a0".to_string())
    }

    /// Key text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the key into its text.
    pub fn into_string(self) -> String {
        self.0
    }

    /// A key strictly between `low` and `high`; either bound may be open.
    pub fn between(low: Option<&Self>, high: Option<&Self>) -> Result<Self, OrderKeyError> {
        match (low, high) {
            (None, None) => Ok(Self::first()),
            (None, Some(high)) => high.key_before(),
            (Some(low), None) => low.key_after(),
            (Some(low), Some(high)) => {
                if low >= high {
                    return Err(OrderKeyError::Unordered);
                }
                let (low_integer, low_fraction) = split(low.0.as_bytes())?;
                let (high_integer, high_fraction) = split(high.0.as_bytes())?;
                let mut output = Vec::with_capacity(high.0.len() + 1);
                if low_integer == high_integer {
                    output.extend_from_slice(low_integer);
                    midpoint(low_fraction, Some(high_fraction), &mut output)?;
                    return finish(output);
                }
                let next = increment_integer(low_integer).ok_or(OrderKeyError::Exhausted)?;
                if next.as_slice() < high.0.as_bytes() {
                    return finish(next);
                }
                output.extend_from_slice(low_integer);
                midpoint(low_fraction, None, &mut output)?;
                finish(output)
            }
        }
    }

    /// A key strictly before this one.
    pub fn key_before(&self) -> Result<Self, OrderKeyError> {
        let (integer, fraction) = split(self.0.as_bytes())?;
        if is_smallest_integer(integer) {
            let mut output = integer.to_vec();
            midpoint(&[], Some(fraction), &mut output)?;
            return finish(output);
        }
        if !fraction.is_empty() {
            return finish(integer.to_vec());
        }
        let mut output = decrement_integer(integer).ok_or(OrderKeyError::Exhausted)?;
        if is_smallest_integer(&output) {
            midpoint(&[], None, &mut output)?;
        }
        finish(output)
    }

    /// A key strictly after this one.
    pub fn key_after(&self) -> Result<Self, OrderKeyError> {
        let (integer, fraction) = split(self.0.as_bytes())?;
        match increment_integer(integer) {
            Some(next) => finish(next),
            None => {
                let mut output = integer.to_vec();
                midpoint(fraction, None, &mut output)?;
                finish(output)
            }
        }
    }

    /// Migration rank key for a zero-based position: `c` followed by three base 62 digits.
    pub fn rank(position: u32) -> Result<Self, OrderKeyError> {
        if position >= RANK_KEY_CAPACITY {
            return Err(OrderKeyError::Exhausted);
        }
        let mut output = vec![RANK_HEAD];
        for power in (0..RANK_DIGITS).rev() {
            let digit = (position / 62u32.pow(power)) % 62;
            output.push(DIGITS[digit as usize]);
        }
        finish(output)
    }
}

impl fmt::Debug for OrderKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("OrderKey").field(&self.0).finish()
    }
}

impl fmt::Display for OrderKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
