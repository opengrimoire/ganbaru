//! User theme persistence independent of Tauri: current and seed tokens and event palettes, seed
//! resets, upgrade dismissals, and validation of every row written to or read from SQLite.
//! Built-in themes are not stored; their reserved ids are rejected.

use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use ganbaru_db::impl_sqlite_from_row;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteQueryResult;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

const PALETTE_SIZE: usize = 32;
const MAX_DISPLAY_NAME_LENGTH: usize = 60;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserThemeWrite {
    id: String,
    display_name: String,
    icon_label: String,
    seed_icon_label: String,
    blend_canvas: String,
    seed_blend_canvas: String,
    derivation_engine_version: i64,
    calendar_default_mode: String,
    calendar_default_custom: String,
    seed_calendar_default_mode: String,
    seed_calendar_default_custom: String,
    tokens: Vec<ThemeTokenWrite>,
    palette: Vec<ThemePaletteWrite>,
    seed_tokens: Vec<ThemeTokenWrite>,
    seed_palette: Vec<ThemePaletteWrite>,
}

#[derive(Deserialize)]
struct ThemeTokenWrite {
    kind: String,
    key: String,
    value: String,
    isolated: bool,
}

#[derive(Deserialize)]
struct ThemePaletteWrite {
    slot: i64,
    value: String,
}

#[derive(Serialize)]
pub struct DismissalRow {
    theme_id: String,
    engine_version: i64,
    dismissed_at_ms: i64,
}

#[derive(Serialize)]
pub struct ThemeRowRead {
    id: String,
    display_name: String,
    blend_canvas: String,
    seed_blend_canvas: String,
    derivation_engine_version: i64,
    calendar_default_mode: String,
    calendar_default_custom: String,
    seed_calendar_default_mode: String,
    seed_calendar_default_custom: String,
    icon_label: String,
    seed_icon_label: String,
    created_at_ms: i64,
    updated_at_ms: i64,
}
impl_sqlite_from_row!(ThemeRowRead {
    id,
    display_name,
    blend_canvas,
    seed_blend_canvas,
    derivation_engine_version,
    calendar_default_mode,
    calendar_default_custom,
    seed_calendar_default_mode,
    seed_calendar_default_custom,
    icon_label,
    seed_icon_label,
    created_at_ms,
    updated_at_ms,
});

#[derive(Serialize)]
pub struct TokenRowRead {
    theme_id: String,
    kind: String,
    key: String,
    value: String,
    isolated: i64,
}
impl_sqlite_from_row!(TokenRowRead {
    theme_id,
    kind,
    key,
    value,
    isolated,
});

#[derive(Serialize)]
pub struct PaletteRowRead {
    theme_id: String,
    slot: i64,
    value: String,
}
impl_sqlite_from_row!(PaletteRowRead {
    theme_id,
    slot,
    value,
});

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserThemeRead {
    theme: ThemeRowRead,
    tokens: Vec<TokenRowRead>,
    palette: Vec<PaletteRowRead>,
    seed_tokens: Vec<TokenRowRead>,
    seed_palette: Vec<PaletteRowRead>,
}

trait ThemeScoped {
    fn theme_id(&self) -> &str;
}

impl ThemeScoped for TokenRowRead {
    fn theme_id(&self) -> &str {
        &self.theme_id
    }
}

impl ThemeScoped for PaletteRowRead {
    fn theme_id(&self) -> &str {
        &self.theme_id
    }
}

