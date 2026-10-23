//! User theme persistence independent of Tauri: current and seed tokens and event palettes, seed
//! resets, upgrade dismissals, sync conflicts on theme content, and validation of every row
//! written to or read from SQLite. Built-in themes are not stored; their reserved ids are rejected.
//!
//! A theme's seed is written once when the theme is created and never changes afterwards, so
//! every linked device resets to the same values. Writes leave rows that already hold the
//! requested values untouched, so a save that changes nothing replicates nothing.

use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use ganbaru_db::impl_sqlite_from_row;
use ganbaru_sync::OPEN_CONFLICT_MASK_SQL;
use ganbaru_sync::domains::themes::{
    PALETTE_SIZE, ThemeContent, ThemeToken, canonical_tokens, is_calendar_mode, is_display_name,
    is_hex_color, is_icon_label, is_token_key,
};
use ganbaru_sync::manifest::vault::themes::THEMES_TABLE;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteQueryResult;
use sqlx::{Connection, Row, Sqlite, SqliteConnection, SqlitePool, Transaction};

mod conflicts;
mod recovery;

pub use conflicts::{
    ThemeConflictError, ThemeConflictRead, ThemeConflictResolution, conflict_from_pool,
    resolve_from_pool,
};
pub use recovery::restore_recovered;

/// Characters of the longest theme id: a UUID or an older `theme-` slug fits well below it.
const MAX_THEME_ID_CHARS: usize = 64;

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

/// The highest derivation engine version whose upgrade prompt was dismissed for a theme.
#[derive(Serialize)]
pub struct DismissalRow {
    theme_id: String,
    engine_version: i64,
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
    /// Whether concurrent saves on linked devices left a content conflict nobody resolved.
    has_conflict: bool,
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
    has_conflict,
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

/// Column `has_conflict` of a theme aliased `t`.
fn has_conflict_column() -> String {
    format!(
        "EXISTS (SELECT 1 FROM sync_rows r
                 WHERE r.table_id = {} AND r.row_key = t.id AND {OPEN_CONFLICT_MASK_SQL} <> 0)
         AS has_conflict",
        THEMES_TABLE.0
    )
}

