//! User themes adapter: the canonical token and event palette codecs and the value checks that
//! `ganbaru-themes` applies to every row it reads, so a theme that merges on one device always
//! loads on another.
//!
//! Tokens are keyed by kind and key and encode in ascending byte order of both, the order the
//! primary key of the token tables reads them in. A palette has exactly [`PALETTE_SIZE`] slots in
//! slot order.

use std::fmt;

use ganbaru_sync_contracts::codec::{Reader, Writer};
use ganbaru_sync_contracts::{Field, GroupId, TableId, Value};

use crate::adapter::{AdapterError, DomainAdapter, Presentation};
use crate::manifest::vault::themes::{
    ADAPTER, PALETTE_OWNED, SEED_PALETTE_OWNED, SEED_TOKENS_OWNED, THEMES_TABLE, TOKENS_OWNED,
    group,
};

/// Event palette slots of a theme.
pub const PALETTE_SIZE: usize = 32;
/// Characters of a theme display name.
pub const MAX_DISPLAY_NAME_CHARS: usize = 60;
/// Characters of a `#rrggbb` or `#rrggbbaa` color.
pub const MAX_COLOR_CHARS: usize = 9;
/// Token kinds in ascending byte order, indexed by their codec byte.
pub const TOKEN_KINDS: [&str; 3] = ["app", "calendar", "source"];
/// Characters of the longest token kind.
pub const MAX_TOKEN_KIND_CHARS: usize = 8;
/// Characters of a token key. Keys are CSS custom property names or source names, about half
/// this long, and the bound leaves room for keys a newer app adds.
pub const MAX_TOKEN_KEY_CHARS: usize = 64;
/// Tokens of one theme. A theme has about 50, and the bound leaves room for tokens a newer app
/// adds.
pub const MAX_TOKENS: usize = 256;
/// Icon labels a theme may have.
pub const ICON_LABELS: [&str; 2] = ["light", "dark"];
/// Characters of the longest icon label.
pub const MAX_ICON_LABEL_CHARS: usize = 5;
/// Modes a theme's Calendar default color may follow.
pub const CALENDAR_MODES: [&str; 4] = ["light", "dark", "app-canvas", "custom"];
/// Characters of the longest Calendar default mode.
pub const MAX_CALENDAR_MODE_CHARS: usize = 10;

/// Whether `value` is a `#` followed by 6 or 8 hex digits.
pub fn is_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.first() == Some(&b'#')
        && matches!(bytes.len(), 7 | 9)
        && bytes[1..].iter().all(u8::is_ascii_hexdigit)
}

/// Whether `value` is a display name: not blank and at most [`MAX_DISPLAY_NAME_CHARS`].
pub fn is_display_name(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= MAX_DISPLAY_NAME_CHARS
}

/// Whether `value` is one of [`ICON_LABELS`].
pub fn is_icon_label(value: &str) -> bool {
    ICON_LABELS.contains(&value)
}

/// Whether `value` is one of [`CALENDAR_MODES`].
pub fn is_calendar_mode(value: &str) -> bool {
    CALENDAR_MODES.contains(&value)
}

/// Whether `value` is a token key: 1 to [`MAX_TOKEN_KEY_CHARS`] visible ASCII characters.
pub fn is_token_key(value: &str) -> bool {
    (1..=MAX_TOKEN_KEY_CHARS).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_graphic())
}

fn kind_code(kind: &str) -> Option<u8> {
    TOKEN_KINDS
        .iter()
        .position(|candidate| *candidate == kind)
        .and_then(|index| u8::try_from(index).ok())
}

/// One theme token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeToken {
    /// One of [`TOKEN_KINDS`].
    pub kind: String,
    /// Source name or CSS custom property name.
    pub key: String,
    /// Hex color.
    pub value: String,
    /// Whether the token keeps its value when its source changes.
    pub isolated: bool,
}

/// Why a token list or palette cannot belong to a theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeValueError {
    /// More than [`MAX_TOKENS`] tokens were given.
    TooManyTokens,
    /// A token kind is not one of [`TOKEN_KINDS`].
    TokenKind,
    /// A token key is empty, too long, or not visible ASCII.
    TokenKey,
    /// Two tokens share a kind and key.
    DuplicateToken,
    /// A color is not a hex color.
    Color,
    /// A palette does not have exactly [`PALETTE_SIZE`] distinct slots.
    Palette,
}

