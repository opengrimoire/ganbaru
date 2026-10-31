use super::*;
use ganbaru_sync_contracts::{Field, GroupId, Value};
use std::future::Future;

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create themes test runtime")
        .block_on(future)
}

async fn migrated_memory_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
    pool
}

fn token(kind: &str, key: &str, value: &str, isolated: bool) -> ThemeTokenWrite {
    ThemeTokenWrite {
        kind: kind.to_string(),
        key: key.to_string(),
        value: value.to_string(),
        isolated,
    }
}

fn palette(value: &str) -> Vec<ThemePaletteWrite> {
    (0..PALETTE_SIZE)
        .map(|slot| ThemePaletteWrite {
            slot: slot as i64,
            value: value.to_string(),
        })
        .collect()
}

fn theme_write(id: &str) -> UserThemeWrite {
    UserThemeWrite {
        id: id.to_string(),
        display_name: "Custom".to_string(),
        icon_label: "dark".to_string(),
        seed_icon_label: "light".to_string(),
        blend_canvas: "#101010".to_string(),
        seed_blend_canvas: "#202020".to_string(),
        derivation_engine_version: 2,
        calendar_default_mode: "custom".to_string(),
        calendar_default_custom: "#303030".to_string(),
        seed_calendar_default_mode: "app-canvas".to_string(),
        seed_calendar_default_custom: "#27282a".to_string(),
        tokens: vec![
            token("source", "canvas", "#111111", true),
            token("app", "--background", "#121212", false),
        ],
        palette: palette("#aaaaaa"),
        seed_tokens: vec![
            token("source", "canvas", "#222222", false),
            token("app", "--background", "#232323", false),
        ],
        seed_palette: palette("#bbbbbb"),
    }
}

async fn load_one(pool: &SqlitePool, id: &str) -> UserThemeRead {
    load_all(pool)
        .await
        .unwrap()
        .into_iter()
        .find(|theme| theme.theme.id == id)
        .unwrap_or_else(|| panic!("theme {id} is missing"))
}

fn token_values(rows: &[TokenRowRead]) -> Vec<(&str, &str, &str, i64)> {
    rows.iter()
        .map(|row| {
            (
                row.kind.as_str(),
                row.key.as_str(),
                row.value.as_str(),
                row.isolated,
            )
        })
        .collect()
}

async fn count_rows(pool: &SqlitePool, table: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn accepts_six_and_eight_digit_hex_colors() {
    assert!(is_hex_color("#123abc"));
    assert!(is_hex_color("#123abcff"));
    assert!(is_hex_color("#ABCDEF"));
}

#[test]
fn rejects_non_hex_colors() {
    assert!(!is_hex_color("123abc"));
    assert!(!is_hex_color("#123abz"));
    assert!(!is_hex_color("#12345"));
    assert!(!is_hex_color("#1234567890"));
}

#[test]
fn rejects_reserved_theme_ids() {
    assert!(validate_theme_id("custom").is_ok());
    assert!(validate_theme_id("light").is_err());
    assert!(validate_theme_id("dark").is_err());
    assert!(validate_theme_id("").is_err());
}

#[test]
fn rejects_non_slug_theme_ids() {
    assert!(validate_theme_id("custom-theme-2").is_ok());
    assert!(validate_theme_id("Custom").is_err());
    assert!(validate_theme_id("-custom").is_err());
    assert!(validate_theme_id("custom theme").is_err());
    assert!(validate_theme_id("custom_theme").is_err());
}

#[test]
fn rejects_duplicate_palette_slots() {
    let mut rows = palette("#123456");
    rows[1].slot = rows[0].slot;
    assert!(validate_palette_rows(&rows, "palette").is_err());
}

#[test]
fn validates_theme_display_names() {
    assert!(validate_display_name("Custom").is_ok());
    assert!(validate_display_name(" ").is_err());
    assert!(validate_display_name(&"x".repeat(61)).is_err());
}

#[test]
fn validates_token_identity() {
    assert!(validate_token_identity("source", "canvas", "token").is_ok());
    assert!(validate_token_identity("app", "--background", "token").is_ok());
    assert!(validate_token_identity("invalid", "canvas", "token").is_err());
    assert!(validate_token_identity("source", " ", "token").is_err());
}

#[test]
fn rejects_writes_with_duplicate_tokens_or_incomplete_palettes() {
    let mut duplicate = theme_write("custom");
    duplicate
        .tokens
        .push(token("source", "canvas", "#333333", false));
    assert!(validate_theme_write(&duplicate).is_err());

    let mut incomplete = theme_write("custom");
    incomplete.seed_palette.pop();
    assert!(validate_theme_write(&incomplete).is_err());
}

#[test]
fn inserted_themes_load_with_their_own_current_and_seed_rows() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        assert!(load_all(&pool).await.unwrap().is_empty());

        insert(&pool, &theme_write("first")).await.unwrap();
        let mut second = theme_write("second");
        second.tokens = vec![token("calendar", "event-text", "#444444", false)];
        insert(&pool, &second).await.unwrap();

        let first = load_one(&pool, "first").await;
        assert_eq!(first.theme.display_name, "Custom");
        assert_eq!(first.theme.seed_icon_label, "light");
        assert_eq!(
            token_values(&first.tokens),
            [
                ("app", "--background", "#121212", 0),
                ("source", "canvas", "#111111", 1),
            ]
        );
        assert_eq!(
            token_values(&first.seed_tokens),
            [
                ("app", "--background", "#232323", 0),
                ("source", "canvas", "#222222", 0),
            ]
        );
        assert_eq!(first.palette.len(), PALETTE_SIZE);
        assert!(first.palette.iter().all(|row| row.value == "#aaaaaa"));
        assert!(first.seed_palette.iter().all(|row| row.value == "#bbbbbb"));
        assert_eq!(
            token_values(&load_one(&pool, "second").await.tokens),
            [("calendar", "event-text", "#444444", 0)]
        );
    });
}

