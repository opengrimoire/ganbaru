use serde::Serialize;
use sqlx::{SqliteConnection, SqlitePool};
use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};

use crate::notes;
use ganbaru_db::run_migrations;

const SQLITE_OK: c_int = 0;
const SQLITE_TRACE_STMT: c_uint = 0x01;
const EMPTY_PROJECTS_SQL_READS: usize = 6;
const EMPTY_PROJECTS_SQL_WRITES: usize = 0;
const EMPTY_PROJECTS_RESPONSE_BYTES: usize = 13_142;
const EMPTY_NOTES_SQL_READS: usize = 7;
const EMPTY_NOTES_SQL_WRITES: usize = 0;
const EMPTY_NOTES_RESPONSE_BYTES: usize = 349;

// SAFETY: This declaration matches SQLite's public C ABI. SQLx links the same
// SQLite library, and callers below pass the locked connection's native handle.
unsafe extern "C" {
    fn sqlite3_trace_v2(
        database: *mut c_void,
        mask: c_uint,
        callback: Option<
            unsafe extern "C" fn(c_uint, *mut c_void, *mut c_void, *mut c_void) -> c_int,
        >,
        context: *mut c_void,
    ) -> c_int;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FirstUseIpcCommand {
    ProjectsLoadWorkspace,
    NotesLoadWorkspaceShell,
}

#[derive(Debug, Default, Eq, PartialEq)]
struct SqlStatementCounts {
    reads: usize,
    writes: usize,
}

#[derive(Debug, Default)]
struct SqlTraceState {
    counts: SqlStatementCounts,
    statements: Vec<String>,
}

#[derive(Debug, Default, Eq, PartialEq)]
struct FirstUseContractMetrics {
    commands: Vec<FirstUseIpcCommand>,
    sql: SqlStatementCounts,
    serialized_response_bytes: usize,
}

impl FirstUseContractMetrics {
    fn record_response<T: Serialize>(&mut self, command: FirstUseIpcCommand, response: &T) {
        self.commands.push(command);
        self.serialized_response_bytes += serde_json::to_vec(response)
            .expect("first-use response should serialize")
            .len();
    }
}

/// Test-only async owner for the SQLite trace registration. Callers must use
/// `finish` before dropping it. Cancellation intentionally leaves the raw Arc
/// reference live because SQLite may still hold the callback context; leaking in
/// that exceptional test path is safer than reclaiming a potentially live pointer.
struct SqlTrace {
    pool: SqlitePool,
    state: Arc<Mutex<SqlTraceState>>,
    context: *const Mutex<SqlTraceState>,
}

impl SqlTrace {
    async fn start(pool: &SqlitePool) -> Self {
        let state = Arc::new(Mutex::new(SqlTraceState::default()));
        let context = Arc::into_raw(Arc::clone(&state));
        let mut connection = pool.acquire().await.expect("acquire trace connection");
        install_trace(
            &mut connection,
            Some(sql_trace_callback),
            context.cast_mut().cast(),
        )
        .await;
        drop(connection);
        Self {
            pool: pool.clone(),
            state,
            context,
        }
    }

    async fn finish(self) -> SqlTraceState {
        let mut connection = self
            .pool
            .acquire()
            .await
            .expect("acquire trace connection for cleanup");
        install_trace(&mut connection, None, std::ptr::null_mut()).await;
        drop(connection);

        // SAFETY: `context` came from `Arc::into_raw` in `start`. This fixture's
        // pool has one connection, SQLite invokes trace callbacks synchronously,
        // and tracing was disabled while that connection was locked. No callback
        // can still borrow the context, so this balances the raw strong reference.
        unsafe {
            drop(Arc::from_raw(self.context));
        }
        Arc::try_unwrap(self.state)
            .expect("trace counter should have one owner")
            .into_inner()
            .expect("trace counter lock should not be poisoned")
    }
}

async fn install_trace(
    connection: &mut SqliteConnection,
    callback: Option<unsafe extern "C" fn(c_uint, *mut c_void, *mut c_void, *mut c_void) -> c_int>,
    context: *mut c_void,
) {
    let mut handle = connection.lock_handle().await.expect("lock SQLite handle");
    // SAFETY: SQLx has locked the connection's live native handle for this call.
    // When installing, `context` is backed by a raw Arc strong reference retained
    // by `SqlTrace`; when removing, both callback and context are null. SQLite
    // copies those two values but neither retains the database pointer itself.
    let result = unsafe {
        sqlite3_trace_v2(
            handle.as_raw_handle().as_ptr().cast(),
            if callback.is_some() {
                SQLITE_TRACE_STMT
            } else {
                0
            },
            callback,
            context,
        )
    };
    assert_eq!(result, SQLITE_OK, "install SQLite statement trace");
}

fn run_sql_trace_callback_body(callback: impl FnOnce()) {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(callback)) {
        // A panic payload can have a destructor that also panics. This callback
        // is test-only and panic is already exceptional, so leak the payload
        // instead of risking a second unwind across SQLite's C ABI.
        std::mem::forget(payload);
    }
}