impl fmt::Display for ThemeValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyTokens => write!(
                formatter,
                "a theme cannot have more than {MAX_TOKENS} tokens"
            ),
            Self::TokenKind => formatter.write_str("theme token kind is invalid"),
            Self::TokenKey => write!(
                formatter,
                "theme token keys must be 1 to {MAX_TOKEN_KEY_CHARS} visible ASCII characters"
            ),
            Self::DuplicateToken => formatter.write_str("a theme cannot repeat a token"),
            Self::Color => formatter.write_str("theme colors must be 6 or 8 digit hex colors"),
            Self::Palette => write!(
                formatter,
                "a theme palette must have exactly {PALETTE_SIZE} distinct slots"
            ),
        }
    }
}

impl std::error::Error for ThemeValueError {}

/// Checks one token against the codec bounds.
pub fn validate_token(token: &ThemeToken) -> Result<(), ThemeValueError> {
    if kind_code(&token.kind).is_none() {
        return Err(ThemeValueError::TokenKind);
    }
    if !is_token_key(&token.key) {
        return Err(ThemeValueError::TokenKey);
    }
    if !is_hex_color(&token.value) {
        return Err(ThemeValueError::Color);
    }
    Ok(())
}

/// Canonical form of a token list: every token checked and the list sorted by kind and key.
pub fn canonical_tokens(tokens: &[ThemeToken]) -> Result<Vec<ThemeToken>, ThemeValueError> {
    if tokens.len() > MAX_TOKENS {
        return Err(ThemeValueError::TooManyTokens);
    }
    for token in tokens {
        validate_token(token)?;
    }
    let mut sorted = tokens.to_vec();
    sorted.sort_by(|left, right| (&left.kind, &left.key).cmp(&(&right.kind, &right.key)));
    if sorted
        .windows(2)
        .any(|pair| (&pair[0].kind, &pair[0].key) == (&pair[1].kind, &pair[1].key))
    {
        return Err(ThemeValueError::DuplicateToken);
    }
    Ok(sorted)
}

/// Encodes a canonical token list: a u16 count, then per token a kind byte, a u8-length key, a
/// u8-length color, and an isolated byte. Fails on a list that is not canonical.
pub fn encode_tokens(tokens: &[ThemeToken]) -> Result<Vec<u8>, ThemeValueError> {
    if canonical_tokens(tokens)? != tokens {
        return Err(ThemeValueError::DuplicateToken);
    }
    let mut writer = Writer::with_capacity(
        2 + tokens
            .iter()
            .map(|token| 4 + token.key.len() + token.value.len())
            .sum::<usize>(),
    );
    writer.u16(tokens.len() as u16);
    for token in tokens {
        writer.u8(kind_code(&token.kind).ok_or(ThemeValueError::TokenKind)?);
        writer.bytes_u8(token.key.as_bytes());
        writer.bytes_u8(token.value.as_bytes());
        writer.u8(u8::from(token.isolated));
    }
    Ok(writer.into_bytes())
}

/// Decodes a canonical token list, refusing any other encoding.
pub fn decode_tokens(bytes: &[u8]) -> Result<Vec<ThemeToken>, AdapterError> {
    const INVALID: AdapterError = AdapterError("theme tokens");
    let mut reader = Reader::new(bytes);
    let count = usize::from(reader.u16().map_err(|_| INVALID)?);
    if count > MAX_TOKENS {
        return Err(INVALID);
    }
    let mut tokens: Vec<ThemeToken> = Vec::with_capacity(count);
    for _ in 0..count {
        let kind = TOKEN_KINDS
            .get(usize::from(reader.u8().map_err(|_| INVALID)?))
            .ok_or(INVALID)?;
        let key = reader
            .bytes_u8(MAX_TOKEN_KEY_CHARS, "key")
            .map_err(|_| INVALID)?;
        let value = reader
            .bytes_u8(MAX_COLOR_CHARS, "color")
            .map_err(|_| INVALID)?;
        let isolated = match reader.u8().map_err(|_| INVALID)? {
            0 => false,
            1 => true,
            _ => return Err(INVALID),
        };
        let token = ThemeToken {
            kind: (*kind).to_owned(),
            key: std::str::from_utf8(key).map_err(|_| INVALID)?.to_owned(),
            value: std::str::from_utf8(value).map_err(|_| INVALID)?.to_owned(),
            isolated,
        };
        validate_token(&token).map_err(|_| INVALID)?;
        if tokens
            .last()
            .is_some_and(|previous| (&previous.kind, &previous.key) >= (&token.kind, &token.key))
        {
            return Err(INVALID);
        }
        tokens.push(token);
    }
    reader.finish().map_err(|_| INVALID)?;
    Ok(tokens)
}