/// Load every user theme with its current and seed rows, rejecting stored rows that fail
/// validation.
pub async fn load_all(pool: &SqlitePool) -> Result<Vec<UserThemeRead>, String> {
    let themes = sqlx::query_as::<_, ThemeRowRead>(sqlx::AssertSqlSafe(format!(
        "SELECT t.id, t.display_name, t.blend_canvas, t.seed_blend_canvas,
                t.derivation_engine_version, t.calendar_default_mode,
                t.calendar_default_custom, t.seed_calendar_default_mode,
                t.seed_calendar_default_custom, t.icon_label, t.seed_icon_label,
                t.created_at_ms, t.updated_at_ms, {}
         FROM themes t
         ORDER BY t.created_at_ms ASC, t.id ASC",
        has_conflict_column()
    )))
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

/// Load one user theme, failing when it does not exist.
pub async fn load_one(pool: &SqlitePool, id: &str) -> Result<UserThemeRead, String> {
    load_all(pool)
        .await?
        .into_iter()
        .find(|theme| theme.theme.id == id)
        .ok_or_else(|| format!("theme '{id}' not found"))
}

/// Insert a new user theme with its current and seed rows in one transaction. Fails when the id
/// is already taken, including by a theme a linked device deleted.
pub async fn insert(pool: &SqlitePool, write: &UserThemeWrite) -> Result<(), String> {
    let mut conn = pool.acquire().await.map_err(|e| format!("acquire: {e}"))?;
    insert_on(&mut conn, write).await
}

/// Insert a new user theme on `conn`. Inside an open transaction the insert joins it as a
/// savepoint, so the caller decides when it commits.
async fn insert_on(conn: &mut SqliteConnection, write: &UserThemeWrite) -> Result<(), String> {
    validate_theme_write(write)?;
    let now = now_ms()?;
    let mut tx = conn.begin().await.map_err(|e| format!("begin: {e}"))?;

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

    insert_tokens(&mut tx, &write.id, &write_tokens(&write.tokens), TOKENS).await?;
    insert_palette(&mut tx, &write.id, &write_palette(&write.palette), PALETTE).await?;
    insert_tokens(
        &mut tx,
        &write.id,
        &write_tokens(&write.seed_tokens),
        SEED_TOKENS,
    )
    .await?;
    insert_palette(
        &mut tx,
        &write.id,
        &write_palette(&write.seed_palette),
        SEED_PALETTE,
    )
    .await?;

    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    Ok(())
}

/// Replace an existing theme's name, icon, and content (blend canvas, engine version, Calendar
/// default, tokens, and palette) in one transaction. The seed fields of `write` are ignored,
/// since a theme's seed never changes after creation. Fails when the theme does not exist.
pub async fn replace_content(pool: &SqlitePool, write: &UserThemeWrite) -> Result<(), String> {
    validate_theme_write(write)?;
    let content = write_content(write)?;
    let now = now_ms()?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;

    let labeled = sqlx::query(
        "UPDATE themes SET display_name = ?, icon_label = ?
          WHERE id = ? AND (display_name IS NOT ? OR icon_label IS NOT ?)",
    )
    .bind(&write.display_name)
    .bind(&write.icon_label)
    .bind(&write.id)
    .bind(&write.display_name)
    .bind(&write.icon_label)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update theme: {e}"))?
    .rows_affected()
        > 0;
    let changed = write_theme_content(&mut tx, &write.id, &content).await?;
    if labeled || changed {
        touch_theme(&mut tx, &write.id, now).await?;
    }

    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    Ok(())
}

/// Delete a user theme; its rows cascade. Deleting a missing theme succeeds.
pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), String> {
    validate_theme_id(id)?;
    sqlx::query("DELETE FROM themes WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("delete theme: {e}"))?;
    Ok(())
}

/// Record that the user dismissed the upgrade of a theme to a derivation engine version. A theme
/// keeps only its highest dismissed version, so dismissing a lower one changes nothing. Fails
/// when the theme does not exist.
pub async fn record_dismissal(
    pool: &SqlitePool,
    id: &str,
    engine_version: i64,
) -> Result<(), String> {
    validate_theme_id(id)?;
    if engine_version < 0 {
        return Err("engine_version cannot be negative".to_string());
    }
    let mut conn = pool.acquire().await.map_err(|e| format!("acquire: {e}"))?;
    let result = sqlx::query(
        "UPDATE themes SET upgrade_dismissed_engine_version = ?
          WHERE id = ? AND upgrade_dismissed_engine_version < ?",
    )
    .bind(engine_version)
    .bind(id)
    .bind(engine_version)
    .execute(&mut *conn)
    .await
    .map_err(|e| format!("record theme dismissal: {e}"))?;
    if result.rows_affected() == 0 {
        ensure_theme_exists(&mut conn, id, "record theme dismissal").await?;
    }
    Ok(())
}

/// Load the highest dismissed upgrade of every theme that has one.
pub async fn load_dismissals(pool: &SqlitePool) -> Result<Vec<DismissalRow>, String> {
    let rows = sqlx::query(
        "SELECT id, upgrade_dismissed_engine_version FROM themes
          WHERE upgrade_dismissed_engine_version > 0",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load theme dismissals: {e}"))?;

    rows.into_iter()
        .map(|row| {
            Ok(DismissalRow {
                theme_id: row
                    .try_get("id")
                    .map_err(|e| format!("read dismissal theme id: {e}"))?,
                engine_version: row
                    .try_get("upgrade_dismissed_engine_version")
                    .map_err(|e| format!("read dismissal engine version: {e}"))?,
            })
        })
        .collect()
}