// SAFETY: SQLite invokes this function only for the installed trace mask and
// supplies callback-duration pointers. The body checks nullable inputs and catches
// Rust panics so no unwind can cross the C ABI boundary.
unsafe extern "C" fn sql_trace_callback(
    event: c_uint,
    context: *mut c_void,
    statement: *mut c_void,
    sql: *mut c_void,
) -> c_int {
    if event != SQLITE_TRACE_STMT || context.is_null() || statement.is_null() || sql.is_null() {
        return SQLITE_OK;
    }
    run_sql_trace_callback_body(|| {
        // SAFETY: For SQLITE_TRACE_STMT, SQLite supplies the original SQL as a
        // NUL-terminated string that remains valid for this callback invocation.
        let sql = unsafe { CStr::from_ptr(sql.cast::<c_char>()) }.to_string_lossy();
        let Some(kind) = statement_kind(&sql) else {
            return;
        };
        // SAFETY: `context` is the pointer created by `Arc::into_raw` in
        // `SqlTrace::start`. That strong reference is retained until tracing is
        // disabled, so the allocation remains live throughout this callback.
        let state = unsafe { &*context.cast::<Mutex<SqlTraceState>>() };
        let Ok(mut state) = state.lock() else {
            return;
        };
        state.statements.push(sql.into_owned());
        match kind {
            StatementKind::Read => state.counts.reads += 1,
            StatementKind::Write => state.counts.writes += 1,
        }
    });
    SQLITE_OK
}

#[test]
fn sql_trace_callback_rejects_invalid_inputs_and_poisoned_state() {
    let mut marker = ();
    let marker_pointer = (&mut marker as *mut ()).cast::<c_void>();
    // SAFETY: Each call has an invalid event or a null required pointer, so the
    // callback returns before borrowing any supplied address.
    unsafe {
        assert_eq!(
            sql_trace_callback(0, marker_pointer, marker_pointer, marker_pointer),
            SQLITE_OK
        );
        assert_eq!(
            sql_trace_callback(
                SQLITE_TRACE_STMT,
                std::ptr::null_mut(),
                marker_pointer,
                marker_pointer,
            ),
            SQLITE_OK
        );
        assert_eq!(
            sql_trace_callback(
                SQLITE_TRACE_STMT,
                marker_pointer,
                std::ptr::null_mut(),
                marker_pointer,
            ),
            SQLITE_OK
        );
        assert_eq!(
            sql_trace_callback(
                SQLITE_TRACE_STMT,
                marker_pointer,
                marker_pointer,
                std::ptr::null_mut(),
            ),
            SQLITE_OK
        );
    }

    let state = Arc::new(Mutex::new(SqlTraceState::default()));
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let _guard = state.lock().expect("fixture state should lock once");
        panic!("poison trace state fixture");
    }));
    let context = Arc::into_raw(Arc::clone(&state));
    let sql = std::ffi::CString::new("SELECT 1").expect("fixture SQL should be valid");
    // SAFETY: `context` is backed by a retained raw Arc reference, `marker_pointer`
    // is non-null but never dereferenced, and `sql` is a live NUL-terminated string.
    // The callback is synchronous and returns without panicking on the poisoned lock.
    unsafe {
        assert_eq!(
            sql_trace_callback(
                SQLITE_TRACE_STMT,
                context.cast_mut().cast(),
                marker_pointer,
                sql.as_ptr().cast_mut().cast(),
            ),
            SQLITE_OK
        );
        drop(Arc::from_raw(context));
    }
    assert!(state.lock().is_err());
}

#[test]
fn sql_trace_callback_barrier_forgets_panicking_payloads() {
    struct PanicOnDrop(Arc<std::sync::atomic::AtomicBool>);

    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            panic!("panic payload drop fixture");
        }
    }

    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let payload = PanicOnDrop(Arc::clone(&dropped));
    run_sql_trace_callback_body(|| std::panic::panic_any(payload));
    assert!(!dropped.load(std::sync::atomic::Ordering::SeqCst));
}

#[derive(Clone, Copy)]
enum StatementKind {
    Read,
    Write,
}

fn statement_kind(sql: &str) -> Option<StatementKind> {
    let first_word = sql
        .trim_start()
        .split_ascii_whitespace()
        .next()?
        .to_ascii_uppercase();
    match first_word.as_str() {
        "SELECT" | "WITH" | "EXPLAIN" => Some(StatementKind::Read),
        "INSERT" | "UPDATE" | "DELETE" | "REPLACE" | "CREATE" | "DROP" | "ALTER" => {
            Some(StatementKind::Write)
        }
        _ => None,
    }
}