/// Checks a palette given in slot order: exactly [`PALETTE_SIZE`] hex colors.
pub fn validate_palette(palette: &[String]) -> Result<(), ThemeValueError> {
    if palette.len() != PALETTE_SIZE {
        return Err(ThemeValueError::Palette);
    }
    if !palette.iter().all(|color| is_hex_color(color)) {
        return Err(ThemeValueError::Color);
    }
    Ok(())
}

/// Encodes a palette in slot order: a u8-length color per slot. Fails on a palette
/// [`validate_palette`] refuses.
pub fn encode_palette(palette: &[String]) -> Result<Vec<u8>, ThemeValueError> {
    validate_palette(palette)?;
    let mut writer =
        Writer::with_capacity(palette.iter().map(|color| 1 + color.len()).sum::<usize>());
    for color in palette {
        writer.bytes_u8(color.as_bytes());
    }
    Ok(writer.into_bytes())
}

/// Decodes a canonical palette in slot order, refusing any other encoding.
pub fn decode_palette(bytes: &[u8]) -> Result<Vec<String>, AdapterError> {
    const INVALID: AdapterError = AdapterError("theme palette");
    let mut reader = Reader::new(bytes);
    let mut palette = Vec::with_capacity(PALETTE_SIZE);
    for _ in 0..PALETTE_SIZE {
        let color = reader
            .bytes_u8(MAX_COLOR_CHARS, "color")
            .map_err(|_| INVALID)?;
        let color = std::str::from_utf8(color).map_err(|_| INVALID)?;
        if !is_hex_color(color) {
            return Err(INVALID);
        }
        palette.push(color.to_owned());
    }
    reader.finish().map_err(|_| INVALID)?;
    Ok(palette)
}

/// A theme's content group value: what the editor saves together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeContent {
    /// Canvas color event colors blend toward.
    pub blend_canvas: String,
    /// Derivation engine version the tokens were derived with.
    pub derivation_engine_version: i64,
    /// One of [`CALENDAR_MODES`].
    pub calendar_default_mode: String,
    /// Custom Calendar default color.
    pub calendar_default_custom: String,
    /// Tokens in canonical order.
    pub tokens: Vec<ThemeToken>,
    /// Event palette in slot order.
    pub palette: Vec<String>,
}

impl ThemeContent {
    /// Reads and checks a content or seed group value. Both hold four columns, then the token
    /// and palette blobs; the seed holds its icon label where content holds its engine version.
    pub fn from_value(value: &Value) -> Result<Self, AdapterError> {
        const INVALID: AdapterError = AdapterError("theme content");
        match value.fields() {
            [
                Field::Text(blend_canvas),
                Field::Integer(derivation_engine_version),
                Field::Text(calendar_default_mode),
                Field::Text(calendar_default_custom),
                Field::Blob(tokens),
                Field::Blob(palette),
            ] => {
                if !is_hex_color(blend_canvas)
                    || *derivation_engine_version < 0
                    || !is_calendar_mode(calendar_default_mode)
                    || !is_hex_color(calendar_default_custom)
                {
                    return Err(INVALID);
                }
                Ok(Self {
                    blend_canvas: blend_canvas.clone(),
                    derivation_engine_version: *derivation_engine_version,
                    calendar_default_mode: calendar_default_mode.clone(),
                    calendar_default_custom: calendar_default_custom.clone(),
                    tokens: decode_tokens(tokens)?,
                    palette: decode_palette(palette)?,
                })
            }
            _ => Err(INVALID),
        }
    }
}

fn validate_seed(value: &Value) -> Result<(), AdapterError> {
    const INVALID: AdapterError = AdapterError("theme seed");
    match value.fields() {
        [
            Field::Text(blend_canvas),
            Field::Text(icon_label),
            Field::Text(calendar_default_mode),
            Field::Text(calendar_default_custom),
            Field::Blob(tokens),
            Field::Blob(palette),
        ] if is_hex_color(blend_canvas)
            && is_icon_label(icon_label)
            && is_calendar_mode(calendar_default_mode)
            && is_hex_color(calendar_default_custom) =>
        {
            decode_tokens(tokens)?;
            decode_palette(palette)?;
            Ok(())
        }
        _ => Err(INVALID),
    }
}