#[test]
fn insert_rejects_taken_ids_and_invalid_writes_without_partial_rows() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert(&pool, &theme_write("custom")).await.unwrap();
        let error = insert(&pool, &theme_write("custom")).await.unwrap_err();
        assert!(error.contains("already exists"), "{error}");

        let mut invalid = theme_write("other");
        invalid.palette.pop();
        assert!(insert(&pool, &invalid).await.is_err());
        assert_eq!(count_rows(&pool, "themes").await, 1);
        assert_eq!(count_rows(&pool, "theme_tokens").await, 2);
    });
}

#[test]
fn replace_content_rewrites_current_rows_keeps_the_seed_and_requires_an_existing_theme() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert(&pool, &theme_write("custom")).await.unwrap();

        let mut replacement = theme_write("custom");
        replacement.display_name = "Replaced".to_string();
        replacement.tokens = vec![token("source", "accent", "#555555", false)];
        replacement.seed_tokens = vec![token("source", "accent", "#666666", false)];
        replacement.seed_blend_canvas = "#999999".to_string();
        replacement.palette = palette("#cccccc");
        replace_content(&pool, &replacement).await.unwrap();

        let theme = load_one(&pool, "custom").await;
        assert_eq!(theme.theme.display_name, "Replaced");
        assert_eq!(
            token_values(&theme.tokens),
            [("source", "accent", "#555555", 0)]
        );
        assert_eq!(
            token_values(&theme.seed_tokens),
            [
                ("app", "--background", "#232323", 0),
                ("source", "canvas", "#222222", 0),
            ]
        );
        assert_eq!(theme.theme.seed_blend_canvas, "#202020");
        assert!(theme.palette.iter().all(|row| row.value == "#cccccc"));
        assert!(theme.seed_palette.iter().all(|row| row.value == "#bbbbbb"));

        let error = replace_content(&pool, &theme_write("missing"))
            .await
            .unwrap_err();
        assert!(error.contains("not found"), "{error}");
        assert_eq!(count_rows(&pool, "themes").await, 1);
    });
}

#[test]
fn reset_token_to_seed_restores_only_the_named_token() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert(&pool, &theme_write("custom")).await.unwrap();

        reset_token_to_seed(&pool, "custom", "source", "canvas")
            .await
            .unwrap();
        assert_eq!(
            token_values(&load_one(&pool, "custom").await.tokens),
            [
                ("app", "--background", "#121212", 0),
                ("source", "canvas", "#222222", 0),
            ]
        );

        let error = reset_token_to_seed(&pool, "custom", "source", "missing")
            .await
            .unwrap_err();
        assert!(error.contains("no matching row"), "{error}");
        assert!(
            reset_token_to_seed(&pool, "custom", "invalid", "canvas")
                .await
                .is_err()
        );
    });
}

