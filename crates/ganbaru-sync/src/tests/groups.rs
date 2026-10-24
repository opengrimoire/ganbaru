//! Apply groups across replicas: changes that only hold together and fill several operations
//! apply in one transaction once every operation of their group is stored, across store passes
//! and restarts, honest groups past the group bounds split where every receiver splits them, and
//! a writer that marks every operation as continuing cannot hold back other writers or grow a
//! group past its bounds.

use ganbaru_sync_contracts::bounds::{MAX_GROUP_BYTES, MAX_OPERATIONS_PER_GROUP};
use ganbaru_sync_contracts::{Content, Envelope, WriterId, ends_group};

use super::block_on;
use super::projects::{project_sql, task_sql};
use super::replica::{Replica, connection_from, converge, quoted, rank};
use crate::domains::music::MAX_DESCRIPTION_CHARS;

const CHILDREN: usize = 10;
const PARENT: &str = "task-p";
const NOW_MS: i64 = 1_800_000_000_000;

async fn replicas(count: usize) -> Vec<Replica> {
    let mut replicas = Vec::with_capacity(count);
    for index in 0..count {
        replicas.push(Replica::new(index, 112).await);
    }
    replicas
}

fn child(index: usize) -> String {
    format!("task-c{index:02}")
}

/// The shared project with a parent task and [`CHILDREN`] root tasks, on every replica.
async fn shared_tasks(count: usize) -> Vec<Replica> {
    let mut sql = project_sql();
    sql.push_str(&task_sql(PARENT, "Parent", 1));
    for index in 0..CHILDREN {
        let position = u32::try_from(index + 2).unwrap();
        sql.push_str(&task_sql(&child(index), "Child", position));
    }
    let mut replicas = replicas(count).await;
    replicas[0].exec(&sql).await;
    converge(&mut replicas).await;
    replicas
}

/// One edit that nests every child under the parent and gives each a description near the
/// longest allowed, so its changes only hold together and fill more than one operation.
fn move_and_nest_sql() -> String {
    format!(
        "UPDATE project_tasks
         SET parent_task_id = {parent}, description = id || hex(zeroblob(30000))
         WHERE id LIKE 'task-c%';",
        parent = quoted(PARENT),
    )
}

/// Every task as `id>parent`, with `-` for a root task.
async fn task_parents(replica: &mut Replica) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT id || '>' || coalesce(parent_task_id, '-') FROM project_tasks ORDER BY id",
    )
    .fetch_all(&mut replica.conn)
    .await
    .unwrap()
}

fn nested() -> Vec<String> {
    let mut parents: Vec<String> = (0..CHILDREN)
        .map(|index| format!("{}>{PARENT}", child(index)))
        .collect();
    parents.push(format!("{PARENT}>-"));
    parents.sort();
    parents
}

async fn assert_converged(replicas: &mut [Replica]) {
    let first = replicas[0].domain_dump().await;
    for replica in replicas.iter_mut().skip(1) {
        assert_eq!(
            replica.domain_dump().await,
            first,
            "replica {} diverged",
            replica.index
        );
    }
}

/// Operations of `writer` that `replica` stores and `known` lacks.
async fn new_ops(
    replica: &mut Replica,
    known: &ganbaru_sync_contracts::VersionVector,
) -> Vec<Vec<u8>> {
    let writer = replica.writer();
    replica
        .ops_since(known)
        .await
        .into_iter()
        .filter(|bytes| Envelope::decode(bytes).unwrap().writer == writer)
        .collect()
}

fn continues(bytes: &[u8]) -> bool {
    Envelope::decode(bytes)
        .unwrap()
        .operation()
        .unwrap()
        .header
        .group_continues
}

/// Sizes of the apply groups a receiver forms from one writer's consecutive operations.
fn group_sizes(ops: &[Vec<u8>]) -> Vec<usize> {
    let mut sizes = Vec::new();
    let (mut operations, mut bytes) = (0, 0);
    for op in ops {
        operations += 1;
        bytes += op.len();
        if ends_group(continues(op), operations, bytes) {
            sizes.push(operations);
            (operations, bytes) = (0, 0);
        }
    }
    if operations > 0 {
        sizes.push(operations);
    }
    sizes
}

async fn applied_seq(replica: &mut Replica, writer: &WriterId) -> u64 {
    replica.applied_vector().await.get(writer)
}