fn token_from_fields(fields: Vec<Field>) -> Result<ThemeToken, AdapterError> {
    const INVALID: AdapterError = AdapterError("theme token");
    let mut fields = fields.into_iter();
    match (
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
    ) {
        (
            Some(Field::Text(kind)),
            Some(Field::Text(key)),
            Some(Field::Text(value)),
            Some(Field::Integer(isolated @ (0 | 1))),
            None,
        ) => Ok(ThemeToken {
            kind,
            key,
            value,
            isolated: isolated == 1,
        }),
        _ => Err(INVALID),
    }
}

fn token_fields(token: ThemeToken) -> Vec<Field> {
    vec![
        Field::Text(token.kind),
        Field::Text(token.key),
        Field::Text(token.value),
        Field::Integer(i64::from(token.isolated)),
    ]
}

/// Palette colors from owned rows, which must hold slots 0 to 31 in order.
fn palette_from_rows(rows: Vec<Vec<Field>>) -> Result<Vec<String>, AdapterError> {
    const INVALID: AdapterError = AdapterError("theme palette");
    rows.into_iter()
        .enumerate()
        .map(|(index, fields)| {
            let mut fields = fields.into_iter();
            match (fields.next(), fields.next(), fields.next()) {
                (Some(Field::Integer(slot)), Some(Field::Text(color)), None)
                    if usize::try_from(slot).ok() == Some(index) =>
                {
                    Ok(color)
                }
                _ => Err(INVALID),
            }
        })
        .collect()
}

fn palette_rows(palette: Vec<String>) -> Vec<Vec<Field>> {
    palette
        .into_iter()
        .enumerate()
        .map(|(slot, color)| vec![Field::Integer(slot as i64), Field::Text(color)])
        .collect()
}

fn is_tokens(owned: &str) -> bool {
    owned == TOKENS_OWNED || owned == SEED_TOKENS_OWNED
}

fn is_palette(owned: &str) -> bool {
    owned == PALETTE_OWNED || owned == SEED_PALETTE_OWNED
}

/// Token lists that reach the codec bounds: no tokens, and the largest list with every kind,
/// the longest keys, both color lengths, and both isolation flags.
fn token_samples() -> Vec<Vec<Vec<Field>>> {
    let full = (0..MAX_TOKENS)
        .map(|index| ThemeToken {
            kind: TOKEN_KINDS[index % TOKEN_KINDS.len()].to_owned(),
            key: format!("--k{index:0>width$}", width = MAX_TOKEN_KEY_CHARS - 3),
            value: if index % 2 == 0 {
                "#A1b2C3".to_owned()
            } else {
                "#a1b2c3d4".to_owned()
            },
            isolated: index % 3 == 0,
        })
        .collect::<Vec<_>>();
    let mut full = canonical_tokens(&full).unwrap_or_default();
    full.truncate(MAX_TOKENS);
    vec![Vec::new(), full.into_iter().map(token_fields).collect()]
}

/// Palettes in both color lengths.
fn palette_samples() -> Vec<Vec<Vec<Field>>> {
    let short = (0..PALETTE_SIZE)
        .map(|slot| format!("#{slot:06x}"))
        .collect();
    let long = (0..PALETTE_SIZE)
        .map(|slot| format!("#{slot:06X}ff"))
        .collect();
    vec![palette_rows(short), palette_rows(long)]
}

/// The user themes adapter.
#[derive(Debug, Clone, Copy, Default)]
pub struct ThemesAdapter;