/// Rename a user theme. Fails when the theme does not exist.
pub async fn rename(pool: &SqlitePool, id: &str, display_name: &str) -> Result<(), String> {
    validate_theme_id(id)?;
    validate_display_name(display_name)?;
    let now = now_ms()?;
    let mut conn = pool.acquire().await.map_err(|e| format!("acquire: {e}"))?;
    let result = sqlx::query(
        "UPDATE themes SET display_name = ?, updated_at_ms = ?
          WHERE id = ? AND display_name IS NOT ?",
    )
    .bind(display_name)
    .bind(now)
    .bind(id)
    .bind(display_name)
    .execute(&mut *conn)
    .await
    .map_err(|e| format!("rename theme: {e}"))?;
    if result.rows_affected() == 0 {
        ensure_theme_exists(&mut conn, id, "rename theme").await?;
    }
    Ok(())
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
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM theme_tokens WHERE theme_id = ? AND kind = ? AND key = ?",
    )
    .bind(id)
    .bind(kind)
    .bind(key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| format!("read theme token: {e}"))?;
    if exists.is_none() {
        return Err("reset theme token to seed: no matching row".to_string());
    }
    let result = sqlx::query(
        "UPDATE theme_tokens AS t
            SET value = s.value, isolated = s.isolated
           FROM theme_seed_tokens AS s
          WHERE t.theme_id = ? AND t.kind = ? AND t.key = ?
            AND s.theme_id = t.theme_id AND s.kind = t.kind AND s.key = t.key
            AND (t.value IS NOT s.value OR t.isolated IS NOT s.isolated)",
    )
    .bind(id)
    .bind(kind)
    .bind(key)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("reset theme token to seed: {e}"))?;
    if result.rows_affected() > 0 {
        touch_theme(&mut tx, id, now).await?;
    }
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    Ok(())
}

/// Restore every current token, palette slot, blend canvas, and Calendar default from the theme's
/// seed. The display name and icon label are kept. Fails when the theme does not exist.
pub async fn reset_to_seed(pool: &SqlitePool, id: &str) -> Result<(), String> {
    validate_theme_id(id)?;
    let now = now_ms()?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    ensure_theme_exists(&mut tx, id, "reset theme to seed").await?;

    let tokens = sqlx::query(
        "UPDATE theme_tokens AS t
            SET value = s.value, isolated = s.isolated
           FROM theme_seed_tokens AS s
          WHERE t.theme_id = ?
            AND s.theme_id = t.theme_id AND s.kind = t.kind AND s.key = t.key
            AND (t.value IS NOT s.value OR t.isolated IS NOT s.isolated)",
    )
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("reset theme tokens to seed: {e}"))?;

    let palette = sqlx::query(
        "UPDATE theme_event_palette AS p
            SET value = s.value
           FROM theme_seed_event_palette AS s
          WHERE p.theme_id = ?
            AND s.theme_id = p.theme_id AND s.slot = p.slot
            AND p.value IS NOT s.value",
    )
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("reset theme palette to seed: {e}"))?;

    let theme = sqlx::query(
        "UPDATE themes
            SET blend_canvas = seed_blend_canvas,
                calendar_default_mode = seed_calendar_default_mode,
                calendar_default_custom = seed_calendar_default_custom
          WHERE id = ?
            AND (blend_canvas IS NOT seed_blend_canvas
                 OR calendar_default_mode IS NOT seed_calendar_default_mode
                 OR calendar_default_custom IS NOT seed_calendar_default_custom)",
    )
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("reset theme to seed: {e}"))?;

    if tokens.rows_affected() + palette.rows_affected() + theme.rows_affected() > 0 {
        touch_theme(&mut tx, id, now).await?;
    }
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    Ok(())
}

const TOKENS: &str =
    "INSERT INTO theme_tokens (theme_id, kind, key, value, isolated) VALUES (?, ?, ?, ?, ?)";
const PALETTE: &str = "INSERT INTO theme_event_palette (theme_id, slot, value) VALUES (?, ?, ?)";
const SEED_TOKENS: &str =
    "INSERT INTO theme_seed_tokens (theme_id, kind, key, value, isolated) VALUES (?, ?, ?, ?, ?)";
