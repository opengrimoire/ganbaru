//! Schema of the device-local Distractions spool, which the app and the browser host both open.
//!
//! The spool is a short-lived queue of usage samples and block events waiting to enter the vault,
//! so it carries no migrations: a spool whose `user_version` differs from
//! [`SPOOL_SCHEMA_VERSION`] holds rows the current code cannot read, and both writers replace its
//! tables with [`SPOOL_RESET_SQL`] inside one immediate transaction before using it.

/// Version stored in the spool's `user_version`. Raise it whenever [`SPOOL_RESET_SQL`] changes.
pub const SPOOL_SCHEMA_VERSION: i64 = 1;

/// Drops every spool table and creates the current ones, stamped with [`SPOOL_SCHEMA_VERSION`].
pub const SPOOL_RESET_SQL: &str = "
    DROP TABLE IF EXISTS pending_usage_samples;
    DROP TABLE IF EXISTS native_usage_batch_receipts;
    DROP TABLE IF EXISTS pending_block_events;
    CREATE TABLE pending_usage_samples (
        sample_id TEXT PRIMARY KEY,
        vault_id TEXT NOT NULL,
        device_id TEXT NOT NULL,
        source_type TEXT NOT NULL,
        source_key TEXT NOT NULL,
        display_name TEXT,
        started_at_ms INTEGER NOT NULL,
        elapsed_seconds INTEGER NOT NULL,
        local_date TEXT NOT NULL,
        created_at_ms INTEGER NOT NULL,
        claimed INTEGER NOT NULL DEFAULT 0 CHECK (claimed IN (0, 1))
    );
    CREATE TABLE native_usage_batch_receipts (
        vault_id TEXT NOT NULL,
        device_id TEXT NOT NULL,
        batch_id TEXT NOT NULL,
        digest TEXT NOT NULL,
        PRIMARY KEY (vault_id, device_id)
    );
    CREATE TABLE pending_block_events (
        event_id TEXT PRIMARY KEY,
        vault_id TEXT NOT NULL,
        device_id TEXT NOT NULL,
        run_id TEXT,
        occurred_at TEXT NOT NULL,
        source_type TEXT NOT NULL,
        source_key TEXT NOT NULL,
        display_name TEXT,
        phase TEXT,
        decision TEXT NOT NULL,
        rule_kind TEXT,
        rule_label TEXT,
        blocker_mode TEXT
    );
    PRAGMA user_version = 1;
";