impl DomainAdapter for ThemesAdapter {
    fn name(&self) -> &'static str {
        ADAPTER
    }

    fn encode_owned(
        &self,
        table: TableId,
        owned: &str,
        rows: Vec<Vec<Field>>,
    ) -> Result<Vec<u8>, AdapterError> {
        if table != THEMES_TABLE {
            return Err(AdapterError("owned table"));
        }
        if is_tokens(owned) {
            let tokens = rows
                .into_iter()
                .map(token_from_fields)
                .collect::<Result<Vec<_>, _>>()?;
            encode_tokens(&tokens).map_err(|_| AdapterError("theme tokens"))
        } else if is_palette(owned) {
            encode_palette(&palette_from_rows(rows)?).map_err(|_| AdapterError("theme palette"))
        } else {
            Err(AdapterError("owned table"))
        }
    }

    fn decode_owned(
        &self,
        table: TableId,
        owned: &str,
        bytes: &[u8],
    ) -> Result<Vec<Vec<Field>>, AdapterError> {
        if table != THEMES_TABLE {
            return Err(AdapterError("owned table"));
        }
        if is_tokens(owned) {
            Ok(decode_tokens(bytes)?
                .into_iter()
                .map(token_fields)
                .collect())
        } else if is_palette(owned) {
            Ok(palette_rows(decode_palette(bytes)?))
        } else {
            Err(AdapterError("owned table"))
        }
    }

    fn owned_samples(&self, table: TableId, owned: &str) -> Vec<Vec<Vec<Field>>> {
        if table != THEMES_TABLE {
            Vec::new()
        } else if is_tokens(owned) {
            token_samples()
        } else if is_palette(owned) {
            palette_samples()
        } else {
            Vec::new()
        }
    }

    fn derived(
        &self,
        _table: TableId,
        _group: GroupId,
        _value: &Value,
    ) -> Result<Vec<(&'static str, Field)>, AdapterError> {
        Ok(Vec::new())
    }

    fn priority(&self, _table: TableId, _group: GroupId, _value: &Value) -> u8 {
        0
    }

    fn validate(&self, table: TableId, group: GroupId, value: &Value) -> Result<(), AdapterError> {
        if table != THEMES_TABLE {
            return Ok(());
        }
        if group == group::CONTENT {
            ThemeContent::from_value(value)?;
        } else if group == group::SEED {
            validate_seed(value)?;
        } else if group == group::NAME {
            match value.fields() {
                [Field::Text(name)] if is_display_name(name) => {}
                _ => return Err(AdapterError("theme name")),
            }
        } else if group == group::ICON {
            match value.fields() {
                [Field::Text(icon)] if is_icon_label(icon) => {}
                _ => return Err(AdapterError("theme icon")),
            }
        }
        Ok(())
    }

    fn presentation(&self, table: TableId, values: &[(GroupId, Value)]) -> Presentation {
        let mut presentation = Presentation::default();
        if table != THEMES_TABLE {
            return presentation;
        }
        for (group, value) in values {
            if *group == group::NAME {
                if let [Field::Text(name)] = value.fields() {
                    presentation.title = name.clone();
                }
            }
        }
        presentation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(kind: &str, key: &str, value: &str, isolated: bool) -> ThemeToken {
        ThemeToken {
            kind: kind.to_owned(),
            key: key.to_owned(),
            value: value.to_owned(),
            isolated,
        }
    }

    fn palette() -> Vec<String> {
        (0..PALETTE_SIZE)
            .map(|slot| format!("#{slot:06x}"))
            .collect()
    }

    fn content_value(tokens: &[ThemeToken]) -> Value {
        Value::new(vec![
            Field::Text("#101010".to_owned()),
            Field::Integer(3),
            Field::Text("app-canvas".to_owned()),
            Field::Text("#27282A".to_owned()),
            Field::Blob(encode_tokens(tokens).unwrap()),
            Field::Blob(encode_palette(&palette()).unwrap()),
        ])
        .unwrap()
    }

    #[test]
    fn owned_samples_round_trip_byte_exactly() {
        for owned in [
            TOKENS_OWNED,
            PALETTE_OWNED,
            SEED_TOKENS_OWNED,
            SEED_PALETTE_OWNED,
        ] {
            let samples = ThemesAdapter.owned_samples(THEMES_TABLE, owned);
            assert!(samples.len() >= 2);
            for rows in samples {
                let bytes = ThemesAdapter
                    .encode_owned(THEMES_TABLE, owned, rows.clone())
                    .unwrap();
                let decoded = ThemesAdapter
                    .decode_owned(THEMES_TABLE, owned, &bytes)
                    .unwrap();
                assert_eq!(decoded, rows);
            }
        }
        let full = &token_samples()[1];
        assert_eq!(full.len(), MAX_TOKENS);
    }

    #[test]
    fn canonical_tokens_sort_by_kind_then_key_and_refuse_duplicates() {
        let tokens = vec![
            token("source", "canvas", "#000000", false),
            token("app", "--card", "#111111", true),
            token("calendar", "--cal-bg", "#222222", false),
            token("app", "--background", "#333333", false),
        ];
        let canonical = canonical_tokens(&tokens).unwrap();
        let order = canonical
            .iter()
            .map(|token| (token.kind.as_str(), token.key.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            order,
            [
                ("app", "--background"),
                ("app", "--card"),
                ("calendar", "--cal-bg"),
                ("source", "canvas"),
            ]
        );
        let mut duplicate = tokens.clone();
        duplicate.push(token("app", "--card", "#444444", false));
        assert_eq!(
            canonical_tokens(&duplicate),
            Err(ThemeValueError::DuplicateToken)
        );
    }

    #[test]
    fn encoding_refuses_lists_it_cannot_carry_unchanged() {
        let unsorted = vec![
            token("source", "canvas", "#000000", false),
            token("app", "--card", "#111111", false),
        ];
        assert!(encode_tokens(&unsorted).is_err());
        assert_eq!(
            encode_tokens(&[token("theme", "x", "#000000", false)]),
            Err(ThemeValueError::TokenKind)
        );
        assert_eq!(
            encode_tokens(&[token("app", "has space", "#000000", false)]),
            Err(ThemeValueError::TokenKey)
        );
        assert_eq!(
            encode_tokens(&[token(
                "app",
                &"k".repeat(MAX_TOKEN_KEY_CHARS + 1),
                "#000000",
                false
            )]),
            Err(ThemeValueError::TokenKey)
        );
        assert_eq!(
            encode_tokens(&[token("app", "--card", "red", false)]),
            Err(ThemeValueError::Color)
        );
        let mut short = palette();
        short.pop();
        assert_eq!(encode_palette(&short), Err(ThemeValueError::Palette));
        let mut rows = palette_rows(palette());
        rows.swap(0, 1);
        assert!(
            ThemesAdapter
                .encode_owned(THEMES_TABLE, PALETTE_OWNED, rows)
                .is_err()
        );
    }

    #[test]
    fn decoding_refuses_unsorted_bad_flags_and_trailing_bytes() {
        let sorted = vec![
            token("app", "--a", "#000000", false),
            token("app", "--b", "#000000", false),
        ];
        let mut bytes = encode_tokens(&sorted).unwrap();
        // Swap the two single-character key suffixes so the keys arrive out of order.
        let first = bytes.iter().position(|byte| *byte == b'a').unwrap();
        let second = bytes.iter().position(|byte| *byte == b'b').unwrap();
        bytes.swap(first, second);
        assert!(decode_tokens(&bytes).is_err());

        let mut flag = encode_tokens(&sorted).unwrap();
        let last = flag.len() - 1;
        flag[last] = 2;
        assert!(decode_tokens(&flag).is_err());

        let mut trailing = encode_palette(&palette()).unwrap();
        trailing.push(0);
        assert!(decode_palette(&trailing).is_err());
    }

    #[test]
    fn validation_mirrors_the_rows_the_theme_store_loads() {
        let tokens = vec![token("app", "--card", "#111111", true)];
        let content = content_value(&tokens);
        assert_eq!(
            ThemesAdapter.validate(THEMES_TABLE, group::CONTENT, &content),
            Ok(())
        );
        let read = ThemeContent::from_value(&content).unwrap();
        assert_eq!(read.tokens, tokens);
        assert_eq!(read.palette, palette());

        let mut fields = content.fields().to_vec();
        fields[2] = Field::Text("sepia".to_owned());
        let bad_mode = Value::new(fields).unwrap();
        assert!(
            ThemesAdapter
                .validate(THEMES_TABLE, group::CONTENT, &bad_mode)
                .is_err()
        );

        let blank = Value::new(vec![Field::Text("   ".to_owned())]).unwrap();
        assert!(
            ThemesAdapter
                .validate(THEMES_TABLE, group::NAME, &blank)
                .is_err()
        );
        let icon = Value::new(vec![Field::Text("dusk".to_owned())]).unwrap();
        assert!(
            ThemesAdapter
                .validate(THEMES_TABLE, group::ICON, &icon)
                .is_err()
        );
        let seed = Value::new(vec![
            Field::Text("#101010".to_owned()),
            Field::Text("dark".to_owned()),
            Field::Text("custom".to_owned()),
            Field::Text("#27282A".to_owned()),
            Field::Blob(encode_tokens(&tokens).unwrap()),
            Field::Blob(encode_palette(&palette()).unwrap()),
        ])
        .unwrap();
        assert_eq!(
            ThemesAdapter.validate(THEMES_TABLE, group::SEED, &seed),
            Ok(())
        );
    }

    #[test]
    fn presentation_titles_a_theme_by_its_name() {
        let name = Value::new(vec![Field::Text("Forest".to_owned())]).unwrap();
        let presentation = ThemesAdapter.presentation(THEMES_TABLE, &[(group::NAME, name)]);
        assert_eq!(presentation.title, "Forest");
        assert_eq!(presentation.preview, "");
    }
}