const SEED_PALETTE: &str =
    "INSERT INTO theme_seed_event_palette (theme_id, slot, value) VALUES (?, ?, ?)";

/// The current content of a theme, or `None` when the theme does not exist.
async fn load_theme_content(
    conn: &mut SqliteConnection,
    id: &str,
) -> Result<Option<ThemeContent>, String> {
    let Some((
        blend_canvas,
        derivation_engine_version,
        calendar_default_mode,
        calendar_default_custom,
    )) = sqlx::query_as::<_, (String, i64, String, String)>(
        "SELECT blend_canvas, derivation_engine_version, calendar_default_mode,
                    calendar_default_custom
               FROM themes WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(|e| format!("read theme content: {e}"))?
    else {
        return Ok(None);
    };
    let tokens = sqlx::query_as::<_, (String, String, String, bool)>(
        "SELECT kind, key, value, isolated FROM theme_tokens
          WHERE theme_id = ? ORDER BY kind ASC, key ASC",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await
    .map_err(|e| format!("read theme tokens: {e}"))?
    .into_iter()
    .map(|(kind, key, value, isolated)| ThemeToken {
        kind,
        key,
        value,
        isolated,
    })
    .collect();
    let palette = sqlx::query_scalar::<_, String>(
        "SELECT value FROM theme_event_palette WHERE theme_id = ? ORDER BY slot ASC",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await
    .map_err(|e| format!("read theme palette: {e}"))?;
    Ok(Some(ThemeContent {
        blend_canvas,
        derivation_engine_version,
        calendar_default_mode,
        calendar_default_custom,
        tokens,
        palette,
    }))
}

/// Writes `content` to an existing theme, touching only the columns and owned rows that differ.
/// Returns whether anything changed; fails when the theme does not exist.
async fn write_theme_content(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    content: &ThemeContent,
) -> Result<bool, String> {
    let current = load_theme_content(tx, id)
        .await?
        .ok_or_else(|| format!("theme '{id}' not found"))?;
    if &current == content {
        return Ok(false);
    }
    sqlx::query(
        "UPDATE themes
            SET blend_canvas = ?, derivation_engine_version = ?,
                calendar_default_mode = ?, calendar_default_custom = ?
          WHERE id = ?",
    )
    .bind(&content.blend_canvas)
    .bind(content.derivation_engine_version)
    .bind(&content.calendar_default_mode)
    .bind(&content.calendar_default_custom)
    .bind(id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("update theme content: {e}"))?;
    if current.tokens != content.tokens {
        sqlx::query("DELETE FROM theme_tokens WHERE theme_id = ?")
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("delete theme tokens: {e}"))?;
        insert_tokens(tx, id, &content.tokens, TOKENS).await?;
    }
    if current.palette != content.palette {
        sqlx::query("DELETE FROM theme_event_palette WHERE theme_id = ?")
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("delete theme palette: {e}"))?;
        insert_palette(tx, id, &content.palette, PALETTE).await?;
    }
    Ok(true)
}

/// The content of a checked write: tokens in canonical order and the palette in slot order.
fn write_content(write: &UserThemeWrite) -> Result<ThemeContent, String> {
    Ok(ThemeContent {
        blend_canvas: write.blend_canvas.clone(),
        derivation_engine_version: write.derivation_engine_version,
        calendar_default_mode: write.calendar_default_mode.clone(),
        calendar_default_custom: write.calendar_default_custom.clone(),
        tokens: canonical_tokens(&write_tokens(&write.tokens)).map_err(|e| e.to_string())?,
        palette: write_palette(&write.palette),
    })
}

fn write_tokens(rows: &[ThemeTokenWrite]) -> Vec<ThemeToken> {
    rows.iter()
        .map(|row| ThemeToken {
            kind: row.kind.clone(),
            key: row.key.clone(),
            value: row.value.clone(),
            isolated: row.isolated,
        })
        .collect()
}

/// Palette values in slot order. Callers pass rows [`validate_palette_rows`] accepted, so every
/// slot appears exactly once.
fn write_palette(rows: &[ThemePaletteWrite]) -> Vec<String> {
    let mut sorted: Vec<&ThemePaletteWrite> = rows.iter().collect();
    sorted.sort_by_key(|row| row.slot);
    sorted.into_iter().map(|row| row.value.clone()).collect()
}

async fn insert_tokens(
    tx: &mut Transaction<'_, Sqlite>,
    theme_id: &str,
    tokens: &[ThemeToken],
    query: &'static str,
) -> Result<(), String> {
    for token in tokens {
        sqlx::query(query)
            .bind(theme_id)
            .bind(&token.kind)
            .bind(&token.key)
            .bind(&token.value)
            .bind(token.isolated)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("insert theme token '{}:{}': {e}", token.kind, token.key))?;
    }
    Ok(())
}

