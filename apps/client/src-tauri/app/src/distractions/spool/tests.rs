use super::*;
use ganbaru_distractions::usage::usage_date_window;

const VAULT: &str = "vault";
const DEVICE: &str = "laptop";

fn temporary_spool(name: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir()
        .join(format!(
            "ganbaru-usage-spool-{name}-{}-{unique}",
            std::process::id()
        ))
        .join(SPOOL_FILE)
}

fn window() -> UsageDateWindow {
    usage_date_window(chrono::NaiveDate::from_ymd_opt(2026, 9, 13).unwrap())
}

fn sample(id: &str, seconds: i64) -> DistractionsUsageSampleRow {
    DistractionsUsageSampleRow {
        id: id.to_string(),
        source_type: "desktop-app".to_string(),
        source_key: "steam".to_string(),
        display_name: Some("Steam".to_string()),
        started_at_ms: 1_700_000_000_000,
        elapsed_seconds: seconds,
        local_date: "2026-09-13".to_string(),
        created_at_ms: 1_700_000_001_000,
    }
}

async fn vault_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("open vault database");
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .expect("apply migrations");
    pool
}

async fn pending(path: &Path) -> Vec<DistractionsUsageSampleRow> {
    let pool = open_spool(path).await.unwrap();
    let mut connection = pool.acquire().await.unwrap();
    let rows = read_pending(&mut connection, VAULT, DEVICE).await.unwrap();
    drop(connection);
    pool.close().await;
    rows
}

async fn journal(pool: &SqlitePool) -> Vec<(String, i64)> {
    sqlx::query_as("SELECT id, elapsed_seconds FROM distractions_usage_samples ORDER BY id")
        .fetch_all(pool)
        .await
        .unwrap()
}

