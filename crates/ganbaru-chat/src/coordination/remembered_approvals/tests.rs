use super::*;
use crate::coordination::reports::tests::{AGENT, NOW, OWNER, PROJECT_ID, migrated_pool, seed};
use crate::project_defaults::default_channel_ids;
use serde_json::json;
use sqlx::SqlitePool;

const LATER: &str = "2026-11-12T09:05:00.000Z";
const THREAD: &str = "thread:reports";

fn url_rule(key: &str) -> Option<ChatRememberableApprovalRead> {
    Some(ChatRememberableApprovalRead {
        kind: ChatRememberedApprovalKind::UrlOrigin,
        key: key.to_string(),
    })
}

/// The persisted display data of a request whose subject is `kind` and `text`.
fn display(kind: &str, text: &str) -> Value {
    json!({
        "title": "Allow?",
        "detail": null,
        "kind": "network",
        "payload": {"schemaVersion": 1, "value": {"toolName": "WebFetch", "hasBlockedPath": false}},
        "subject": {"kind": kind, "text": text, "location": null, "truncated": false},
        "sessionGrants": [],
    })
}

#[test]
fn web_addresses_are_remembered_by_their_public_origin() {
    let cases = [
        (
            "https://Docs.Example.com/guide?page=2#top",
            "https://docs.example.com",
        ),
        ("http://example.com:8080/x", "http://example.com:8080"),
        ("https://example.com:443/", "https://example.com"),
        ("https://bücher.example/", "https://xn--bcher-kva.example"),
        // Userinfo is not the host, so the rule names the host the request reaches.
        (
            "https://docs.example.com@evil.example/",
            "https://evil.example",
        ),
    ];
    for (address, origin) in cases {
        assert_eq!(
            url_origin_key(address).as_deref(),
            Some(origin),
            "{address}"
        );
    }
}

#[test]
fn private_numeric_and_non_web_addresses_are_never_remembered() {
    for address in [
        "http://127.0.0.1/",
        "http://0x7f.1/",
        "http://2130706433/",
        "http://[::1]/",
        "http://localhost:3000/",
        "http://app.localhost/",
        "http://foo.localhost./",
        "http://printer.local/",
        "http://x.local./",
        "http://router.lan/",
        "http://service.internal/",
        "http://nas.home.arpa/",
        "http://intranet/",
        "https://example.com./",
        "ftp://example.com/",
        "file:///etc/passwd",
        "not an address",
    ] {
        assert_eq!(url_origin_key(address), None, "{address}");
    }
}

#[test]
fn single_commands_are_remembered_exactly() {
    for command in [
        "cargo test -p ganbaru-chat",
        "git status",
        "npm run build -- --watch",
    ] {
        assert_eq!(command_key(command).as_deref(), Some(command));
    }
    let longest = "a".repeat(MAX_REMEMBERED_APPROVAL_SUBJECT_BYTES);
    assert_eq!(command_key(&longest), Some(longest.clone()));
    for command in [
        "",
        "   ",
        "git status; rm -rf .",
        "make && make install",
        "cat file | sh",
        "echo $HOME",
        "echo `id`",
        "echo $(id)",
        "sort < input",
        "echo hi > out",
        "run {a,b}",
        "git status\nrm -rf .",
        "git status \\\nrm -rf .",
        "git\tstatus",
    ] {
        assert_eq!(command_key(command), None, "{command:?}");
    }
    assert_eq!(command_key(&format!("{longest}a")), None);
}

#[test]
fn tools_are_remembered_by_their_exact_name() {
    assert_eq!(
        tool_key("mcp__docs__search").as_deref(),
        Some("mcp__docs__search")
    );
    assert_eq!(tool_key(""), None);
    assert_eq!(tool_key("two words"), None);
}

