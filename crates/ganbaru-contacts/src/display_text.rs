//! Validation of short text other devices display, such as device labels and person names.
//!
//! Signed or peer-supplied text is rejected rather than stripped, so a signature still covers
//! exactly what is shown. Zero-width joiner and non-joiner (U+200D, U+200C) stay valid because
//! emoji sequences and Persian and Indic scripts need them.

/// Why display text was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayTextError {
    /// The text is empty or only whitespace.
    Empty,
    /// The text is longer than the field allows, in UTF-8 bytes.
    TooLong,
    /// The text holds a control, bidirectional override, or invisible character.
    DisallowedCharacter,
}

impl std::fmt::Display for DisplayTextError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "text is empty",
            Self::TooLong => "text is too long",
            Self::DisallowedCharacter => "text contains a control or invisible character",
        })
    }
}

impl std::error::Error for DisplayTextError {}

/// Whether `character` can spoof or hide displayed text: control characters, the Arabic letter
/// mark, directional marks, embeddings, overrides and isolates, and invisible separators.
pub fn is_disallowed_display_character(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{061C}'
                | '\u{200B}'
                | '\u{200E}'
                | '\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'
                | '\u{2066}'..='\u{2069}'
                | '\u{FEFF}'
        )
}

/// Validates text shown to other people or devices.
///
/// # Errors
///
/// Returns [`DisplayTextError`] when `text` is blank, longer than `max_bytes`, or contains a
/// character that [`is_disallowed_display_character`] reports.
pub fn validate_display_text(text: &str, max_bytes: usize) -> Result<(), DisplayTextError> {
    if text.trim().is_empty() {
        return Err(DisplayTextError::Empty);
    }
    if text.len() > max_bytes {
        return Err(DisplayTextError::TooLong);
    }
    if text.chars().any(is_disallowed_display_character) {
        return Err(DisplayTextError::DisallowedCharacter);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_control_bidi_and_invisible_characters() {
        for text in [
            "phone\u{0007}",
            "a\u{202E}b",
            "a\u{2066}b\u{2069}",
            "a\u{200E}b",
            "a\u{061C}b",
            "a\u{200B}b",
            "a\u{2060}b",
            "\u{FEFF}phone",
            "line\nbreak",
        ] {
            assert_eq!(
                validate_display_text(text, 128),
                Err(DisplayTextError::DisallowedCharacter),
                "{text:?}"
            );
        }
    }

    #[test]
    fn keeps_joiners_that_scripts_and_emoji_need() {
        for text in [
            "Mi teléfono",
            "\u{0645}\u{06CC}\u{200C}\u{062E}\u{0648}\u{0627}\u{0647}\u{0645}",
            "\u{0915}\u{094D}\u{200D}\u{0937}",
            "\u{1F469}\u{200D}\u{1F4BB} laptop",
            "\u{05E9}\u{05DC}\u{05D5}\u{05DD}",
        ] {
            assert_eq!(validate_display_text(text, 128), Ok(()), "{text:?}");
        }
    }

    #[test]
    fn rejects_blank_and_oversized_text() {
        assert_eq!(
            validate_display_text(" \t", 8),
            Err(DisplayTextError::Empty)
        );
        assert_eq!(validate_display_text("", 8), Err(DisplayTextError::Empty));
        assert_eq!(
            validate_display_text("ñññññ", 8),
            Err(DisplayTextError::TooLong)
        );
        assert_eq!(validate_display_text("ññññ", 8), Ok(()));
    }
}