async fn migrated_empty_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("connect empty first-use fixture");
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .expect("enable foreign keys");
    run_migrations(&pool)
        .await
        .expect("migrate empty first-use fixture");
    pool
}

async fn assert_no_user_content(pool: &SqlitePool) {
    for table in [
        "notes_pages",
        "notes_blocks",
        "notes_folders",
        "notes_page_templates",
        "project_tasks",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(pool)
            .await
            .unwrap_or_else(|error| panic!("count {table}: {error}"));
        assert_eq!(count, 0, "fixture should have no rows in {table}");
    }
}

async fn traced_projects_workspace(
    pool: &SqlitePool,
    preferred_project_id: Option<&str>,
) -> (serde_json::Value, FirstUseContractMetrics, Vec<String>) {
    let trace = SqlTrace::start(pool).await;
    let mut metrics = FirstUseContractMetrics::default();
    let response = ganbaru_projects::workspace::load_workspace(
        pool,
        preferred_project_id,
        ganbaru_projects::ProjectViewId::List,
    )
    .await;
    let trace = trace.finish().await;
    let response = response.expect("load Projects workspace");
    metrics.record_response(FirstUseIpcCommand::ProjectsLoadWorkspace, &response);
    let response = serde_json::to_value(response).expect("serialize Projects workspace");
    metrics.sql = trace.counts;
    (response, metrics, trace.statements)
}

fn assert_no_optional_project_queries(statements: &[String]) {
    for table in [
        "project_checklist_items",
        "project_tags",
        "project_task_tag_links",
        "project_custom_fields",
        "project_custom_field_options",
        "project_custom_field_values",
        "project_custom_field_option_values",
        "project_task_dependencies",
        "project_task_event_links",
        "project_task_change_events",
        "project_view_preferences",
        "project_custom_emojis",
    ] {
        assert!(
            statements
                .iter()
                .all(|statement| !statement.contains(table)),
            "initial Projects workspace queried optional table {table}",
        );
    }
}

#[test]
fn empty_projects_first_use_has_a_fixed_backend_contract() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_empty_pool().await;
        assert_no_user_content(&pool).await;

        let (response, metrics, statements) = traced_projects_workspace(&pool, None).await;
        assert_eq!(response["resolved_project_id"], "project-routine-learning");
        assert_eq!(response["active_view"], "list");
        assert_eq!(response["snapshot"]["tasks"], serde_json::json!([]));
        assert_no_optional_project_queries(&statements);
        assert_eq!(
            metrics,
            FirstUseContractMetrics {
                commands: vec![FirstUseIpcCommand::ProjectsLoadWorkspace],
                sql: SqlStatementCounts {
                    reads: EMPTY_PROJECTS_SQL_READS,
                    writes: EMPTY_PROJECTS_SQL_WRITES,
                },
                serialized_response_bytes: EMPTY_PROJECTS_RESPONSE_BYTES,
            }
        );
    });
}

#[test]
fn projects_first_use_resolves_invalid_preferred_project_without_repair() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_empty_pool().await;
        let (response, metrics, statements) =
            traced_projects_workspace(&pool, Some("missing-project")).await;

        assert_eq!(response["resolved_project_id"], "project-routine-learning");
        assert_eq!(
            metrics.commands,
            vec![FirstUseIpcCommand::ProjectsLoadWorkspace]
        );
        assert_eq!(metrics.sql.writes, 0);
        assert_no_optional_project_queries(&statements);
    });
}

#[test]
fn projects_first_use_keeps_a_valid_saved_project() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_empty_pool().await;
        let (response, metrics, statements) =
            traced_projects_workspace(&pool, Some("project-routine-sleep")).await;

        assert_eq!(response["resolved_project_id"], "project-routine-sleep");
        assert_eq!(
            metrics.commands,
            vec![FirstUseIpcCommand::ProjectsLoadWorkspace]
        );
        assert_eq!(metrics.sql.writes, 0);
        assert_no_optional_project_queries(&statements);
    });
}

#[test]
fn projects_first_use_repairs_a_missing_built_in_with_default_graph() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_empty_pool().await;
        sqlx::query("DELETE FROM projects WHERE id = 'project-routine-reading'")
            .execute(&pool)
            .await
            .expect("remove built-in Reading project");

        let (response, metrics, statements) = traced_projects_workspace(&pool, None).await;
        let restored = response["snapshot"]["projects"]
            .as_array()
            .expect("Projects rows")
            .iter()
            .find(|project| project["id"] == "project-routine-reading")
            .expect("restored Reading project");
        assert_eq!(restored["color"], 25);
        let restored_sections: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM project_sections WHERE project_id = 'project-routine-reading'",
        )
        .fetch_one(&pool)
        .await
        .expect("count restored sections");
        let restored_statuses: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM project_statuses WHERE project_id = 'project-routine-reading'",
        )
        .fetch_one(&pool)
        .await
        .expect("count restored statuses");
        let restored_priorities: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM project_priorities WHERE project_id = 'project-routine-reading'",
        )
        .fetch_one(&pool)
        .await
        .expect("count restored priorities");
        assert_eq!(
            (restored_sections, restored_statuses, restored_priorities),
            (1, 6, 4)
        );
        assert_eq!(metrics.sql.writes, 19);
        assert_no_optional_project_queries(&statements);
    });
}