#[test]
fn only_whole_url_command_and_tool_subjects_offer_a_rule() {
    assert_eq!(
        rememberable_approval(&display("url", "https://docs.example.com/a")),
        url_rule("https://docs.example.com")
    );
    assert_eq!(
        rememberable_approval(&display("command", "git status")),
        Some(ChatRememberableApprovalRead {
            kind: ChatRememberedApprovalKind::Command,
            key: "git status".to_string(),
        })
    );
    assert_eq!(
        rememberable_approval(&display("tool", "mcp__docs__search")).map(|rule| rule.kind),
        Some(ChatRememberedApprovalKind::Tool)
    );
    for kind in ["path", "pattern", "query"] {
        assert_eq!(rememberable_approval(&display(kind, "src")), None, "{kind}");
    }

    let mut truncated = display("command", "git status");
    truncated["subject"]["truncated"] = json!(true);
    assert_eq!(rememberable_approval(&truncated), None);
    let mut unknown_truncation = display("command", "git status");
    unknown_truncation["subject"]
        .as_object_mut()
        .unwrap()
        .remove("truncated");
    assert_eq!(rememberable_approval(&unknown_truncation), None);
    let mut blocked = display("command", "git status");
    blocked["payload"]["value"]["hasBlockedPath"] = json!(true);
    assert_eq!(rememberable_approval(&blocked), None);
    let mut missing = display("command", "git status");
    missing["subject"] = Value::Null;
    assert_eq!(rememberable_approval(&missing), None);
}

fn conversation_id() -> String {
    default_channel_ids(PROJECT_ID).unwrap().0
}

async fn execute(pool: &SqlitePool, sql: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(pool)
        .await
        .unwrap();
}

/// Seeds a working run of the helper in the default channel, with the helper a present member
/// and a person to approve.
async fn seed_channel_run(pool: &SqlitePool) {
    seed(pool, "working", "working").await;
    let conversation = conversation_id();
    execute(
        pool,
        &format!(
            "INSERT OR IGNORE INTO chat_participants (id, participant_kind, display_name, created_at, updated_at)
             VALUES ('{OWNER}', 'local_user', 'Me', '{NOW}', '{NOW}');
             INSERT INTO chat_conversation_memberships
                (conversation_id, participant_id, sync_key, membership_role, created_at, updated_at)
             VALUES ('{conversation}', '{AGENT}', 'membership:helper', 'member', '{NOW}', '{NOW}');
             INSERT INTO chat_agent_channel_memberships
                (conversation_id, agent_id, sync_key, created_at, updated_at)
             VALUES ('{conversation}', '{AGENT}', 'channel-membership:helper', '{NOW}', '{NOW}');"
        ),
    )
    .await;
}

/// Starts a later run of the helper in a working folder, written as the sync applier would
/// since run authority never changes.
const RUN_IN_FOLDER: &str = "CREATE TEMP TABLE later_run AS SELECT * FROM chat_agent_runs;
     UPDATE later_run SET id = 'run:folder', run_ordinal = 2, working_folder_id = 'folder:code',
         execution_environment_id = 'environment:local', provider_turn_id = 'turn:folder';
     UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
     INSERT INTO chat_agent_runs SELECT * FROM later_run;
     UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;
     DROP TABLE later_run;";

async fn set_channel_access(pool: &SqlitePool, access: &str) {
    execute(
        pool,
        &format!(
            "UPDATE chat_channels SET agent_access = '{access}', revision = revision + 1
             WHERE conversation_id = '{}'",
            conversation_id()
        ),
    )
    .await;
}

#[tokio::test]
async fn only_runs_that_work_in_a_channel_have_a_scope() {
    let pool = migrated_pool().await;
    seed_channel_run(&pool).await;
    let mut connection = pool.acquire().await.unwrap();
    assert_eq!(
        channel_run_scope(&mut connection, THREAD).await.unwrap(),
        None
    );
    assert_eq!(
        channel_run_scope(&mut connection, "thread:missing")
            .await
            .unwrap(),
        None
    );
    sqlx::raw_sql(RUN_IN_FOLDER)
        .execute(&mut *connection)
        .await
        .unwrap();
    assert_eq!(
        channel_run_scope(&mut connection, THREAD).await.unwrap(),
        Some(RememberedApprovalScope {
            conversation_id: conversation_id(),
            agent_id: AGENT.to_string(),
            access: ChatAgentAccess::Own,
        })
    );
}

