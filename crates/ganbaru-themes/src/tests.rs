use super::*;
use ganbaru_db::run_migrations;
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
    run_migrations(&pool).await.unwrap();
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
    sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
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
fn replace_content_rewrites_every_row_and_requires_an_existing_theme() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert(&pool, &theme_write("custom")).await.unwrap();

        let mut replacement = theme_write("custom");
        replacement.display_name = "Replaced".to_string();
        replacement.tokens = vec![token("source", "accent", "#555555", false)];
        replacement.seed_tokens = vec![token("source", "accent", "#666666", false)];
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
            [("source", "accent", "#666666", 0)]
        );
        assert!(theme.palette.iter().all(|row| row.value == "#cccccc"));

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
fn dismissals_replace_per_version_and_cascade_when_the_theme_is_deleted() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert(&pool, &theme_write("custom")).await.unwrap();

        record_dismissal(&pool, "custom", 3).await.unwrap();
        record_dismissal(&pool, "custom", 3).await.unwrap();
        record_dismissal(&pool, "custom", 4).await.unwrap();
        assert!(record_dismissal(&pool, "custom", -1).await.is_err());
        let mut versions = load_dismissals(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|row| (row.theme_id, row.engine_version))
            .collect::<Vec<_>>();
        versions.sort();
        assert_eq!(
            versions,
            [("custom".to_string(), 3), ("custom".to_string(), 4)]
        );

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
