//! Read-only guards on replica connections.

use sqlx::SqlitePool;

use super::{block_on, execute, migrated_pool};
use crate::guards::{guarded_tables, install_guards, is_guard_abort};
use crate::manifest::vault::VAULT_MANIFEST;

async fn guard(pool: &SqlitePool) -> usize {
    let mut conn = pool.acquire().await.unwrap();
    install_guards(&mut conn, &VAULT_MANIFEST).await.unwrap()
}

async fn refusal(pool: &SqlitePool, sql: &str) -> String {
    match sqlx::raw_sql(sql).execute(pool).await {
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
            "sync_ops",
            "sync_capture",
            "_sqlx_migrations",
        ] {
            assert!(
                !tables.iter().any(|table| table == excluded),
                "{excluded} is guarded"
            );
        }
        for guarded in ["projects", "contacts_local_identity"] {
            assert!(
                tables.iter().any(|table| table == guarded),
                "{guarded} is not guarded"
            );
        }
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
        let path = directory.join("ganbaru-ai.sqlite");
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
            "INSERT INTO project_groups (id, name) VALUES ('group-1', 'Group')",
        )
        .await;
        assert!(is_guard_abort(&error), "{error}");

        registry.close_all().await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    });
}
