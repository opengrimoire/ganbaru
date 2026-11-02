//! Domain trigger policies against the triggers installed in each manifest's database.
//!
//! Every non-engine trigger on a converted, `Local`, or domain engine table is declared with a
//! [`TriggerPolicy`], and its SQL is scanned for what the policy allows, including the tables an
//! `ApplierVisible` trigger reads for the dependency order. Triggers on unconverted
//! tables that write converted tables must stay quiet while the engine applies.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::TableId;
use sqlx::SqlitePool;

use super::block_on;
use super::conformance::{Token, cases, tokenize};
use crate::manifest::order::{Dependency, DependencyKind};
use crate::manifest::vault::VAULT_MANIFEST;
use crate::manifest::{Classification, DomainTrigger, Manifest, TriggerPolicy};
use crate::triggers::NOT_APPLYING;

/// Name prefix of the triggers the engine renders from the manifest.
const ENGINE_TRIGGER_PREFIX: &str = "sync_";

/// A trigger split into the table it fires on, its `WHEN` condition, and its body.
#[derive(Debug)]
struct TriggerShape {
    table: String,
    when: Vec<Token>,
    body: Vec<Token>,
}

/// One write statement of a trigger body.
#[derive(Debug, PartialEq, Eq)]
struct Write {
    kind: WriteKind,
    table: String,
    /// Columns an update assigns.
    columns: Vec<String>,
    /// Whether the statement filters by a column of `NEW` or `OLD`.
    keyed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteKind {
    Insert,
    Update,
    Delete,
}

fn is_word(token: Option<&Token>, word: &str) -> bool {
    matches!(token, Some(Token::Word(text)) if text == word)
}

fn word_at(tokens: &[Token], index: usize) -> Option<&str> {
    match tokens.get(index) {
        Some(Token::Word(text)) => Some(text),
        _ => None,
    }
}

/// The table name at `index`, skipping a schema prefix, and the index after it.
fn table_at(tokens: &[Token], index: usize) -> Option<(String, usize)> {
    if tokens.get(index + 1) == Some(&Token::Symbol('.')) {
        return Some((word_at(tokens, index + 2)?.to_string(), index + 3));
    }
    Some((word_at(tokens, index)?.to_string(), index + 1))
}

fn shape(sql: &str) -> Option<TriggerShape> {
    let tokens = tokenize(sql);
    let on = tokens.iter().position(|token| is_word(Some(token), "on"))?;
    let (table, after_table) = table_at(&tokens, on + 1)?;
    let begin = after_table
        + tokens[after_table..]
            .iter()
            .position(|token| is_word(Some(token), "begin"))?;
    let end = tokens
        .iter()
        .rposition(|token| is_word(Some(token), "end"))?;
    let when = tokens[after_table..begin]
        .iter()
        .position(|token| is_word(Some(token), "when"))
        .map(|offset| tokens[after_table + offset + 1..begin].to_vec())
        .unwrap_or_default();
    Some(TriggerShape {
        table,
        when,
        body: tokens.get(begin + 1..end)?.to_vec(),
    })
}

/// Index of the `;` that ends the statement starting at `start`, or the body length.
fn statement_end(body: &[Token], start: usize) -> usize {
    let mut depth = 0_i32;
    for (index, token) in body.iter().enumerate().skip(start) {
        match token {
            Token::Symbol('(') => depth += 1,
            Token::Symbol(')') => depth -= 1,
            Token::Symbol(';') if depth == 0 => return index,
            _ => {}
        }
    }
    body.len()
}

fn keyed(statement: &[Token]) -> bool {
    statement
        .iter()
        .position(|token| is_word(Some(token), "where"))
        .is_some_and(|start| {
            statement[start..]
                .iter()
                .any(|token| is_word(Some(token), "new") || is_word(Some(token), "old"))
        })
}

/// Columns assigned by an `UPDATE ... SET` statement.
fn assigned_columns(statement: &[Token]) -> Vec<String> {
    let Some(set) = statement
        .iter()
        .position(|token| is_word(Some(token), "set"))
    else {
        return Vec::new();
    };
    let mut columns = Vec::new();
    let mut depth = 0_i32;
    for index in set + 1..statement.len() {
        match &statement[index] {
            Token::Symbol('(') => depth += 1,
            Token::Symbol(')') => depth -= 1,
            Token::Word(word)
                if depth == 0 && matches!(word.as_str(), "where" | "from" | "returning") =>
            {
                break;
            }
            Token::Word(word)
                if depth == 0
                    && statement.get(index + 1) == Some(&Token::Symbol('='))
                    && (index == set + 1 || statement[index - 1] == Token::Symbol(',')) =>
            {
                columns.push(word.clone());
            }
            _ => {}
        }
    }
    columns
}

fn writes(body: &[Token]) -> Vec<Write> {
    let mut writes = Vec::new();
    for (index, token) in body.iter().enumerate() {
        let Token::Word(word) = token else { continue };
        let statement = &body[index..statement_end(body, index)];
        let write = match word.as_str() {
            "insert" => statement
                .iter()
                .position(|token| is_word(Some(token), "into"))
                .and_then(|into| table_at(statement, into + 1))
                .map(|(table, _)| (WriteKind::Insert, table)),
            "replace"
                if is_word(body.get(index + 1), "into")
                    && !is_word(index.checked_sub(1).and_then(|i| body.get(i)), "or") =>
            {
                table_at(statement, 2).map(|(table, _)| (WriteKind::Insert, table))
            }
            "update" if !is_word(body.get(index + 1), "set") => {
                let start = if is_word(body.get(index + 1), "or") {
                    3
                } else {
                    1
                };
                table_at(statement, start).map(|(table, _)| (WriteKind::Update, table))
            }
            "delete" if is_word(body.get(index + 1), "from") => {
                table_at(statement, 2).map(|(table, _)| (WriteKind::Delete, table))
            }
            _ => None,
        };
        if let Some((kind, table)) = write {
            writes.push(Write {
                kind,
                table,
                columns: if kind == WriteKind::Update {
                    assigned_columns(statement)
                } else {
                    Vec::new()
                },
                keyed: keyed(statement),
            });
        }
    }
    writes
}

/// Whether a `WHEN` condition holds only while the engine is not applying: it contains the
/// apply check and joins nothing to it with a top-level `OR`.
fn is_gated(when: &[Token]) -> bool {
    let check = tokenize(NOT_APPLYING);
    let contains = when.windows(check.len()).any(|window| window == check);
    let mut depth = 0_i32;
    let top_level_or = when.iter().any(|token| {
        match token {
            Token::Symbol('(') => depth += 1,
            Token::Symbol(')') => depth -= 1,
            _ => {}
        }
        depth == 0 && is_word(Some(token), "or")
    });
    contains && !top_level_or
}

/// Columns a same-row trigger on `table` may write: every column of a `Local` or engine
/// table, and the `Local` and derived columns of a replicated table.
fn same_row_columns(manifest: &Manifest, table: &str) -> Option<BTreeSet<String>> {
    match manifest.classify(table) {
        Classification::Local | Classification::Engine => None,
        _ => Some(
            manifest
                .table_named(table)
                .map(|spec| {
                    spec.local_columns
                        .iter()
                        .map(|column| column.name)
                        .chain(spec.derived_columns.iter().copied())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
        ),
    }
}

/// Why a trigger breaks its declared policy, or `None` when it keeps it.
fn policy_violation(manifest: &Manifest, policy: TriggerPolicy, sql: &str) -> Option<String> {
    let Some(shape) = shape(sql) else {
        return Some("trigger SQL does not parse".to_string());
    };
    let writes = writes(&shape.body);
    match policy {
        TriggerPolicy::SameRow => {
            let allowed = same_row_columns(manifest, &shape.table);
            for write in &writes {
                if write.kind != WriteKind::Update || write.table != shape.table {
                    return Some(format!("writes {} outside its own row", write.table));
                }
                if !write.keyed || write.columns.is_empty() {
                    return Some("updates rows other than its own".to_string());
                }
                if let Some(allowed) = &allowed
                    && let Some(column) = write
                        .columns
                        .iter()
                        .find(|column| !allowed.contains(*column))
                {
                    return Some(format!("writes replicated column {column}"));
                }
            }
            None
        }
        TriggerPolicy::Gated => (!is_gated(&shape.when))
            .then(|| "its WHEN clause does not require that the engine is not applying".into()),
        TriggerPolicy::ApplierVisible => {
            if let Some(write) = writes.first() {
                Some(format!("writes {}", write.table))
            } else if !shape.body.iter().any(|token| is_word(Some(token), "raise")) {
                Some("never aborts".to_string())
            } else {
                None
            }
        }
        TriggerPolicy::DerivedIndex => {
            if writes.is_empty() {
                return Some("writes no derived table".to_string());
            }
            writes
                .iter()
                .find(|write| manifest.classify(&write.table) != Classification::Derived)
                .map(|write| format!("writes {}, which is not derived", write.table))
        }
    }
}

/// Declared domain triggers by name, with the tables each may be installed on.
fn declared_triggers(
    manifest: &Manifest,
) -> BTreeMap<&'static str, (DomainTrigger, Vec<&'static str>)> {
    let mut declared = BTreeMap::new();
    let mut declare = |trigger: &DomainTrigger, tables: Vec<&'static str>| {
        assert!(
            !trigger.name.starts_with(ENGINE_TRIGGER_PREFIX),
            "{}: domain triggers do not use the engine prefix",
            trigger.name
        );
        assert!(
            declared.insert(trigger.name, (*trigger, tables)).is_none(),
            "{}: declared twice",
            trigger.name
        );
    };
    for table in manifest.tables {
        let covered = std::iter::once(table.name)
            .chain(table.owned_children().map(|(_, owned)| owned.table))
            .collect::<Vec<_>>();
        for trigger in table.triggers {
            declare(trigger, covered.clone());
        }
    }
    for domain in manifest.domains {
        for table in domain.local.iter().chain(domain.engine) {
            for trigger in table.triggers {
                declare(trigger, vec![table.name]);
            }
        }
    }
    declared
}

/// `(name, table, sql)` of every installed trigger.
async fn all_triggers(pool: &SqlitePool) -> Vec<(String, String, String)> {
    sqlx::query_as(
        "SELECT name, tbl_name, sql FROM sqlite_schema WHERE type = 'trigger' ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

/// `(name, table, sql)` of every installed trigger outside the engine prefix.
async fn installed_triggers(pool: &SqlitePool) -> Vec<(String, String, String)> {
    let mut triggers = all_triggers(pool).await;
    triggers.retain(|(name, _, _)| !name.starts_with(ENGINE_TRIGGER_PREFIX));
    triggers
}

async fn assert_policies(manifest: &Manifest, pool: &SqlitePool) {
    let declared = declared_triggers(manifest);
    let mut seen = BTreeSet::new();
    for (name, table, sql) in installed_triggers(pool).await {
        if manifest.classify(&table) == Classification::Unconverted {
            let Some(shape) = shape(&sql) else {
                panic!("{name}: trigger SQL does not parse");
            };
            let writes_converted = writes(&shape.body).iter().any(|write| {
                matches!(
                    manifest.classify(&write.table),
                    Classification::Replicated | Classification::OwnedChildren
                )
            });
            assert!(
                !writes_converted || is_gated(&shape.when),
                "{name}: an unconverted trigger that writes converted tables must be gated"
            );
            continue;
        }
        let Some((trigger, tables)) = declared.get(name.as_str()) else {
            panic!("{name} on {table} has no declared policy");
        };
        assert!(
            tables.contains(&table.as_str()),
            "{name} is installed on {table}, not on the table that declares it"
        );
        if let Some(problem) = policy_violation(manifest, trigger.policy, &sql) {
            panic!("{name} breaks its {:?} policy: {problem}", trigger.policy);
        }
        seen.insert(name);
    }
    for name in declared.keys() {
        assert!(
            seen.contains(*name),
            "declared trigger {name} is not installed"
        );
    }
}

#[test]
fn domain_triggers_keep_their_declared_policies() {
    block_on(async {
        for (manifest, pool) in cases().await {
            assert_policies(manifest, &pool).await;
        }
    });
}

/// How apply stays clear of an aborting trigger on a replicated table.
#[derive(Debug, Clone, Copy)]
enum TriggerClass {
    /// A validation rule refuses the same value when an operation is stored.
    Stateless(&'static str),
    /// Merge state hides the row before apply writes what the trigger refuses.
    MergeState(&'static str),
    /// Apply reaches the trigger, and the refusal net in materialization catches it.
    Net(&'static str),
}

impl TriggerClass {
    const fn reason(self) -> &'static str {
        match self {
            Self::Stateless(reason) | Self::MergeState(reason) | Self::Net(reason) => reason,
        }
    }
}

const ORDER_KEY: TriggerClass = TriggerClass::Stateless("the order key grammar refuses empty text");
const SYNC_KEY: TriggerClass = TriggerClass::Stateless("the row key grammar refuses empty text");
const ICON_SIZE: TriggerClass =
    TriggerClass::Stateless("an icon group naming a managed file requires its size");
const EMOJI_SIZE: TriggerClass =
    TriggerClass::Stateless("the custom emoji asset group requires its size");
const FOLDER_PATH: TriggerClass =
    TriggerClass::Stateless("a managed folder path must stay contained");
const DESCRIPTION: TriggerClass =
    TriggerClass::Stateless("the column bound is the description limit");
const IMMUTABLE_ROW: TriggerClass =
    TriggerClass::Net("a forged change can rewrite or delete an immutable revision");
const CROSS_ROW: TriggerClass =
    TriggerClass::Net("the rule compares rows that a single change does not carry");

/// Aborting triggers of replicated tables and owned children in the vault manifest that stay
/// armed while apply runs. Triggers gated on the apply state are left out, and so are triggers
/// on device local tables, because apply never writes those tables.
const CLASSIFIED_TRIGGERS: &[(&str, TriggerClass)] = &[
    (
        "calendar_event_archive_import_objects_sync_key_insert",
        SYNC_KEY,
    ),
    (
        "calendar_event_archive_import_objects_sync_key_update",
        SYNC_KEY,
    ),
    (
        "chat_access_profile_builtin_protected",
        TriggerClass::Net("a forged change can edit a built-in profile"),
    ),
    ("chat_access_profile_builtin_revision_protected", CROSS_ROW),
    (
        "chat_access_profile_revision_immutable_delete",
        IMMUTABLE_ROW,
    ),
    (
        "chat_access_profile_revision_immutable_update",
        IMMUTABLE_ROW,
    ),
    ("chat_channels_conversation_kind_insert", CROSS_ROW),
    (
        "chat_communication_message_revision_immutable_update",
        IMMUTABLE_ROW,
    ),
    ("chat_conversation_items_reply_thread_insert", CROSS_ROW),
    (
        "chat_participants_local_user_identity",
        TriggerClass::Stateless("the participant row rule binds the local user to its key"),
    ),
    (
        "chat_teammate_channel_membership_identity_insert",
        TriggerClass::MergeState("the membership cascades from its teammate"),
    ),
    ("chat_teammate_policy_identity_insert", CROSS_ROW),
    ("chat_teammate_policy_immutable_delete", IMMUTABLE_ROW),
    ("chat_teammate_policy_immutable_update", IMMUTABLE_ROW),
    (
        "contact_card_prevent_delete",
        TriggerClass::Net("a forged change can delete the card"),
    ),
    ("music_library_items_description_limits_insert", DESCRIPTION),
    ("music_library_items_description_limits_update", DESCRIPTION),
    ("music_playlist_memberships_order_key_insert", ORDER_KEY),
    ("music_playlist_memberships_order_key_update", ORDER_KEY),
    ("music_playlists_order_key_insert", ORDER_KEY),
    ("music_playlists_order_key_update", ORDER_KEY),
    ("project_checklist_items_order_key_insert", ORDER_KEY),
    ("project_checklist_items_order_key_update", ORDER_KEY),
    ("project_custom_emojis_asset_size_insert", EMOJI_SIZE),
    ("project_custom_emojis_asset_size_update", EMOJI_SIZE),
    ("project_custom_emojis_order_key_insert", ORDER_KEY),
    ("project_custom_emojis_order_key_update", ORDER_KEY),
    ("project_custom_field_options_order_key_insert", ORDER_KEY),
    ("project_custom_field_options_order_key_update", ORDER_KEY),
    ("project_custom_fields_order_key_insert", ORDER_KEY),
    ("project_custom_fields_order_key_update", ORDER_KEY),
    ("project_groups_icon_size_insert", ICON_SIZE),
    ("project_groups_icon_size_update", ICON_SIZE),
    ("project_groups_order_key_insert", ORDER_KEY),
    ("project_groups_order_key_update", ORDER_KEY),
    ("project_priorities_order_key_insert", ORDER_KEY),
    ("project_priorities_order_key_update", ORDER_KEY),
    ("project_sections_order_key_insert", ORDER_KEY),
    ("project_sections_order_key_update", ORDER_KEY),
    ("project_statuses_order_key_insert", ORDER_KEY),
    ("project_statuses_order_key_update", ORDER_KEY),
    ("project_tags_order_key_insert", ORDER_KEY),
    ("project_tags_order_key_update", ORDER_KEY),
    ("project_task_event_links_sync_key_insert", SYNC_KEY),
    ("project_task_event_links_sync_key_update", SYNC_KEY),
    ("project_task_tag_links_sync_key_insert", SYNC_KEY),
    ("project_task_tag_links_sync_key_update", SYNC_KEY),
    ("project_tasks_order_key_insert", ORDER_KEY),
    ("project_tasks_order_key_update", ORDER_KEY),
    ("project_view_preferences_sync_key_insert", SYNC_KEY),
    ("project_view_preferences_sync_key_update", SYNC_KEY),
    ("project_views_order_key_insert", ORDER_KEY),
    ("project_views_order_key_update", ORDER_KEY),
    (
        "project_working_folders_managed_relative_path_shape_insert",
        FOLDER_PATH,
    ),
    (
        "project_working_folders_managed_relative_path_shape_update",
        FOLDER_PATH,
    ),
    ("projects_icon_size_insert", ICON_SIZE),
    ("projects_icon_size_update", ICON_SIZE),
    ("projects_order_key_insert", ORDER_KEY),
    ("projects_order_key_update", ORDER_KEY),
];

fn aborts(sql: &str) -> bool {
    tokenize(sql)
        .iter()
        .any(|token| matches!(token, Token::Word(word) if word.eq_ignore_ascii_case("raise")))
}

#[test]
fn every_applier_visible_trigger_is_classified() {
    block_on(async {
        let (_, pool) = cases().await.into_iter().next().unwrap();
        let mut aborting = BTreeSet::new();
        for (name, table, sql) in all_triggers(&pool).await {
            let replicated = matches!(
                VAULT_MANIFEST.classify(&table),
                Classification::Replicated | Classification::OwnedChildren
            );
            let gated = shape(&sql).is_some_and(|shape| is_gated(&shape.when));
            if replicated && !gated && aborts(&sql) {
                aborting.insert(name);
            }
        }
        let classified: BTreeSet<String> = CLASSIFIED_TRIGGERS
            .iter()
            .map(|(name, class)| {
                assert!(!class.reason().is_empty(), "{name} states no reason");
                (*name).to_string()
            })
            .collect();
        let unclassified: Vec<_> = aborting.difference(&classified).collect();
        let stale: Vec<_> = classified.difference(&aborting).collect();
        assert!(
            unclassified.is_empty() && stale.is_empty(),
            "unclassified triggers: {unclassified:#?}\nstale entries: {stale:#?}"
        );
    });
}

/// The replicated table that owns rows of `name`: the table itself, or the owner of an owned
/// child table.
fn row_owner(manifest: &Manifest, name: &str) -> Option<TableId> {
    manifest.tables.iter().find_map(|table| {
        let owns =
            table.name == name || table.owned_children().any(|(_, owned)| owned.table == name);
        owns.then_some(table.id)
    })
}

/// Dependencies of an `ApplierVisible` trigger installed on `table`: one on every replicated
/// table it names, through owned child tables. A trigger on an owned child table may read its
/// own owner, whose row materializes before its owned rows.
fn applier_dependencies(manifest: &Manifest, table: &str, sql: &str) -> Vec<Dependency> {
    let (Some(from), Some(shape)) = (row_owner(manifest, table), shape(sql)) else {
        return Vec::new();
    };
    let reads_own_owner = |name: &str| manifest.table(from).is_some_and(|spec| spec.name == name);
    let child = manifest.table(from).is_some_and(|spec| spec.name != table);
    shape
        .when
        .iter()
        .chain(&shape.body)
        .filter_map(|token| match token {
            Token::Word(word) if !(child && reads_own_owner(word)) => row_owner(manifest, word),
            _ => None,
        })
        .map(|to| Dependency {
            from,
            to,
            kind: DependencyKind::ApplierVisible,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Owned child tables keep only their parent foreign key, which the conformance tests check,
/// so references and `ApplierVisible` triggers are the dependencies a manifest has today.
async fn assert_dependency_order(manifest: &Manifest, pool: &SqlitePool) {
    let declared = declared_triggers(manifest);
    let mut dependencies = manifest.reference_dependencies();
    for (name, table, sql) in installed_triggers(pool).await {
        if declared
            .get(name.as_str())
            .is_some_and(|(trigger, _)| trigger.policy == TriggerPolicy::ApplierVisible)
        {
            dependencies.extend(applier_dependencies(manifest, &table, &sql));
        }
    }
    let problems = manifest.order_problems(&dependencies);
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn tables_are_listed_in_dependency_order() {
    block_on(async {
        for (manifest, pool) in cases().await {
            assert_dependency_order(manifest, &pool).await;
        }
    });
}

#[test]
fn applier_visible_triggers_depend_on_the_tables_they_read() {
    use crate::manifest::vault::quick_notes::{NOTES_TABLE, TAGS_TABLE};
    let dependency = |from, to| Dependency {
        from,
        to,
        kind: DependencyKind::ApplierVisible,
    };
    let reads_tags = "CREATE TRIGGER t BEFORE INSERT ON quick_notes
        WHEN NOT EXISTS (SELECT 1 FROM main.quick_note_tags WHERE id = NEW.tag_id)
        BEGIN SELECT RAISE(ABORT, 'quick_notes need a tag'); END";
    assert_eq!(
        applier_dependencies(&VAULT_MANIFEST, "quick_notes", reads_tags),
        [dependency(NOTES_TABLE, TAGS_TABLE)]
    );
    let reads_itself = "CREATE TRIGGER t BEFORE INSERT ON quick_notes
        WHEN (SELECT count(*) FROM quick_note_text_runs) > 9
        BEGIN SELECT RAISE(ABORT, 'full'); END";
    assert_eq!(
        applier_dependencies(&VAULT_MANIFEST, "quick_notes", reads_itself),
        [dependency(NOTES_TABLE, NOTES_TABLE)]
    );
    let reads_owner = "CREATE TRIGGER t BEFORE INSERT ON quick_note_text_runs
        WHEN NOT EXISTS (SELECT 1 FROM quick_notes WHERE id = NEW.note_id)
        BEGIN SELECT RAISE(ABORT, 'orphan'); END";
    assert_eq!(
        applier_dependencies(&VAULT_MANIFEST, "quick_note_text_runs", reads_owner),
        []
    );
    let self_loop = VAULT_MANIFEST.order_problems(&applier_dependencies(
        &VAULT_MANIFEST,
        "quick_notes",
        reads_itself,
    ));
    assert!(
        self_loop
            .iter()
            .any(|problem| problem.contains("lies on a cycle")),
        "{self_loop:?}"
    );
}

fn violation(policy: TriggerPolicy, sql: &str) -> Option<String> {
    policy_violation(&VAULT_MANIFEST, policy, sql)
}

#[test]
fn the_policy_scanner_refuses_triggers_that_break_their_policy() {
    let gated_or = format!(
        "CREATE TRIGGER t AFTER INSERT ON quick_notes WHEN {NOT_APPLYING} OR NEW.title = 'x'
         BEGIN UPDATE quick_note_tags SET name = 'y'; END"
    );
    assert!(violation(TriggerPolicy::Gated, &gated_or).is_some());
    let gated = format!(
        "CREATE TRIGGER t AFTER INSERT ON quick_notes FOR EACH ROW
         WHEN NEW.title <> 'x' AND {NOT_APPLYING}
         BEGIN UPDATE quick_note_tags SET name = 'y'; END"
    );
    assert_eq!(violation(TriggerPolicy::Gated, &gated), None);
    assert!(
        violation(
            TriggerPolicy::Gated,
            "CREATE TRIGGER t AFTER INSERT ON quick_notes BEGIN SELECT 1; END"
        )
        .is_some()
    );

    let writes_replicated = "CREATE TRIGGER t AFTER UPDATE ON quick_notes BEGIN
        UPDATE quick_notes SET revision = revision + 1, title = 'x' WHERE id = NEW.id; END";
    assert!(violation(TriggerPolicy::SameRow, writes_replicated).is_some());
    let other_rows = "CREATE TRIGGER t AFTER UPDATE ON quick_notes BEGIN
        UPDATE quick_notes SET revision = revision + 1; END";
    assert!(violation(TriggerPolicy::SameRow, other_rows).is_some());
    let other_table = "CREATE TRIGGER t AFTER UPDATE ON quick_notes BEGIN
        UPDATE quick_note_tags SET name = 'x' WHERE id = NEW.tag_id; END";
    assert!(violation(TriggerPolicy::SameRow, other_table).is_some());
    let same_row = "CREATE TRIGGER t AFTER UPDATE OF title ON main.quick_notes BEGIN
        UPDATE OR IGNORE quick_notes SET revision = revision + 1, body_plain_text = lower(NEW.title)
        WHERE id = NEW.id; END";
    assert_eq!(violation(TriggerPolicy::SameRow, same_row), None);

    let aborting = "CREATE TRIGGER t BEFORE INSERT ON quick_notes WHEN NEW.title = ''
        BEGIN SELECT RAISE(ABORT, 'delete from quick_notes'); END";
    assert_eq!(violation(TriggerPolicy::ApplierVisible, aborting), None);
    let writing = "CREATE TRIGGER t BEFORE INSERT ON quick_notes
        BEGIN INSERT OR REPLACE INTO quick_note_tags (id) VALUES ('x'); SELECT RAISE(ABORT, 'no'); END";
    assert!(violation(TriggerPolicy::ApplierVisible, writing).is_some());

    let indexes_replicated = "CREATE TRIGGER t AFTER INSERT ON quick_notes BEGIN
        INSERT INTO quick_notes_search_fts (note_id) VALUES (NEW.id);
        DELETE FROM quick_note_tags WHERE id = NEW.tag_id; END";
    assert_eq!(
        violation(TriggerPolicy::DerivedIndex, indexes_replicated),
        Some("writes quick_note_tags, which is not derived".to_string())
    );
    let replaces = "CREATE TRIGGER t AFTER INSERT ON quick_notes BEGIN
        REPLACE INTO quick_notes_search_fts (note_id, title) VALUES (NEW.id, replace(NEW.title, 'a', 'b')); END";
    assert_eq!(violation(TriggerPolicy::DerivedIndex, replaces), None);
}

#[test]
fn the_write_scanner_ignores_literals_and_upserts() {
    let shape = shape(
        "CREATE TRIGGER t AFTER INSERT ON quick_notes BEGIN
         INSERT INTO quick_notes_search_fts (quick_notes_search_fts, rowid) VALUES ('delete', 1);
         INSERT INTO quick_note_tags (id) VALUES (NEW.id)
             ON CONFLICT (id) DO UPDATE SET name = 'update quick_notes';
         END",
    )
    .unwrap();
    let tables = writes(&shape.body)
        .into_iter()
        .map(|write| (write.kind, write.table))
        .collect::<Vec<_>>();
    assert_eq!(
        tables,
        [
            (WriteKind::Insert, "quick_notes_search_fts".to_string()),
            (WriteKind::Insert, "quick_note_tags".to_string()),
        ]
    );
}