#[tokio::test]
async fn an_approval_holds_until_access_drops_below_what_it_was_granted_under_or_it_is_revoked() {
    let pool = migrated_pool().await;
    seed_channel_run(&pool).await;
    set_channel_access(&pool, "ask").await;
    execute(&pool, RUN_IN_FOLDER).await;
    let mut connection = pool.acquire().await.unwrap();
    let scope = channel_run_scope(&mut connection, THREAD)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(scope.access, ChatAgentAccess::Ask);
    let rule = url_rule("https://docs.example.com").unwrap();
    let other = url_rule("https://other.example.com").unwrap();
    assert!(
        !approval_is_live(&mut connection, &scope, &rule)
            .await
            .unwrap()
    );

    remember_approval(&mut connection, &scope, &rule, OWNER, NOW)
        .await
        .unwrap();
    assert!(
        approval_is_live(&mut connection, &scope, &rule)
            .await
            .unwrap()
    );
    assert!(
        !approval_is_live(&mut connection, &scope, &other)
            .await
            .unwrap()
    );
    drop(connection);

    set_channel_access(&pool, "own").await;
    let mut connection = pool.acquire().await.unwrap();
    assert!(
        approval_is_live(&mut connection, &scope, &rule)
            .await
            .unwrap()
    );
    drop(connection);
    set_channel_access(&pool, "read").await;
    let mut connection = pool.acquire().await.unwrap();
    assert!(
        !approval_is_live(&mut connection, &scope, &rule)
            .await
            .unwrap()
    );
    drop(connection);

    set_channel_access(&pool, "ask").await;
    let mut connection = pool.acquire().await.unwrap();
    assert!(
        approval_is_live(&mut connection, &scope, &rule)
            .await
            .unwrap()
    );
    let listed = read_channel_remembered_approvals(&mut connection, &conversation_id())
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].kind, ChatRememberedApprovalKind::UrlOrigin);
    assert_eq!(listed[0].key, "https://docs.example.com");
    assert_eq!(listed[0].agent_id.as_str(), AGENT);

    revoke_remembered_approval(&mut connection, &listed[0].approval_id, LATER)
        .await
        .unwrap();
    assert!(
        !approval_is_live(&mut connection, &scope, &rule)
            .await
            .unwrap()
    );
    assert!(
        read_channel_remembered_approvals(&mut connection, &conversation_id())
            .await
            .unwrap()
            .is_empty()
    );

    // Approving again clears the revocation under the same id.
    remember_approval(&mut connection, &scope, &rule, OWNER, LATER)
        .await
        .unwrap();
    assert!(
        approval_is_live(&mut connection, &scope, &rule)
            .await
            .unwrap()
    );
    let again = read_channel_remembered_approvals(&mut connection, &conversation_id())
        .await
        .unwrap();
    assert_eq!(again[0].approval_id, listed[0].approval_id);
}

#[tokio::test]
async fn approvals_list_newest_first_and_agents_cannot_approve() {
    let pool = migrated_pool().await;
    seed_channel_run(&pool).await;
    execute(&pool, RUN_IN_FOLDER).await;
    let mut connection = pool.acquire().await.unwrap();
    let scope = channel_run_scope(&mut connection, THREAD)
        .await
        .unwrap()
        .unwrap();
    let older = url_rule("https://docs.example.com").unwrap();
    let newer = ChatRememberableApprovalRead {
        kind: ChatRememberedApprovalKind::Command,
        key: "git status".to_string(),
    };
    remember_approval(&mut connection, &scope, &older, OWNER, NOW)
        .await
        .unwrap();
    remember_approval(&mut connection, &scope, &newer, OWNER, LATER)
        .await
        .unwrap();
    let listed = read_channel_remembered_approvals(&mut connection, &conversation_id())
        .await
        .unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|approval| approval.key.as_str())
            .collect::<Vec<_>>(),
        ["git status", "https://docs.example.com"]
    );

    let by_agent = ChatRememberableApprovalRead {
        kind: ChatRememberedApprovalKind::Tool,
        key: "mcp__docs__search".to_string(),
    };
    assert!(
        remember_approval(&mut connection, &scope, &by_agent, AGENT, LATER)
            .await
            .is_err()
    );
}