/// Load every user theme with its current and seed rows, rejecting stored rows that fail
/// validation.
pub async fn load_all(pool: &SqlitePool) -> Result<Vec<UserThemeRead>, String> {
    let themes = sqlx::query_as::<_, ThemeRowRead>(
        "SELECT id, display_name, blend_canvas, seed_blend_canvas,
                derivation_engine_version, calendar_default_mode,
                calendar_default_custom, seed_calendar_default_mode,
                seed_calendar_default_custom, icon_label, seed_icon_label,
                created_at_ms, updated_at_ms
         FROM themes
         ORDER BY created_at_ms ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load themes: {e}"))?;
    if themes.is_empty() {
        return Ok(Vec::new());
    }

    let tokens = sqlx::query_as::<_, TokenRowRead>(
        "SELECT theme_id, kind, key, value, isolated
         FROM theme_tokens
         ORDER BY theme_id ASC, kind ASC, key ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load theme tokens: {e}"))?;
    let palette = sqlx::query_as::<_, PaletteRowRead>(
        "SELECT theme_id, slot, value
         FROM theme_event_palette
         ORDER BY theme_id ASC, slot ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load theme palette: {e}"))?;
    let seed_tokens = sqlx::query_as::<_, TokenRowRead>(
        "SELECT theme_id, kind, key, value, isolated
         FROM theme_seed_tokens
         ORDER BY theme_id ASC, kind ASC, key ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load seed theme tokens: {e}"))?;
    let seed_palette = sqlx::query_as::<_, PaletteRowRead>(
        "SELECT theme_id, slot, value
         FROM theme_seed_event_palette
         ORDER BY theme_id ASC, slot ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load seed theme palette: {e}"))?;

    validate_theme_rows(&themes, &tokens, &palette, &seed_tokens, &seed_palette)?;

    let mut tokens_by_theme = group_by_theme(tokens);
    let mut palette_by_theme = group_by_theme(palette);
    let mut seed_tokens_by_theme = group_by_theme(seed_tokens);
    let mut seed_palette_by_theme = group_by_theme(seed_palette);

    Ok(themes
        .into_iter()
        .map(|theme| {
            let id = theme.id.clone();
            UserThemeRead {
                theme,
                tokens: tokens_by_theme.remove(&id).unwrap_or_default(),
                palette: palette_by_theme.remove(&id).unwrap_or_default(),
                seed_tokens: seed_tokens_by_theme.remove(&id).unwrap_or_default(),
                seed_palette: seed_palette_by_theme.remove(&id).unwrap_or_default(),
            }
        })
        .collect())
}

/// Insert a new user theme with its current and seed rows in one transaction. Fails when the id
/// is already taken.
pub async fn insert(pool: &SqlitePool, write: &UserThemeWrite) -> Result<(), String> {
    validate_theme_write(write)?;
    let now = now_ms()?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;

    let existing = sqlx::query_scalar::<_, i64>("SELECT 1 FROM themes WHERE id = ? LIMIT 1")
        .bind(&write.id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("check theme id availability: {e}"))?;
    if existing.is_some() {
        return Err(format!("theme id '{}' already exists", write.id));
    }

    sqlx::query(
        "INSERT INTO themes
            (id, display_name, icon_label, seed_icon_label, blend_canvas,
             seed_blend_canvas, derivation_engine_version, calendar_default_mode,
             calendar_default_custom, seed_calendar_default_mode,
             seed_calendar_default_custom, created_at_ms, updated_at_ms)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&write.id)
    .bind(&write.display_name)
    .bind(&write.icon_label)
    .bind(&write.seed_icon_label)
    .bind(&write.blend_canvas)
    .bind(&write.seed_blend_canvas)
    .bind(write.derivation_engine_version)
    .bind(&write.calendar_default_mode)
    .bind(&write.calendar_default_custom)
    .bind(&write.seed_calendar_default_mode)
    .bind(&write.seed_calendar_default_custom)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        let message = e.to_string();
        if message.contains("UNIQUE constraint failed: themes.id") {
            format!("theme id '{}' already exists", write.id)
        } else {
            format!("insert theme: {e}")
        }
    })?;

    insert_token_rows(
        &mut tx,
        &write.id,
        &write.tokens,
        "INSERT INTO theme_tokens (theme_id, kind, key, value, isolated) VALUES (?, ?, ?, ?, ?)",
    )
    .await?;
    insert_palette_rows(
        &mut tx,
        &write.id,
        &write.palette,
        "INSERT INTO theme_event_palette (theme_id, slot, value) VALUES (?, ?, ?)",
    )
    .await?;
    insert_token_rows(
        &mut tx,
        &write.id,
        &write.seed_tokens,
        "INSERT INTO theme_seed_tokens (theme_id, kind, key, value, isolated) VALUES (?, ?, ?, ?, ?)",
    )
    .await?;
    insert_palette_rows(
        &mut tx,
        &write.id,
        &write.seed_palette,
        "INSERT INTO theme_seed_event_palette (theme_id, slot, value) VALUES (?, ?, ?)",
    )
    .await?;

    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    Ok(())
}

/// Replace an existing theme's metadata and every current and seed row in one transaction. Fails
/// when the theme does not exist.
pub async fn replace_content(pool: &SqlitePool, write: &UserThemeWrite) -> Result<(), String> {
    validate_theme_write(write)?;
    let now = now_ms()?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;

    let result = sqlx::query(
        "UPDATE themes
            SET display_name = ?,
                icon_label = ?,
                seed_icon_label = ?,
                blend_canvas = ?,
                seed_blend_canvas = ?,
                derivation_engine_version = ?,
                calendar_default_mode = ?,
                calendar_default_custom = ?,
                seed_calendar_default_mode = ?,
                seed_calendar_default_custom = ?,
                updated_at_ms = ?
          WHERE id = ?",
    )
    .bind(&write.display_name)
    .bind(&write.icon_label)
    .bind(&write.seed_icon_label)
    .bind(&write.blend_canvas)
    .bind(&write.seed_blend_canvas)
    .bind(write.derivation_engine_version)
    .bind(&write.calendar_default_mode)
    .bind(&write.calendar_default_custom)
    .bind(&write.seed_calendar_default_mode)
    .bind(&write.seed_calendar_default_custom)
    .bind(now)
    .bind(&write.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update theme: {e}"))?;

    if result.rows_affected() == 0 {
        return Err(format!("theme '{}' not found", write.id));
    }

    delete_theme_children(&mut tx, &write.id).await?;
    insert_token_rows(
        &mut tx,
        &write.id,
        &write.tokens,
        "INSERT INTO theme_tokens (theme_id, kind, key, value, isolated) VALUES (?, ?, ?, ?, ?)",
    )
    .await?;
    insert_palette_rows(
        &mut tx,
        &write.id,
        &write.palette,
        "INSERT INTO theme_event_palette (theme_id, slot, value) VALUES (?, ?, ?)",
    )
    .await?;
    insert_token_rows(
        &mut tx,
        &write.id,
        &write.seed_tokens,
        "INSERT INTO theme_seed_tokens (theme_id, kind, key, value, isolated) VALUES (?, ?, ?, ?, ?)",
    )
    .await?;
    insert_palette_rows(
        &mut tx,
        &write.id,
        &write.seed_palette,
        "INSERT INTO theme_seed_event_palette (theme_id, slot, value) VALUES (?, ?, ?)",
    )
    .await?;

    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    Ok(())
}

/// Delete a user theme; its rows and upgrade dismissals cascade. Deleting a missing theme
/// succeeds.
pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), String> {
    validate_theme_id(id)?;
    sqlx::query("DELETE FROM themes WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("delete theme: {e}"))?;
    Ok(())
}

/// Record that the user dismissed the upgrade of a theme to a derivation engine version,
/// replacing an earlier dismissal of the same version.
pub async fn record_dismissal(
    pool: &SqlitePool,
    id: &str,
    engine_version: i64,
) -> Result<(), String> {
    validate_theme_id(id)?;
    if engine_version < 0 {
        return Err("engine_version cannot be negative".to_string());
    }
    let dismissed_at_ms = now_ms()?;
    sqlx::query(
        "INSERT OR REPLACE INTO theme_upgrade_dismissals
            (theme_id, engine_version, dismissed_at_ms)
         VALUES (?, ?, ?)",
    )
    .bind(id)
    .bind(engine_version)
    .bind(dismissed_at_ms)
    .execute(pool)
    .await
    .map_err(|e| format!("record theme dismissal: {e}"))?;
    Ok(())
}

/// Load every recorded theme upgrade dismissal.
pub async fn load_dismissals(pool: &SqlitePool) -> Result<Vec<DismissalRow>, String> {
    let rows = sqlx::query(
        "SELECT theme_id, engine_version, dismissed_at_ms FROM theme_upgrade_dismissals",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load theme dismissals: {e}"))?;

    rows.into_iter()
        .map(|row| {
            Ok(DismissalRow {
                theme_id: row
                    .try_get("theme_id")
                    .map_err(|e| format!("read dismissal theme_id: {e}"))?,
                engine_version: row
                    .try_get("engine_version")
                    .map_err(|e| format!("read dismissal engine_version: {e}"))?,
                dismissed_at_ms: row
                    .try_get("dismissed_at_ms")
                    .map_err(|e| format!("read dismissal dismissed_at_ms: {e}"))?,
            })
        })
        .collect()
}

/// Rename a user theme. Fails when the theme does not exist.
pub async fn rename(pool: &SqlitePool, id: &str, display_name: &str) -> Result<(), String> {
    validate_theme_id(id)?;
    validate_display_name(display_name)?;
    let now = now_ms()?;
    let result = sqlx::query("UPDATE themes SET display_name = ?, updated_at_ms = ? WHERE id = ?")
        .bind(display_name)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("rename theme: {e}"))?;
    ensure_row_changed(result, "rename theme")
}

/// Restore one current token's value and isolation from the theme's seed. Fails when the theme
/// has no such token.
pub async fn reset_token_to_seed(
    pool: &SqlitePool,
    id: &str,
    kind: &str,
    key: &str,
) -> Result<(), String> {
    validate_theme_id(id)?;
    validate_token_identity(kind, key, "token")?;
    let now = now_ms()?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    let result = sqlx::query(
        "UPDATE theme_tokens AS t
            SET value = (
                    SELECT value FROM theme_seed_tokens AS s
                    WHERE s.theme_id = t.theme_id AND s.kind = t.kind AND s.key = t.key
                ),
                isolated = (
                    SELECT isolated FROM theme_seed_tokens AS s
                    WHERE s.theme_id = t.theme_id AND s.kind = t.kind AND s.key = t.key
                )
          WHERE t.theme_id = ? AND t.kind = ? AND t.key = ?",
    )
    .bind(id)
    .bind(kind)
    .bind(key)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("reset theme token to seed: {e}"))?;
    ensure_row_changed(result, "reset theme token to seed")?;
    touch_theme(&mut tx, id, now).await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    Ok(())
}

/// Restore every current token, palette slot, blend canvas, and Calendar default from the theme's
/// seed. The display name and icon label are kept. Fails when the theme does not exist.
pub async fn reset_to_seed(pool: &SqlitePool, id: &str) -> Result<(), String> {
    validate_theme_id(id)?;
    let now = now_ms()?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;

    sqlx::query(
        "UPDATE theme_tokens AS t
            SET value = (
                    SELECT value FROM theme_seed_tokens AS s
                    WHERE s.theme_id = t.theme_id AND s.kind = t.kind AND s.key = t.key
                ),
                isolated = (
                    SELECT isolated FROM theme_seed_tokens AS s
                    WHERE s.theme_id = t.theme_id AND s.kind = t.kind AND s.key = t.key
                )
          WHERE t.theme_id = ?",
    )
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("reset theme tokens to seed: {e}"))?;

    sqlx::query(
        "UPDATE theme_event_palette AS p
            SET value = (
                SELECT value FROM theme_seed_event_palette AS s
                WHERE s.theme_id = p.theme_id AND s.slot = p.slot
            )
          WHERE p.theme_id = ?",
    )
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("reset theme palette to seed: {e}"))?;

    let result = sqlx::query(
        "UPDATE themes
            SET blend_canvas = seed_blend_canvas,
                calendar_default_mode = seed_calendar_default_mode,
                calendar_default_custom = seed_calendar_default_custom,
                updated_at_ms = ?
          WHERE id = ?",
    )
    .bind(now)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("reset theme to seed: {e}"))?;
    ensure_row_changed(result, "reset theme to seed")?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    Ok(())
}

