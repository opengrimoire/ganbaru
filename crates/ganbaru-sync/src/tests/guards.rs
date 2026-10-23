//! Read-only guards on replica connections.

use std::sync::LazyLock;

use sqlx::SqlitePool;

use super::fixture::{TEST_MANIFEST, fixture_pool};
use super::{block_on, execute, migrated_pool, set_applying};
use crate::guards::{guarded_tables, install_guards, is_guard_abort};
use crate::manifest::vault::{VAULT_MANIFEST, quick_notes};
use crate::manifest::{DomainManifest, Manifest};

async fn guard(pool: &SqlitePool) -> usize {
    let mut conn = pool.acquire().await.unwrap();
    install_guards(&mut conn, &VAULT_MANIFEST).await.unwrap()
}

async fn refusal(pool: &SqlitePool, sql: &str) -> String {
    match sqlx::raw_sql(sqlx::AssertSqlSafe(sql)).execute(pool).await {
        Ok(_) => panic!("guarded write succeeded: {sql}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn guarded_set_excludes_replicated_owned_derived_and_engine_tables() {
    block_on(async {
        let pool = migrated_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        let tables = guarded_tables(&mut conn, &VAULT_MANIFEST).await.unwrap();
        for excluded in [
            "quick_notes",
            "quick_note_tags",
            "quick_note_text_runs",
            "quick_notes_search_fts",
            "quick_notes_search_fts_data",
            "contacts",
            "contact_requests",
            "contact_card",
            "contacts_local_identity",
            "projects",
            "project_tasks",
            "project_custom_field_option_values",
            "sync_ops",
            "sync_capture",
            "_sqlx_migrations",
        ] {
            assert!(
                !tables.iter().any(|table| table == excluded),
                "{excluded} is guarded"
            );
        }
        assert!(
            tables.iter().any(|table| table == "calendar_events"),
            "calendar_events is not guarded"
        );
    });
}

#[test]
fn guards_abort_unconverted_writes_and_allow_replicated_ones() {
    block_on(async {
        let pool = migrated_pool().await;
        execute(
            &pool,
            "CREATE TABLE newer_feature (id TEXT PRIMARY KEY, value TEXT)",
        )
        .await;
        execute(&pool, "INSERT INTO newer_feature VALUES ('a', 'one')").await;
        assert!(guard(&pool).await > 0);

        for sql in [
            "INSERT INTO newer_feature VALUES ('b', 'two')",
            "UPDATE newer_feature SET value = 'changed'",
            "DELETE FROM newer_feature",
        ] {
            let error = refusal(&pool, sql).await;
            assert!(is_guard_abort(&error), "{sql}: {error}");
        }
        let value: String = sqlx::query_scalar("SELECT value FROM newer_feature WHERE id = 'a'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(value, "one");

        execute(
            &pool,
            "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-1', 'Work', 'a0');
             INSERT INTO quick_notes (id, title, tag_id, order_key) VALUES ('note-1', 'One', 'tag-1', 'a0');
             INSERT INTO quick_note_text_runs (note_id, sort_order, content) VALUES ('note-1', 0, 'body');
             UPDATE quick_notes SET title = 'Renamed' WHERE id = 'note-1';
             DELETE FROM quick_note_tags WHERE id = 'tag-1';
             DELETE FROM quick_notes WHERE id = 'note-1';",
        )
        .await;
        let captures: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_capture")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(captures, 2);
    });
}

#[test]
fn guards_let_engine_apply_transactions_write_guarded_tables() {
    block_on(async {
        let pool = migrated_pool().await;
        guard(&pool).await;
        let insert = "INSERT INTO calendars (id, name) VALUES ('calendar-1', 'Calendar')";
        let error = refusal(&pool, insert).await;
        assert!(is_guard_abort(&error), "{error}");

        set_applying(&pool, true).await;
        execute(&pool, insert).await;
        execute(
            &pool,
            "UPDATE calendars SET name = 'Renamed' WHERE id = 'calendar-1'",
        )
        .await;
        set_applying(&pool, false).await;

        let error = refusal(&pool, "DELETE FROM calendars").await;
        assert!(is_guard_abort(&error), "{error}");
        let name: String = sqlx::query_scalar("SELECT name FROM calendars WHERE id = 'calendar-1'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(name, "Renamed");
    });
}

#[test]
fn guards_hold_when_the_apply_state_row_is_missing() {
    block_on(async {
        let pool = migrated_pool().await;
        guard(&pool).await;
        execute(&pool, "DELETE FROM sync_apply_state").await;
        let error = refusal(
            &pool,
            "INSERT INTO calendars (id, name) VALUES ('calendar-1', 'Calendar')",
        )
        .await;
        assert!(is_guard_abort(&error), "{error}");
    });
}

#[test]
fn replicated_tables_of_domains_that_replicas_cannot_write_are_guarded() {
    const READ_ONLY: DomainManifest = DomainManifest {
        replica_writable: false,
        ..quick_notes::DOMAIN
    };
    static DOMAINS: &[&DomainManifest] = &[&READ_ONLY];
    static MANIFEST: LazyLock<Manifest> = LazyLock::new(|| Manifest::from_domains(1, DOMAINS));
    block_on(async {
        let pool = migrated_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        let tables = guarded_tables(&mut conn, &MANIFEST).await.unwrap();
        for guarded in [
            "quick_notes",
            "quick_note_tags",
            "quick_note_text_runs",
            "quick_note_images",
        ] {
            assert!(
                tables.iter().any(|table| table == guarded),
                "{guarded} is not guarded"
            );
        }
        assert!(!tables.iter().any(|table| table == "quick_notes_search_fts"));
        install_guards(&mut conn, &MANIFEST).await.unwrap();
        drop(conn);

        let insert =
            "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-1', 'Work', 'a0')";
        let error = refusal(&pool, insert).await;
        assert!(is_guard_abort(&error), "{error}");
        set_applying(&pool, true).await;
        execute(&pool, insert).await;
    });
}

#[test]
fn local_and_derived_tables_stay_writable_under_guards() {
    block_on(async {
        let pool = fixture_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        let tables = guarded_tables(&mut conn, &TEST_MANIFEST).await.unwrap();
        for open in ["fixture_items", "fixture_item_marks", "fixture_items_index"] {
            assert!(
                !tables.iter().any(|table| table == open),
                "{open} is guarded"
            );
        }
        install_guards(&mut conn, &TEST_MANIFEST).await.unwrap();
        drop(conn);
        execute(
            &pool,
            "INSERT INTO fixture_items (id, created_at, label, label_edits)
                 VALUES ('item-1', '2027-01-01T00:00:00.000Z', 'One', 0);
             UPDATE fixture_item_marks SET mark = 2 WHERE item_id = 'item-1';
             UPDATE fixture_items_index SET label = 'Rebuilt' WHERE item_id = 'item-1';",
        )
        .await;
        let mark: i64 = sqlx::query_scalar("SELECT mark FROM fixture_item_marks")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mark, 2);
    });
}

#[test]
fn guards_stay_out_of_the_vault_schema_and_install_idempotently() {
    block_on(async {
        let pool = migrated_pool().await;
        let tables = guard(&pool).await;
        assert_eq!(guard(&pool).await, tables);
        let persistent: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM main.sqlite_schema WHERE name LIKE 'sync\\_guard\\_%' ESCAPE '\\'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(persistent, 0);
        let temporary: i64 =
            sqlx::query_scalar("SELECT count(*) FROM temp.sqlite_schema WHERE type = 'trigger'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(temporary, i64::try_from(tables * 3).unwrap());
    });
}

fn guard_hook(
    conn: &mut sqlx::SqliteConnection,
    access: ganbaru_db::DatabaseAccessMode,
) -> ganbaru_db::ConnectionHookFuture<'_> {
    Box::pin(async move {
        if access == ganbaru_db::DatabaseAccessMode::Guarded {
            install_guards(conn, &VAULT_MANIFEST)
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    })
}

#[test]
fn a_guarded_vault_pool_installs_guards_on_its_connection() {
    block_on(async {
        let directory = std::env::temp_dir().join(format!(
            "ganbaru-sync-guarded-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("ganbaru.sqlite");
        let registry = ganbaru_db::DatabasePoolRegistry::default();
        registry.connect_path(&path).await.unwrap();
        registry.close_all().await.unwrap();

        registry.set_connection_hook(Some(ganbaru_db::connection_hook(guard_hook)));
        let (pool, access) = registry.connect_path_guarded(&path).await.unwrap();
        assert_eq!(access, ganbaru_db::DatabaseAccessMode::Guarded);
        execute(
            &pool,
            "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-1', 'Work', 'a0')",
        )
        .await;
        let error = refusal(
            &pool,
            "INSERT INTO calendars (id, name) VALUES ('calendar-1', 'Calendar')",
        )
        .await;
        assert!(is_guard_abort(&error), "{error}");

        registry.close_all().await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    });
}
