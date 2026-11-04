//! Restoring a theme that a linked device deleted while another device still edited it.

use std::collections::HashSet;

use ganbaru_sync::domains::themes::{ThemeContent, ThemeToken, is_display_name};
use ganbaru_sync::manifest::vault::themes::group;
use ganbaru_sync_contracts::{Field, GroupId, Value};
use ring::rand::{SecureRandom, SystemRandom};
use sqlx::SqliteConnection;

use super::{ThemePaletteWrite, ThemeTokenWrite, UserThemeWrite, insert_on};

/// Exclusive upper bound of the numeric suffix tried for a unique display name, matching the
/// frontend rule for duplicated and imported themes.
const NAME_SUFFIX_CAP: u32 = 999;

/// `base` when no other theme uses it, ignoring case, or else `base` followed by the lowest free
/// numeric suffix from 2, as the frontend names duplicated and imported themes. A suffixed name
/// that would exceed the display name limit is skipped; `base` is kept when no suffix fits.
fn unique_display_name<'a>(base: &str, existing: impl IntoIterator<Item = &'a str>) -> String {
    let taken: HashSet<String> = existing.into_iter().map(str::to_lowercase).collect();
    if !taken.contains(&base.to_lowercase()) {
        return base.to_string();
    }
    (2..NAME_SUFFIX_CAP)
        .map(|suffix| format!("{base} {suffix}"))
        .find(|candidate| is_display_name(candidate) && !taken.contains(&candidate.to_lowercase()))
        .unwrap_or_else(|| base.to_string())
}

/// The display names of every stored theme.
async fn stored_display_names(conn: &mut SqliteConnection) -> Result<Vec<String>, String> {
    sqlx::query_scalar::<_, String>("SELECT display_name FROM themes")
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| format!("read theme names: {e}"))
}

/// A random version 4 UUID, the form the frontend gives new user themes.
fn new_theme_id() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "secure random generator is unavailable".to_string())?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

fn token_writes(tokens: &[ThemeToken]) -> Vec<ThemeTokenWrite> {
    tokens
        .iter()
        .map(|token| ThemeTokenWrite {
            kind: token.kind.clone(),
            key: token.key.clone(),
            value: token.value.clone(),
            isolated: token.isolated,
        })
        .collect()
}

fn palette_writes(palette: &[String]) -> Vec<ThemePaletteWrite> {
    (0_i64..)
        .zip(palette)
        .map(|(slot, value)| ThemePaletteWrite {
            slot,
            value: value.clone(),
        })
        .collect()
}

/// Creates a new theme from the retained values of a deleted theme offered for recovery and
/// returns its id. The recovered content also becomes the new theme's seed, as it does for an
/// imported theme, so the restored theme starts with a seed of its own, and its name gets a
/// numeric suffix when another theme already uses it. Inside an open
/// transaction on `conn` the insert joins it as a savepoint, so the caller decides when it
/// commits.
pub async fn restore_recovered(
    conn: &mut SqliteConnection,
    values: &[(GroupId, Value)],
) -> Result<String, String> {
    let mut display_name = None;
    let mut icon_label = None;
    let mut content = None;
    for (group_id, value) in values {
        match (*group_id, value.fields()) {
            (group::NAME, [Field::Text(name)]) => display_name = Some(name.clone()),
            (group::ICON, [Field::Text(icon)]) => icon_label = Some(icon.clone()),
            (group::CONTENT, _) => {
                content = Some(ThemeContent::from_value(value).map_err(|e| e.to_string())?);
            }
            _ => {}
        }
    }
    let display_name = display_name.ok_or("a recovered theme has no name")?;
    let existing = stored_display_names(conn).await?;
    let display_name = unique_display_name(&display_name, existing.iter().map(String::as_str));
    let icon_label = icon_label.ok_or("a recovered theme has no icon")?;
    let content = content.ok_or("a recovered theme has no content")?;
    let tokens = token_writes(&content.tokens);
    let palette = palette_writes(&content.palette);
    let write = UserThemeWrite {
        id: new_theme_id()?,
        display_name,
        seed_icon_label: icon_label.clone(),
        icon_label,
        seed_blend_canvas: content.blend_canvas.clone(),
        blend_canvas: content.blend_canvas,
        derivation_engine_version: content.derivation_engine_version,
        seed_calendar_default_mode: content.calendar_default_mode.clone(),
        calendar_default_mode: content.calendar_default_mode,
        seed_calendar_default_custom: content.calendar_default_custom.clone(),
        calendar_default_custom: content.calendar_default_custom,
        seed_tokens: token_writes(&content.tokens),
        tokens,
        seed_palette: palette_writes(&content.palette),
        palette,
    };
    insert_on(conn, &write).await?;
    Ok(write.id)
}