async fn delete_theme_children(
    tx: &mut Transaction<'_, Sqlite>,
    theme_id: &str,
) -> Result<(), String> {
    for query in [
        "DELETE FROM theme_tokens WHERE theme_id = ?",
        "DELETE FROM theme_event_palette WHERE theme_id = ?",
        "DELETE FROM theme_seed_tokens WHERE theme_id = ?",
        "DELETE FROM theme_seed_event_palette WHERE theme_id = ?",
    ] {
        sqlx::query(query)
            .bind(theme_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("delete theme children: {e}"))?;
    }
    Ok(())
}

async fn insert_token_rows(
    tx: &mut Transaction<'_, Sqlite>,
    theme_id: &str,
    rows: &[ThemeTokenWrite],
    query: &'static str,
) -> Result<(), String> {
    for row in rows {
        sqlx::query(query)
            .bind(theme_id)
            .bind(&row.kind)
            .bind(&row.key)
            .bind(&row.value)
            .bind(if row.isolated { 1_i64 } else { 0_i64 })
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("insert theme token '{}:{}': {e}", row.kind, row.key))?;
    }
    Ok(())
}

async fn insert_palette_rows(
    tx: &mut Transaction<'_, Sqlite>,
    theme_id: &str,
    rows: &[ThemePaletteWrite],
    query: &'static str,
) -> Result<(), String> {
    for row in rows {
        sqlx::query(query)
            .bind(theme_id)
            .bind(row.slot)
            .bind(&row.value)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("insert theme palette slot {}: {e}", row.slot))?;
    }
    Ok(())
}