#[test]
fn projects_move_and_nest_split_by_bytes_applies_in_one_transaction() {
    block_on(async {
        let mut replicas = shared_tasks(2).await;
        let known = replicas[1].stored_vector().await;
        replicas[0].exec(&move_and_nest_sql()).await;
        let sealed = replicas[0].seal().await.unwrap();
        assert!(!sealed.more, "the edit should seal in one call");
        let ops = new_ops(&mut replicas[0], &known).await;
        assert!(ops.len() >= 2, "the edit should fill several operations");
        let flags: Vec<bool> = ops.iter().map(|op| continues(op)).collect();
        assert!(flags[..ops.len() - 1].iter().all(|flag| *flag), "{flags:?}");
        assert!(
            !flags[ops.len() - 1],
            "the last operation should close the group"
        );
        assert_eq!(group_sizes(&ops), vec![ops.len()]);

        replicas[1].receive(&ops).await;
        let report = replicas[1].apply_once().await.unwrap();
        assert_eq!(report.applied, ops.len());
        assert!(
            report.seal.as_ref().is_none_or(|seal| seal.sealed_ops == 0),
            "a whole group should need no repair: {:?}",
            report.seal
        );
        assert_eq!(task_parents(&mut replicas[1]).await, nested());
        assert_eq!(replicas[1].seal().await.unwrap().sealed_ops, 0);
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn a_group_stored_over_two_passes_with_a_restart_applies_only_once_whole() {
    block_on(async {
        let mut replicas = shared_tasks(2).await;
        let writer = replicas[0].writer();
        let known = replicas[1].stored_vector().await;
        replicas[0].exec(&move_and_nest_sql()).await;
        replicas[0].seal().await.unwrap();
        let ops = new_ops(&mut replicas[0], &known).await;
        assert!(ops.len() >= 2, "the edit should fill several operations");
        let (head, tail) = ops.split_at(ops.len() - 1);

        let before = applied_seq(&mut replicas[1], &writer).await;
        let parents = task_parents(&mut replicas[1]).await;
        replicas[1].receive(head).await;
        let reports = replicas[1].apply_all().await.unwrap();
        assert!(
            reports.iter().all(|report| report.applied == 0),
            "{reports:?}"
        );
        assert_eq!(applied_seq(&mut replicas[1], &writer).await, before);
        assert_eq!(task_parents(&mut replicas[1]).await, parents);

        let snapshot = replicas[1].snapshot().await;
        replicas[1].conn = connection_from(&snapshot).await;
        replicas[1].receive(tail).await;
        let report = replicas[1].apply_once().await.unwrap();
        assert_eq!(report.applied, ops.len());
        assert_eq!(
            applied_seq(&mut replicas[1], &writer).await,
            before + u64::try_from(ops.len()).unwrap()
        );
        assert_eq!(task_parents(&mut replicas[1]).await, nested());
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
    });
}

fn insert_item(index: usize) -> String {
    let id = format!("music-item-{index:05}");
    format!(
        "INSERT INTO music_library_items
            (id, identity_key, source_kind, media_kind, original_title, discovered_at_ms, updated_at_ms)
         VALUES ({id}, {identity}, 'local-file', 'audio', 'Track', {NOW_MS}, {NOW_MS});",
        identity = quoted(&format!("local:{index:05}")),
        id = quoted(&id),
    )
}

/// Characters of each metadata field a refresh rewrites, the longest allowed.
const METADATA_CHARS: usize = MAX_DESCRIPTION_CHARS;

/// A local media refresh that rewrites the metadata of every item in one transaction. Updates
/// of a table with a unique rule only hold together, so they seal as one group.
fn refresh_sql() -> String {
    let field = format!("substr(id || hex(zeroblob({METADATA_CHARS})), 1, {METADATA_CHARS})");
    format!(
        "UPDATE music_library_items
         SET original_title = {field}, original_artist = {field}, original_album = {field},
             metadata_resolved = 1, updated_at_ms = {};",
        NOW_MS + 1,
    )
}

#[test]
fn an_honest_group_past_the_byte_bound_applies_in_bounded_groups() {
    block_on(async {
        let mut replicas = replicas(2).await;
        // Enough items that their rewritten metadata spans two groups by bytes.
        let items = 2 * MAX_GROUP_BYTES / (3 * METADATA_CHARS) + 2;
        let discovery: String = (0..items).map(insert_item).collect();
        replicas[0].exec(&discovery).await;
        converge(&mut replicas).await;
        let known = replicas[1].stored_vector().await;
        replicas[0].exec(&refresh_sql()).await;
        let sealed = replicas[0].seal().await.unwrap();
        assert!(!sealed.more, "the refresh should seal in one call");
        let ops = new_ops(&mut replicas[0], &known).await;
        let sizes = group_sizes(&ops);
        assert!(
            sizes.len() >= 2,
            "the refresh should span bounded groups: {sizes:?}"
        );
        // The sender kept the group open; the byte bound ended it.
        assert!(continues(&ops[sizes[0] - 1]));
        let first_bytes: usize = ops[..sizes[0]].iter().map(Vec::len).sum();
        assert!(first_bytes >= MAX_GROUP_BYTES);

        replicas[1].receive(&ops).await;
        let report = replicas[1].apply_once().await.unwrap();
        assert_eq!(report.applied, sizes[0]);
        assert!(report.more);
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let refreshed = replicas[1]
            .count("SELECT count(*) FROM music_library_items WHERE metadata_resolved = 1")
            .await;
        assert_eq!(refreshed, i64::try_from(items).unwrap());
    });
}

fn insert_note(writer: usize, index: usize) -> String {
    format!(
        "INSERT INTO quick_notes (id, title, order_key) VALUES ({}, 'Note', {});",
        quoted(&format!("note-{writer}-{index:04}")),
        quoted(&rank(u32::try_from(writer * 10_000 + index + 1).unwrap())),
    )
}

/// `count` operations of one note each, sealed by `replica` and then re-sealed with every
/// operation marking its group as continuing, chained after the replica's earlier operations.
pub(super) async fn continuing_ops(replica: &mut Replica, count: usize) -> Vec<Vec<u8>> {
    let known = replica.stored_vector().await;
    let previous = replica
        .own_ops()
        .await
        .pop()
        .expect("the replica has sealed");
    for index in 0..count {
        replica.exec(&insert_note(replica.index, index)).await;
        assert_eq!(replica.seal().await.unwrap().sealed_ops, 1);
    }
    let mut previous_hash = Envelope::decode(&previous).unwrap().hash();
    let mut forged = Vec::with_capacity(count);
    for bytes in new_ops(replica, &known).await {
        let mut operation = Envelope::decode(&bytes).unwrap().operation().unwrap();
        assert!(matches!(operation.content, Content::Changes(_)));
        operation.header.group_continues = true;
        operation.previous_hash = previous_hash;
        let sealed = operation.seal(&replica.identity.key).unwrap();
        previous_hash = sealed.hash();
        forged.push(sealed.into_bytes());
    }
    assert_eq!(forged.len(), count);
    forged
}

async fn notes_of(replica: &mut Replica, writer: usize) -> i64 {
    replica
        .count(&format!(
            "SELECT count(*) FROM quick_notes WHERE id LIKE 'note-{writer}-%'"
        ))
        .await
}

#[test]
fn a_group_that_never_closes_holds_back_only_its_writer() {
    block_on(async {
        let mut replicas = replicas(3).await;
        for (index, replica) in replicas.iter_mut().take(2).enumerate() {
            replica.exec(&insert_note(index, 9_999)).await;
        }
        converge(&mut replicas).await;
        let hostile = replicas[0].writer();
        let boundary = applied_seq(&mut replicas[2], &hostile).await;
        let forged = continuing_ops(&mut replicas[0], 5).await;
        let known = replicas[2].stored_vector().await;
        replicas[1].exec(&insert_note(1, 0)).await;
        replicas[1].seal().await.unwrap();
        let honest = new_ops(&mut replicas[1], &known).await;

        replicas[2].receive(&forged).await;
        replicas[2].receive(&honest).await;
        let reports = replicas[2].apply_all().await.unwrap();
        assert_eq!(
            reports.iter().map(|report| report.applied).sum::<usize>(),
            honest.len()
        );
        assert_eq!(notes_of(&mut replicas[2], 1).await, 2);
        assert_eq!(notes_of(&mut replicas[2], 0).await, 1);
        assert_eq!(applied_seq(&mut replicas[2], &hostile).await, boundary);
        replicas[2].check_invariants().await;
    });
}

#[test]
fn a_writer_marking_every_operation_continuing_forms_groups_of_the_operation_bound() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0].exec(&insert_note(0, 9_999)).await;
        converge(&mut replicas).await;
        let hostile = replicas[0].writer();
        let boundary = applied_seq(&mut replicas[1], &hostile).await;
        let forged = continuing_ops(&mut replicas[0], MAX_OPERATIONS_PER_GROUP + 1).await;
        assert_eq!(
            group_sizes(&forged),
            vec![MAX_OPERATIONS_PER_GROUP, 1],
            "the operation bound should end the first group"
        );

        replicas[1].receive(&forged).await;
        let report = replicas[1].apply_once().await.unwrap();
        assert_eq!(report.applied, MAX_OPERATIONS_PER_GROUP);
        replicas[1].apply_all().await.unwrap();
        let bound = u64::try_from(MAX_OPERATIONS_PER_GROUP).unwrap();
        assert_eq!(
            applied_seq(&mut replicas[1], &hostile).await,
            boundary + bound
        );
        assert_eq!(
            notes_of(&mut replicas[1], 0).await,
            i64::try_from(MAX_OPERATIONS_PER_GROUP + 1).unwrap()
        );
        replicas[1].check_invariants().await;
    });
}