#[test]
fn reset_to_seed_restores_appearance_and_keeps_the_name_and_icon() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert(&pool, &theme_write("custom")).await.unwrap();
        rename(&pool, "custom", "Renamed").await.unwrap();

        reset_to_seed(&pool, "custom").await.unwrap();
        let theme = load_one(&pool, "custom").await;
        assert_eq!(theme.theme.display_name, "Renamed");
        assert_eq!(theme.theme.icon_label, "dark");
        assert_eq!(theme.theme.blend_canvas, "#202020");
        assert_eq!(theme.theme.calendar_default_mode, "app-canvas");
        assert_eq!(theme.theme.calendar_default_custom, "#27282a");
        assert_eq!(
            token_values(&theme.tokens),
            token_values(&theme.seed_tokens)
        );
        assert!(theme.palette.iter().all(|row| row.value == "#bbbbbb"));

        let error = reset_to_seed(&pool, "missing").await.unwrap_err();
        assert!(error.contains("no matching row"), "{error}");
    });
}

#[test]
fn rename_validates_the_name_and_requires_an_existing_theme() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert(&pool, &theme_write("custom")).await.unwrap();

        assert!(rename(&pool, "custom", " ").await.is_err());
        assert!(rename(&pool, "missing", "Renamed").await.is_err());
        assert_eq!(load_one(&pool, "custom").await.theme.display_name, "Custom");
    });
}

#[test]
fn rename_never_moves_the_update_time_backwards() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert(&pool, &theme_write("custom")).await.unwrap();
        let ahead = 4_102_444_800_000_i64;
        sqlx::query("UPDATE themes SET updated_at_ms = ? WHERE id = 'custom'")
            .bind(ahead)
            .execute(&pool)
            .await
            .unwrap();

        rename(&pool, "custom", "Renamed").await.unwrap();
        let theme = load_one(&pool, "custom").await;
        assert_eq!(theme.theme.display_name, "Renamed");
        assert_eq!(theme.theme.updated_at_ms, ahead);
    });
}

#[test]
fn dismissals_keep_the_highest_version_and_go_with_the_deleted_theme() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert(&pool, &theme_write("custom")).await.unwrap();
        insert(&pool, &theme_write("other")).await.unwrap();

        record_dismissal(&pool, "custom", 3).await.unwrap();
        record_dismissal(&pool, "custom", 3).await.unwrap();
        record_dismissal(&pool, "custom", 4).await.unwrap();
        record_dismissal(&pool, "custom", 2).await.unwrap();
        assert!(record_dismissal(&pool, "custom", -1).await.is_err());
        assert!(record_dismissal(&pool, "missing", 1).await.is_err());
        let versions = load_dismissals(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|row| (row.theme_id, row.engine_version))
            .collect::<Vec<_>>();
        assert_eq!(versions, [("custom".to_string(), 4)]);
        delete(&pool, "other").await.unwrap();

        delete(&pool, "custom").await.unwrap();
        delete(&pool, "custom").await.unwrap();
        assert!(load_all(&pool).await.unwrap().is_empty());
        assert!(load_dismissals(&pool).await.unwrap().is_empty());
        for table in [
            "theme_tokens",
            "theme_event_palette",
            "theme_seed_tokens",
            "theme_seed_event_palette",
        ] {
            assert_eq!(count_rows(&pool, table).await, 0, "{table}");
        }
    });
}

#[test]
fn load_all_rejects_stored_rows_that_fail_validation() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert(&pool, &theme_write("custom")).await.unwrap();
        sqlx::query("UPDATE theme_seed_tokens SET value = 'red' WHERE key = 'canvas'")
            .execute(&pool)
            .await
            .unwrap();

        let error = load_all(&pool)
            .await
            .err()
            .expect("stored rows that fail validation are rejected");
        assert!(error.contains("theme_seed_tokens"), "{error}");
    });
}

/// Pending sync capture masks by row key, then clears them.
async fn take_captures(pool: &SqlitePool) -> Vec<(String, i64)> {
    let rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT row_key, mask FROM sync_capture ORDER BY row_key",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM sync_capture")
        .execute(pool)
        .await
        .unwrap();
    rows
}

const CONTENT_MASK: i64 = 16;
const NAME_MASK: i64 = 4;
const DISMISSAL_MASK: i64 = 32;