async fn touch_theme(
    tx: &mut Transaction<'_, Sqlite>,
    theme_id: &str,
    now: i64,
) -> Result<(), String> {
    let result = sqlx::query("UPDATE themes SET updated_at_ms = ? WHERE id = ?")
        .bind(now)
        .bind(theme_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("touch theme: {e}"))?;
    ensure_row_changed(result, "touch theme")
}

fn ensure_row_changed(result: SqliteQueryResult, context: &str) -> Result<(), String> {
    if result.rows_affected() == 0 {
        Err(format!("{context}: no matching row"))
    } else {
        Ok(())
    }
}

fn group_by_theme<T: ThemeScoped>(rows: Vec<T>) -> HashMap<String, Vec<T>> {
    let mut grouped = HashMap::new();
    for row in rows {
        grouped
            .entry(row.theme_id().to_string())
            .or_insert_with(Vec::new)
            .push(row);
    }
    grouped
}

fn validate_theme_rows(
    themes: &[ThemeRowRead],
    tokens: &[TokenRowRead],
    palette: &[PaletteRowRead],
    seed_tokens: &[TokenRowRead],
    seed_palette: &[PaletteRowRead],
) -> Result<(), String> {
    for theme in themes {
        validate_theme_read(theme)?;
    }
    validate_token_read_rows(tokens, "theme_tokens")?;
    validate_palette_read_rows(palette, "theme_event_palette")?;
    validate_token_read_rows(seed_tokens, "theme_seed_tokens")?;
    validate_palette_read_rows(seed_palette, "theme_seed_event_palette")
}