async fn insert_palette(
    tx: &mut Transaction<'_, Sqlite>,
    theme_id: &str,
    palette: &[String],
    query: &'static str,
) -> Result<(), String> {
    for (slot, value) in palette.iter().enumerate() {
        sqlx::query(query)
            .bind(theme_id)
            .bind(slot as i64)
            .bind(value)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("insert theme palette slot {slot}: {e}"))?;
    }
    Ok(())
}

async fn touch_theme(
    tx: &mut Transaction<'_, Sqlite>,
    theme_id: &str,
    now: i64,
) -> Result<(), String> {
    let result =
        sqlx::query("UPDATE themes SET updated_at_ms = max(updated_at_ms, ?) WHERE id = ?")
            .bind(now)
            .bind(theme_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("touch theme: {e}"))?;
    ensure_row_changed(result, "touch theme")
}

async fn ensure_theme_exists(
    conn: &mut SqliteConnection,
    id: &str,
    context: &str,
) -> Result<(), String> {
    let exists = sqlx::query_scalar::<_, i64>("SELECT 1 FROM themes WHERE id = ?")
        .bind(id)
        .fetch_optional(conn)
        .await
        .map_err(|e| format!("{context}: {e}"))?;
    if exists.is_some() {
        Ok(())
    } else {
        Err(format!("{context}: no matching row"))
    }
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
    if is_display_name(value) {
        Ok(())
    } else {
        Err(format!(
            "display_name must be 1 to {} characters and not blank",
            ganbaru_sync::domains::themes::MAX_DISPLAY_NAME_CHARS
        ))
    }
}

fn validate_theme_id(id: &str) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err("theme id cannot be empty".to_string());
    }
    if id.len() > MAX_THEME_ID_CHARS {
        return Err(format!(
            "theme id cannot exceed {MAX_THEME_ID_CHARS} characters"
        ));
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
    if !ganbaru_sync::domains::themes::TOKEN_KINDS.contains(&kind) {
        return Err(format!("{field} contains invalid token kind '{kind}'"));
    }
    if !is_token_key(key) {
        return Err(format!("{field} contains an invalid token key"));
    }
    Ok(())
}

fn validate_icon_label(value: &str, field: &str) -> Result<(), String> {
    if is_icon_label(value) {
        Ok(())
    } else {
        Err(format!("{field} must be 'light' or 'dark'"))
    }
}

fn validate_calendar_mode(value: &str, field: &str) -> Result<(), String> {
    if is_calendar_mode(value) {
        Ok(())
    } else {
        Err(format!(
            "{field} must be 'light', 'dark', 'app-canvas', or 'custom'"
        ))
    }
}

fn validate_token_rows(rows: &[ThemeTokenWrite], field: &str) -> Result<(), String> {
    canonical_tokens(&write_tokens(rows)).map_err(|e| format!("{field}: {e}"))?;
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

fn now_ms() -> Result<i64, String> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("system clock before unix epoch: {e}"))?;
    i64::try_from(duration.as_millis()).map_err(|e| format!("system clock out of range: {e}"))
}

#[cfg(test)]
mod tests;