#[test]
fn accepts_uuid_ids_and_rejects_overlong_ids() {
    assert!(validate_theme_id("0f8fad5b-d9cb-469f-a165-70867728950e").is_ok());
    assert!(validate_theme_id(&"a".repeat(MAX_THEME_ID_CHARS)).is_ok());
    assert!(validate_theme_id(&"a".repeat(MAX_THEME_ID_CHARS + 1)).is_err());
}

#[test]
fn saves_that_change_nothing_capture_nothing() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert(&pool, &theme_write("custom")).await.unwrap();
        take_captures(&pool).await;

        replace_content(&pool, &theme_write("custom"))
            .await
            .unwrap();
        rename(&pool, "custom", "Custom").await.unwrap();
        record_dismissal(&pool, "custom", 0).await.unwrap();
        assert_eq!(take_captures(&pool).await, []);

        let mut reordered = theme_write("custom");
        reordered.tokens.reverse();
        reordered.palette.reverse();
        replace_content(&pool, &reordered).await.unwrap();
        assert_eq!(take_captures(&pool).await, []);
    });
}

#[test]
fn a_rename_through_save_captures_the_name_without_the_content() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert(&pool, &theme_write("custom")).await.unwrap();
        take_captures(&pool).await;

        let mut renamed = theme_write("custom");
        renamed.display_name = "Renamed".to_string();
        replace_content(&pool, &renamed).await.unwrap();
        let captures = take_captures(&pool).await;
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].1 & (NAME_MASK | CONTENT_MASK), NAME_MASK);

        let mut edited = renamed;
        edited.palette = palette("#cccccc");
        replace_content(&pool, &edited).await.unwrap();
        let captures = take_captures(&pool).await;
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].1 & (NAME_MASK | CONTENT_MASK), CONTENT_MASK);

        record_dismissal(&pool, "custom", 5).await.unwrap();
        let captures = take_captures(&pool).await;
        assert_eq!(captures[0].1, DISMISSAL_MASK);
    });
}

#[test]
fn resets_that_change_nothing_capture_nothing() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert(&pool, &theme_write("custom")).await.unwrap();
        reset_to_seed(&pool, "custom").await.unwrap();
        take_captures(&pool).await;

        reset_to_seed(&pool, "custom").await.unwrap();
        reset_token_to_seed(&pool, "custom", "source", "canvas")
            .await
            .unwrap();
        assert_eq!(take_captures(&pool).await, []);
    });
}

/// Restores a recovered theme on a connection of its own, outside any caller transaction.
async fn restore_from_pool(
    pool: &SqlitePool,
    values: &[(GroupId, Value)],
) -> Result<String, String> {
    let mut conn = pool.acquire().await.unwrap();
    restore_recovered(&mut conn, values).await
}

fn recovered_theme_values(content: Option<&ThemeContent>) -> Vec<(GroupId, Value)> {
    use ganbaru_sync::domains::themes::{encode_palette, encode_tokens};
    use ganbaru_sync::manifest::vault::themes::group;
    let mut values = vec![
        (
            group::NAME,
            Value::new(vec![Field::Text("Forest".to_string())]).unwrap(),
        ),
        (
            group::ICON,
            Value::new(vec![Field::Text("dark".to_string())]).unwrap(),
        ),
    ];
    if let Some(content) = content {
        values.push((
            group::CONTENT,
            Value::new(vec![
                Field::Text(content.blend_canvas.clone()),
                Field::Integer(content.derivation_engine_version),
                Field::Text(content.calendar_default_mode.clone()),
                Field::Text(content.calendar_default_custom.clone()),
                Field::Blob(encode_tokens(&content.tokens).unwrap()),
                Field::Blob(encode_palette(&content.palette).unwrap()),
            ])
            .unwrap(),
        ));
    }
    values
}