fn validate_theme_read(theme: &ThemeRowRead) -> Result<(), String> {
    validate_theme_id(&theme.id)?;
    validate_display_name(&theme.display_name)?;
    validate_hex_color(&theme.blend_canvas, "blend_canvas")?;
    validate_hex_color(&theme.seed_blend_canvas, "seed_blend_canvas")?;
    if theme.derivation_engine_version < 0 {
        return Err("derivation_engine_version cannot be negative".to_string());
    }
    validate_calendar_mode(&theme.calendar_default_mode, "calendar_default_mode")?;
    validate_calendar_mode(
        &theme.seed_calendar_default_mode,
        "seed_calendar_default_mode",
    )?;
    validate_hex_color(&theme.calendar_default_custom, "calendar_default_custom")?;
    validate_hex_color(
        &theme.seed_calendar_default_custom,
        "seed_calendar_default_custom",
    )?;
    validate_icon_label(&theme.icon_label, "icon_label")?;
    validate_icon_label(&theme.seed_icon_label, "seed_icon_label")
}

fn validate_token_read_rows(rows: &[TokenRowRead], field: &str) -> Result<(), String> {
    for row in rows {
        validate_theme_id(&row.theme_id)?;
        validate_token_identity(&row.kind, &row.key, field)?;
        validate_hex_color(&row.value, field)?;
        if !matches!(row.isolated, 0 | 1) {
            return Err(format!("{field} isolated must be 0 or 1"));
        }
    }
    Ok(())
}

fn validate_palette_read_rows(rows: &[PaletteRowRead], field: &str) -> Result<(), String> {
    for row in rows {
        validate_theme_id(&row.theme_id)?;
        validate_palette_slot(row.slot, field)?;
        validate_hex_color(&row.value, field)?;
    }
    Ok(())
}

fn validate_theme_write(write: &UserThemeWrite) -> Result<(), String> {
    validate_theme_id(&write.id)?;
    validate_display_name(&write.display_name)?;
    validate_icon_label(&write.icon_label, "icon_label")?;
    validate_icon_label(&write.seed_icon_label, "seed_icon_label")?;
    validate_hex_color(&write.blend_canvas, "blend_canvas")?;
    validate_hex_color(&write.seed_blend_canvas, "seed_blend_canvas")?;
    validate_calendar_mode(&write.calendar_default_mode, "calendar_default_mode")?;
    validate_calendar_mode(
        &write.seed_calendar_default_mode,
        "seed_calendar_default_mode",
    )?;
    validate_hex_color(&write.calendar_default_custom, "calendar_default_custom")?;
    validate_hex_color(
        &write.seed_calendar_default_custom,
        "seed_calendar_default_custom",
    )?;
    if write.derivation_engine_version < 0 {
        return Err("derivation_engine_version cannot be negative".to_string());
    }
    validate_token_rows(&write.tokens, "tokens")?;
    validate_palette_rows(&write.palette, "palette")?;
    validate_token_rows(&write.seed_tokens, "seed_tokens")?;
    validate_palette_rows(&write.seed_palette, "seed_palette")?;
    Ok(())
}