fn remove(path: &Path) {
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn idle_drain_does_not_create_a_spool_before_any_samples_exist() {
    let path = temporary_spool("idle");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    for _ in 0..3 {
        assert_eq!(
            drain_at(&path, &pool, VAULT, DEVICE, &window())
                .await
                .unwrap(),
            0
        );
    }
    assert!(!path.parent().unwrap().exists());
    pool.close().await;
}

#[tokio::test]
async fn a_fresh_spool_holds_only_pending_samples_and_batch_receipts() {
    let path = temporary_spool("tables");
    let pool = open_spool(&path).await.unwrap();
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_schema WHERE type IN ('table', 'trigger') ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    pool.close().await;
    assert_eq!(
        tables,
        ["native_usage_batch_receipts", "pending_usage_samples"]
    );
    remove(&path);
}

#[tokio::test]
async fn drained_samples_join_the_journal_once_and_leave_the_spool() {
    let path = temporary_spool("drain");
    let vault = vault_pool().await;
    enqueue_native_samples_at(
        &path,
        VAULT,
        DEVICE,
        &[sample("one", 12), sample("two", 18)],
    )
    .await
    .unwrap();
    enqueue_native_samples_at(&path, "other-vault", DEVICE, &[sample("three", 5)])
        .await
        .unwrap();
    assert_eq!(
        drain_at(&path, &vault, VAULT, DEVICE, &window())
            .await
            .unwrap(),
        2
    );
    assert!(pending(&path).await.is_empty());
    assert_eq!(
        journal(&vault).await,
        [("one".to_string(), 12), ("two".to_string(), 18)]
    );

    // A batch that committed to the journal but stayed in the spool drains again harmlessly.
    let spool = open_spool(&path).await.unwrap();
    let mut connection = spool.acquire().await.unwrap();
    insert_pending(&mut connection, VAULT, DEVICE, &sample("one", 12))
        .await
        .unwrap();
    drop(connection);
    spool.close().await;
    assert_eq!(
        drain_at(&path, &vault, VAULT, DEVICE, &window())
            .await
            .unwrap(),
        1
    );
    assert_eq!(journal(&vault).await.len(), 2);
    vault.close().await;
    remove(&path);
}

#[tokio::test]
async fn a_failed_journal_import_keeps_the_batch_in_the_spool() {
    let path = temporary_spool("failed-drain");
    let vault = vault_pool().await;
    sqlx::raw_sql(
        "CREATE TRIGGER reject_usage BEFORE INSERT ON distractions_usage_samples
         WHEN NEW.id = 'two' BEGIN SELECT RAISE(ABORT, 'injected failure'); END",
    )
    .execute(&vault)
    .await
    .unwrap();
    enqueue_native_samples_at(
        &path,
        VAULT,
        DEVICE,
        &[sample("one", 12), sample("two", 18)],
    )
    .await
    .unwrap();
    assert!(
        drain_at(&path, &vault, VAULT, DEVICE, &window())
            .await
            .is_err()
    );
    assert_eq!(pending(&path).await.len(), 2);
    assert!(journal(&vault).await.is_empty());
    vault.close().await;
    remove(&path);
}

#[tokio::test]
async fn native_batch_retry_after_a_drain_does_not_reinsert_usage() {
    let path = temporary_spool("receipt");
    let vault = vault_pool().await;
    let rows = [sample("one", 12), sample("two", 18)];
    enqueue_native_samples_at(&path, VAULT, DEVICE, &rows)
        .await
        .unwrap();
    drain_at(&path, &vault, VAULT, DEVICE, &window())
        .await
        .unwrap();
    enqueue_native_samples_at(&path, VAULT, DEVICE, &rows)
        .await
        .unwrap();
    assert!(pending(&path).await.is_empty());
    assert!(
        enqueue_native_samples_at(
            &path,
            VAULT,
            DEVICE,
            &[sample("one", 13), sample("two", 18)]
        )
        .await
        .unwrap_err()
        .contains("changed its payload")
    );
    enqueue_native_samples_at(&path, VAULT, DEVICE, &[sample("next", 5)])
        .await
        .unwrap();
    assert_eq!(pending(&path).await, [sample("next", 5)]);
    vault.close().await;
    remove(&path);
}

#[tokio::test]
async fn failed_native_batch_rolls_back_all_rows_and_receipt_before_retry() {
    let path = temporary_spool("rollback");
    let pool = open_spool(&path).await.unwrap();
    sqlx::raw_sql(
        "CREATE TRIGGER reject_native_batch BEFORE INSERT ON pending_usage_samples
         WHEN NEW.sample_id = 'two' BEGIN SELECT RAISE(ABORT, 'injected failure'); END",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    let rows = [sample("one", 12), sample("two", 18)];
    assert!(
        enqueue_native_samples_at(&path, VAULT, DEVICE, &rows)
            .await
            .is_err()
    );
    assert!(pending(&path).await.is_empty());
    let pool = open_spool(&path).await.unwrap();
    let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM native_usage_batch_receipts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(receipts, 0);
    sqlx::query("DROP TRIGGER reject_native_batch")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    enqueue_native_samples_at(&path, VAULT, DEVICE, &rows)
        .await
        .unwrap();
    assert_eq!(pending(&path).await.len(), 2);
    remove(&path);
}

#[tokio::test]
async fn compaction_preserves_the_exact_pending_total() {
    let path = temporary_spool("compaction");
    enqueue_native_samples_at(&path, VAULT, DEVICE, &[sample("one", 12)])
        .await
        .unwrap();
    enqueue_native_samples_at(&path, VAULT, DEVICE, &[sample("two", 86_400)])
        .await
        .unwrap();
    let pool = open_spool(&path).await.unwrap();
    let mut transaction = pool.begin().await.unwrap();
    compact_pending(&mut transaction, VAULT, DEVICE)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    pool.close().await;
    let compacted = pending(&path).await;
    assert_eq!(compacted.len(), 2, "one day of usage per compacted sample");
    assert_eq!(
        compacted
            .iter()
            .map(|sample| sample.elapsed_seconds)
            .sum::<i64>(),
        86_412
    );
    assert!(
        compacted
            .iter()
            .all(|sample| sample.id.starts_with("compact-"))
    );
    remove(&path);
}

#[tokio::test]
async fn pending_usage_sums_per_source_and_day_inside_the_week() {
    let path = temporary_spool("pending-days");
    let mut earlier = sample("earlier", 40);
    earlier.local_date = "2026-09-06".to_string();
    enqueue_native_samples_at(
        &path,
        VAULT,
        DEVICE,
        &[sample("one", 12), sample("two", 18), earlier],
    )
    .await
    .unwrap();
    let days = pending_source_days_at(&path, VAULT, DEVICE, "2026-09-07", "2026-09-13")
        .await
        .unwrap();
    assert_eq!(
        days,
        [UsageSourceDay {
            source_type: "desktop-app".to_string(),
            source_key: "steam".to_string(),
            local_date: "2026-09-13".to_string(),
            elapsed_seconds: 30,
        }]
    );
    remove(&path);
}