#[test]
fn restoring_a_recovered_theme_creates_a_new_theme_seeded_with_its_content() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        let content = ThemeContent {
            blend_canvas: "#0a0a0a".to_string(),
            derivation_engine_version: 3,
            calendar_default_mode: "custom".to_string(),
            calendar_default_custom: "#0b0b0b".to_string(),
            tokens: vec![ThemeToken {
                kind: "source".to_string(),
                key: "canvas".to_string(),
                value: "#0c0c0c".to_string(),
                isolated: true,
            }],
            palette: vec!["#0d0d0d".to_string(); PALETTE_SIZE],
        };

        let id = restore_from_pool(&pool, &recovered_theme_values(Some(&content)))
            .await
            .unwrap();
        let again = restore_from_pool(&pool, &recovered_theme_values(Some(&content)))
            .await
            .unwrap();
        assert_ne!(id, again);
        validate_theme_id(&id).unwrap();
        assert_eq!(load_one(&pool, &again).await.theme.display_name, "Forest 2");

        let restored = load_one(&pool, &id).await;
        assert_eq!(restored.theme.display_name, "Forest");
        assert_eq!(restored.theme.icon_label, "dark");
        assert_eq!(restored.theme.seed_icon_label, "dark");
        assert_eq!(restored.theme.blend_canvas, "#0a0a0a");
        assert_eq!(restored.theme.seed_blend_canvas, "#0a0a0a");
        assert_eq!(restored.theme.derivation_engine_version, 3);
        assert_eq!(restored.theme.seed_calendar_default_custom, "#0b0b0b");
        assert_eq!(
            token_values(&restored.tokens),
            [("source", "canvas", "#0c0c0c", 1)]
        );
        assert_eq!(
            token_values(&restored.seed_tokens),
            token_values(&restored.tokens)
        );
        assert_eq!(restored.palette.len(), PALETTE_SIZE);
        assert!(
            restored
                .seed_palette
                .iter()
                .all(|row| row.value == "#0d0d0d")
        );
    });
}

#[test]
fn restoring_a_recovered_theme_takes_the_lowest_free_name_ignoring_case() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        let mut first = theme_write("first");
        first.display_name = "forest".to_string();
        insert(&pool, &first).await.unwrap();
        let mut third = theme_write("third");
        third.display_name = "FOREST 3".to_string();
        insert(&pool, &third).await.unwrap();
        let content = ThemeContent {
            blend_canvas: "#0a0a0a".to_string(),
            derivation_engine_version: 3,
            calendar_default_mode: "custom".to_string(),
            calendar_default_custom: "#0b0b0b".to_string(),
            tokens: vec![ThemeToken {
                kind: "source".to_string(),
                key: "canvas".to_string(),
                value: "#0c0c0c".to_string(),
                isolated: false,
            }],
            palette: vec!["#0d0d0d".to_string(); PALETTE_SIZE],
        };

        let second = restore_from_pool(&pool, &recovered_theme_values(Some(&content)))
            .await
            .unwrap();
        let fourth = restore_from_pool(&pool, &recovered_theme_values(Some(&content)))
            .await
            .unwrap();
        assert_eq!(
            load_one(&pool, &second).await.theme.display_name,
            "Forest 2"
        );
        assert_eq!(
            load_one(&pool, &fourth).await.theme.display_name,
            "Forest 4"
        );
    });
}

#[test]
fn restoring_a_recovered_theme_keeps_a_long_name_when_no_suffix_fits() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        let long_name = "F".repeat(ganbaru_sync::domains::themes::MAX_DISPLAY_NAME_CHARS);
        let mut existing = theme_write("existing");
        existing.display_name = long_name.clone();
        insert(&pool, &existing).await.unwrap();
        let content = ThemeContent {
            blend_canvas: "#0a0a0a".to_string(),
            derivation_engine_version: 3,
            calendar_default_mode: "custom".to_string(),
            calendar_default_custom: "#0b0b0b".to_string(),
            tokens: vec![ThemeToken {
                kind: "source".to_string(),
                key: "canvas".to_string(),
                value: "#0c0c0c".to_string(),
                isolated: false,
            }],
            palette: vec!["#0d0d0d".to_string(); PALETTE_SIZE],
        };
        let mut values = recovered_theme_values(Some(&content));
        values[0].1 = Value::new(vec![Field::Text(long_name.clone())]).unwrap();

        let id = restore_from_pool(&pool, &values).await.unwrap();
        assert_eq!(load_one(&pool, &id).await.theme.display_name, long_name);
    });
}

#[test]
fn restoring_a_recovered_theme_without_content_writes_nothing() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        let error = restore_from_pool(&pool, &recovered_theme_values(None))
            .await
            .unwrap_err();
        assert!(error.contains("no content"), "{error}");
        assert_eq!(count_rows(&pool, "themes").await, 0);
    });
}