fn validate_display_name(value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err("display_name cannot be empty".to_string());
    }
    if value.chars().count() > MAX_DISPLAY_NAME_LENGTH {
        return Err(format!(
            "display_name cannot exceed {MAX_DISPLAY_NAME_LENGTH} characters"
        ));
    }
    Ok(())
}

fn validate_theme_id(id: &str) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err("theme id cannot be empty".to_string());
    }
    if !is_valid_theme_slug(id) {
        return Err(
            "theme id must be a slug (lowercase letters, digits, and hyphens; must start with a letter or digit)"
                .to_string(),
        );
    }
    if matches!(id, "light" | "dark") {
        return Err(format!("theme id '{id}' is reserved"));
    }
    Ok(())
}

fn is_valid_theme_slug(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn validate_token_identity(kind: &str, key: &str, field: &str) -> Result<(), String> {
    if !matches!(kind, "source" | "app" | "calendar") {
        return Err(format!("{field} contains invalid token kind '{kind}'"));
    }
    if key.trim().is_empty() {
        return Err(format!("{field} contains an empty token key"));
    }
    Ok(())
}

fn validate_icon_label(value: &str, field: &str) -> Result<(), String> {
    if matches!(value, "light" | "dark") {
        Ok(())
    } else {
        Err(format!("{field} must be 'light' or 'dark'"))
    }
}

fn validate_calendar_mode(value: &str, field: &str) -> Result<(), String> {
    if matches!(value, "light" | "dark" | "app-canvas" | "custom") {
        Ok(())
    } else {
        Err(format!(
            "{field} must be 'light', 'dark', 'app-canvas', or 'custom'"
        ))
    }
}

fn validate_token_rows(rows: &[ThemeTokenWrite], field: &str) -> Result<(), String> {
    let mut seen = HashSet::with_capacity(rows.len());
    for row in rows {
        if !matches!(row.kind.as_str(), "source" | "app" | "calendar") {
            return Err(format!(
                "{field} contains invalid token kind '{}'",
                row.kind
            ));
        }
        if row.key.trim().is_empty() {
            return Err(format!("{field} contains an empty token key"));
        }
        validate_hex_color(&row.value, field)?;
        if !seen.insert((row.kind.as_str(), row.key.as_str())) {
            return Err(format!(
                "{field} contains duplicate token '{}:{}'",
                row.kind, row.key
            ));
        }
    }
    Ok(())
}

fn validate_palette_rows(rows: &[ThemePaletteWrite], field: &str) -> Result<(), String> {
    if rows.len() != PALETTE_SIZE {
        return Err(format!(
            "{field} must contain exactly {PALETTE_SIZE} entries"
        ));
    }
    let mut seen = HashSet::with_capacity(rows.len());
    for row in rows {
        validate_palette_slot(row.slot, field)?;
        validate_hex_color(&row.value, field)?;
        if !seen.insert(row.slot) {
            return Err(format!("{field} contains duplicate slot {}", row.slot));
        }
    }
    Ok(())
}

fn validate_palette_slot(slot: i64, field: &str) -> Result<(), String> {
    if slot < 0 || slot >= PALETTE_SIZE as i64 {
        return Err(format!("{field} contains invalid slot {slot}"));
    }
    Ok(())
}

fn validate_hex_color(value: &str, field: &str) -> Result<(), String> {
    if is_hex_color(value) {
        Ok(())
    } else {
        Err(format!("{field} must be a 6 or 8 digit hex color"))
    }
}

fn is_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.first() != Some(&b'#') {
        return false;
    }
    if bytes.len() != 7 && bytes.len() != 9 {
        return false;
    }
    bytes[1..].iter().all(u8::is_ascii_hexdigit)
}

fn now_ms() -> Result<i64, String> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("system clock before unix epoch: {e}"))?;
    Ok(duration.as_millis() as i64)
}

#[cfg(test)]
mod tests;