#[test]
fn project_refresh_does_not_run_built_in_repair() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_empty_pool().await;
        sqlx::query("DELETE FROM projects WHERE id = 'project-routine-reading'")
            .execute(&pool)
            .await
            .expect("remove built-in Reading project");
        let trace = SqlTrace::start(&pool).await;
        let refresh = ganbaru_projects::workspace::refresh_workspace(
            &pool,
            Some("project-routine-learning"),
            ganbaru_projects::ProjectViewId::List,
        )
        .await;
        let trace = trace.finish().await;
        refresh.expect("refresh Projects workspace");

        let restored: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM projects WHERE id = 'project-routine-reading'",
        )
        .fetch_one(&pool)
        .await
        .expect("count Reading project after refresh");
        assert_eq!(restored, 0);
        assert_eq!(trace.counts.writes, 0);
        assert!(
            trace
                .statements
                .iter()
                .all(|statement| !statement.contains("SELECT EXISTS(SELECT 1 FROM project_groups")),
        );
        assert_no_optional_project_queries(&trace.statements);
    });
}

#[test]
fn projects_first_use_normalizes_identity_without_overwriting_authored_values() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_empty_pool().await;
        sqlx::query("UPDATE project_groups SET name = 'Renamed' WHERE id = 'group-routine'")
            .execute(&pool)
            .await
            .expect("rename Routine group");
        sqlx::query(
            "UPDATE projects
             SET name = 'Meals', sort_order = 999, icon = 'utensils', color = 11,
                 status = 'hidden', default_event_name = 'Lunch'
             WHERE id = 'project-routine-eat'",
        )
        .execute(&pool)
        .await
        .expect("customize Eating project");

        let (_response, metrics, statements) = traced_projects_workspace(&pool, None).await;
        let row: (String, i64, String, i64, String, Option<String>) = sqlx::query_as(
            "SELECT name, sort_order, icon, color, status, default_event_name
             FROM projects WHERE id = 'project-routine-eat'",
        )
        .fetch_one(&pool)
        .await
        .expect("load normalized Eating project");
        assert_eq!(
            row,
            (
                "Eating".to_string(),
                40,
                "utensils".to_string(),
                11,
                "hidden".to_string(),
                Some("Lunch".to_string()),
            ),
        );
        assert_eq!(metrics.sql.writes, 16);
        assert_no_optional_project_queries(&statements);
    });
}

#[test]
fn empty_notes_first_use_has_a_fixed_backend_contract() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_empty_pool().await;
        assert_no_user_content(&pool).await;

        let trace = SqlTrace::start(&pool).await;
        let mut metrics = FirstUseContractMetrics::default();
        let shell = notes::load_workspace_shell_for_first_use_contract(&pool).await;
        let trace = trace.finish().await;
        let shell = shell.expect("load Notes workspace shell");
        metrics.record_response(FirstUseIpcCommand::NotesLoadWorkspaceShell, &shell);
        metrics.sql = trace.counts;
        let response = serde_json::to_value(&shell).expect("serialize Notes workspace shell");
        assert_eq!(response["navigation_databases"], serde_json::json!([]));
        for optional_table in [
            "notes_blocks",
            "notes_databases",
            "notes_data_sources",
            "notes_database_views",
            "notes_page_templates",
            "notes_local_users",
            "notes_page_history",
            "notes_project_history",
            "notes_history_maintenance_state",
            "notes_comments",
            "notes_suggestions",
            "notes_backlinks",
            "notes_page_aliases",
            "notes_unresolved_links",
            "notes_undo_state",
        ] {
            assert!(
                trace
                    .statements
                    .iter()
                    .all(|statement| !statement.contains(optional_table)),
                "empty Notes shell queried auxiliary table {optional_table}",
            );
        }

        assert_eq!(
            metrics,
            FirstUseContractMetrics {
                commands: vec![FirstUseIpcCommand::NotesLoadWorkspaceShell],
                sql: SqlStatementCounts {
                    reads: EMPTY_NOTES_SQL_READS,
                    writes: EMPTY_NOTES_SQL_WRITES,
                },
                serialized_response_bytes: EMPTY_NOTES_RESPONSE_BYTES,
            }
        );
    });
}
