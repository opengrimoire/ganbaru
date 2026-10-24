//! Apply groups across replicas: changes that only hold together and fill several operations
//! apply in one transaction once every operation of their group is stored, across store passes
//! and restarts, honest changes past the group bounds apply as a chain of bounded groups with no
//! repair sealed between them, and a writer that marks every operation as continuing cannot
//! hold back other writers, grow a group past its bounds, or defer repairs past the chain bound.

use std::time::Instant;

use ganbaru_sync_contracts::bounds::{
    MAX_GROUP_BYTES, MAX_OPERATIONS_PER_CHAIN, MAX_OPERATIONS_PER_GROUP,
};
use ganbaru_sync_contracts::{Content, Envelope, Operation, WriterId, ends_group};

use super::block_on;
use super::calendar::{calendar_sql, event_sql};
use super::projects::{project_sql, task_sql};
use super::replica::{Replica, connection_from, converge, quoted, rank};
use crate::domains::music::MAX_DESCRIPTION_CHARS;

const CHILDREN: usize = 10;
const PARENT: &str = "task-p";
const NOW_MS: i64 = 1_800_000_000_000;

pub(super) async fn replicas(count: usize) -> Vec<Replica> {
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
pub(super) async fn shared_tasks(count: usize) -> Vec<Replica> {
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
pub(super) fn move_and_nest_sql() -> String {
    format!(
        "UPDATE project_tasks
         SET parent_task_id = {parent}, description = id || hex(zeroblob(30000))
         WHERE id LIKE 'task-c%';",
        parent = quoted(PARENT),
    )
}

/// Every task as `id>parent`, with `-` for a root task.
pub(super) async fn task_parents(replica: &mut Replica) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT id || '>' || coalesce(parent_task_id, '-') FROM project_tasks ORDER BY id",
    )
    .fetch_all(&mut replica.conn)
    .await
    .unwrap()
}

pub(super) fn nested() -> Vec<String> {
    let mut parents: Vec<String> = (0..CHILDREN)
        .map(|index| format!("{}>{PARENT}", child(index)))
        .collect();
    parents.push(format!("{PARENT}>-"));
    parents.sort();
    parents
}

pub(super) async fn assert_converged(replicas: &mut [Replica]) {
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
pub(super) async fn new_ops(
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

pub(super) fn continues(bytes: &[u8]) -> bool {
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

pub(super) async fn applied_seq(replica: &mut Replica, writer: &WriterId) -> u64 {
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
    insert_keyed_item(&item_id(index), &identity(index))
}

fn item_id(index: usize) -> String {
    format!("music-item-{index:05}")
}

fn identity(index: usize) -> String {
    format!("local:{index:05}")
}

fn insert_keyed_item(id: &str, identity: &str) -> String {
    format!(
        "INSERT INTO music_library_items
            (id, identity_key, source_kind, media_kind, original_title, discovered_at_ms, updated_at_ms)
         VALUES ({id}, {identity}, 'local-file', 'audio', 'Track', {NOW_MS}, {NOW_MS});",
        identity = quoted(identity),
        id = quoted(id),
    )
}

/// Characters of each metadata field a refresh rewrites, the longest allowed.
const METADATA_CHARS: usize = MAX_DESCRIPTION_CHARS;

/// A local media refresh that rewrites the metadata of every item in one transaction. Updates
/// of a table with a unique rule only hold together, so they seal as one group.
fn refresh_sql() -> String {
    refresh_fields_sql(
        &["original_title", "original_artist", "original_album"],
        METADATA_CHARS,
    )
}

/// A refresh that rewrites `columns` of every item with values of `chars` characters.
fn refresh_fields_sql(columns: &[&str], chars: usize) -> String {
    let field = format!("substr(id || hex(zeroblob({chars})), 1, {chars})");
    let set: String = columns
        .iter()
        .map(|column| format!("{column} = {field}, "))
        .collect();
    format!(
        "UPDATE music_library_items SET {set} metadata_resolved = 1, updated_at_ms = {};",
        NOW_MS + 1,
    )
}

/// Events of the calendar test, each with a description long enough that two fill an operation.
const EVENTS: usize = 8;
/// Characters of each rewritten event description.
const EVENT_DESCRIPTION_CHARS: usize = 200_000;

fn event_id(index: usize) -> String {
    format!("event-{index}")
}

fn event_uid(index: usize) -> String {
    format!("uid-{index}")
}

/// One edit that moves the import uid of the last event to the first and rewrites every
/// description. The first event seals in the first group and the last event in a later one,
/// so between them a receiver holds two events with one uid, which it hides as duplicates.
fn move_uid_sql() -> String {
    let last = EVENTS - 1;
    format!(
        "UPDATE calendar_events SET source_uid = {moved} WHERE id = {last_id};
         UPDATE calendar_events SET source_uid = {taken} WHERE id = {first_id};
         UPDATE calendar_events
         SET description = substr(id || hex(zeroblob({chars})), 1, {chars}),
             updated_at = '2027-01-02T00:00:00.000Z';",
        moved = quoted("uid-moved"),
        taken = quoted(&event_uid(last)),
        last_id = quoted(&event_id(last)),
        first_id = quoted(&event_id(0)),
        chars = EVENT_DESCRIPTION_CHARS,
    )
}

async fn event_text(replica: &mut Replica, column: &str, index: usize) -> Option<String> {
    replica
        .scalar_text(&format!(
            "SELECT {column} FROM calendar_events WHERE id = {}",
            quoted(&event_id(index))
        ))
        .await
}

#[test]
fn a_unique_value_moved_across_a_group_cut_seals_no_repair() {
    block_on(async {
        let mut replicas = replicas(2).await;
        let mut setup = calendar_sql();
        for index in 0..EVENTS {
            setup.push_str(&event_sql(&event_id(index), &event_uid(index), "Event"));
        }
        replicas[0].exec(&setup).await;
        converge(&mut replicas).await;
        let known = replicas[1].stored_vector().await;
        replicas[0].exec(&move_uid_sql()).await;
        replicas[0].seal().await.unwrap();
        let ops = new_ops(&mut replicas[0], &known).await;
        let sizes = group_sizes(&ops);
        assert!(sizes.len() >= 2, "the edit should span groups: {sizes:?}");

        let own = replicas[1].own_ops().await.len();
        replicas[1].receive(&ops).await;
        let report = replicas[1].apply_once().await.unwrap();
        assert_eq!(report.applied, sizes[0]);
        // The cut separates the two events: the first group gave the first event the last
        // one's uid, so the last event is hidden as its duplicate until a later group.
        let length = "CAST(length(description) AS TEXT)";
        let rewritten = event_text(&mut replicas[1], length, 0).await;
        assert_eq!(rewritten, Some(EVENT_DESCRIPTION_CHARS.to_string()));
        assert_eq!(event_text(&mut replicas[1], "id", EVENTS - 1).await, None);
        assert_eq!(report.seal.map_or(0, |seal| seal.sealed_ops), 0);
        assert_eq!(
            replicas[1].own_ops().await.len(),
            own,
            "no repair is sealed between groups of one chain"
        );

        replicas[1].apply_all().await.unwrap();
        assert_eq!(replicas[1].own_ops().await.len(), own);
        assert_eq!(
            event_text(&mut replicas[1], "source_uid", 0).await,
            Some(event_uid(EVENTS - 1))
        );
        assert_eq!(
            event_text(&mut replicas[1], "source_uid", EVENTS - 1)
                .await
                .as_deref(),
            Some("uid-moved")
        );
        let events = replicas[1]
            .count("SELECT count(*) FROM calendar_events")
            .await;
        assert_eq!(events, i64::try_from(EVENTS).unwrap());
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn an_honest_chain_past_the_group_bounds_seals_no_repair_between_its_groups() {
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
        // The sender kept the chain open; the byte bound ended the first group.
        assert!(ops[..ops.len() - 1].iter().all(|op| continues(op)));
        let first_bytes: usize = ops[..sizes[0]].iter().map(Vec::len).sum();
        assert!(first_bytes >= MAX_GROUP_BYTES);

        let own = replicas[1].own_ops().await.len();
        replicas[1].receive(&ops).await;
        let report = replicas[1].apply_once().await.unwrap();
        assert_eq!(report.applied, sizes[0]);
        assert!(report.more);
        assert_eq!(report.seal.map_or(0, |seal| seal.sealed_ops), 0);
        assert_eq!(
            replicas[1].own_ops().await.len(),
            own,
            "no repair is sealed between groups of one chain"
        );
        replicas[1].apply_all().await.unwrap();
        assert_eq!(replicas[1].own_ops().await.len(), own);
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let refreshed = replicas[1]
            .count("SELECT count(*) FROM music_library_items WHERE metadata_resolved = 1")
            .await;
        assert_eq!(refreshed, i64::try_from(items).unwrap());
    });
}

/// Stores `ops` on `to` and applies everything ready, returning how many operations applied.
pub(super) async fn take(to: &mut Replica, ops: &[Vec<u8>]) -> usize {
    to.receive(ops).await;
    let reports = to.apply_all().await.unwrap();
    reports.iter().map(|report| report.applied).sum()
}

/// Inserts an item on each of two replicas under one identity key and delivers the second's to
/// the third, which hides one of them as a duplicate. Returns the operations the third sealed.
async fn duplicate_on(replicas: &mut [Replica], key: &str) -> usize {
    replicas[2]
        .exec(&insert_keyed_item(&format!("music-dup-a-{key}"), key))
        .await;
    replicas[2].seal().await.unwrap();
    let known = replicas[2].stored_vector().await;
    replicas[1]
        .exec(&insert_keyed_item(&format!("music-dup-b-{key}"), key))
        .await;
    replicas[1].seal().await.unwrap();
    let ops = new_ops(&mut replicas[1], &known).await;
    let own = replicas[2].own_ops().await.len();
    replicas[2].receive(&ops).await;
    replicas[2].apply_all().await.unwrap();
    let hidden = replicas[2]
        .count(&format!(
            "SELECT count(*) FROM music_library_items WHERE identity_key = {}",
            quoted(key)
        ))
        .await;
    assert_eq!(hidden, 1, "one of the duplicates is hidden");
    replicas[2].own_ops().await.len() - own
}

#[test]
fn a_writer_marking_every_operation_continuing_defers_repairs_for_the_chain_bound_only() {
    block_on(async {
        let mut replicas = replicas(3).await;
        for (index, replica) in replicas.iter_mut().enumerate() {
            replica.exec(&insert_note(index, 9_999)).await;
        }
        converge(&mut replicas).await;
        // Every group of the forged run ends at the operation bound, so all of it applies
        // without a closing operation.
        assert_eq!(MAX_OPERATIONS_PER_CHAIN % MAX_OPERATIONS_PER_GROUP, 0);
        let forged = continuing_ops(&mut replicas[0], MAX_OPERATIONS_PER_CHAIN).await;
        let (open, rest) = forged.split_at(MAX_OPERATIONS_PER_GROUP);
        assert_eq!(take(&mut replicas[2], open).await, open.len());

        assert_eq!(
            duplicate_on(&mut replicas, "local:dup-1").await,
            0,
            "an open chain defers the repair"
        );

        assert_eq!(take(&mut replicas[2], rest).await, rest.len());
        assert!(
            duplicate_on(&mut replicas, "local:dup-2").await > 0,
            "past the chain bound the repair is sealed"
        );
        replicas[2].check_invariants().await;
    });
}

pub(super) fn insert_note(writer: usize, index: usize) -> String {
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
    let ops = new_ops(replica, &known).await;
    assert_eq!(ops.len(), count);
    reseal_chain(replica, &previous, &ops, &vec![true; count], |_, _| {})
}

/// Re-seals `ops`, which `replica` sealed right after `previous`, as one chain whose operations
/// mark their groups with `flags`, after `forge` changes each, as a writer may sign anything.
pub(super) fn reseal_chain(
    replica: &Replica,
    previous: &[u8],
    ops: &[Vec<u8>],
    flags: &[bool],
    mut forge: impl FnMut(usize, &mut Operation),
) -> Vec<Vec<u8>> {
    assert_eq!(ops.len(), flags.len());
    let mut previous_hash = Envelope::decode(previous).unwrap().hash();
    let mut forged = Vec::with_capacity(ops.len());
    for (index, (bytes, flag)) in ops.iter().zip(flags).enumerate() {
        let mut operation = Envelope::decode(bytes).unwrap().operation().unwrap();
        assert!(matches!(operation.content, Content::Changes(_)));
        operation.header.group_continues = *flag;
        operation.previous_hash = previous_hash;
        forge(index, &mut operation);
        let sealed = operation.seal(&replica.identity.key).unwrap();
        previous_hash = sealed.hash();
        forged.push(sealed.into_bytes());
    }
    forged
}

pub(super) async fn notes_of(replica: &mut Replica, writer: usize) -> i64 {
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

/// Prints the operations, bytes, and receiver groups of one writer's chain from `edit`, and the
/// time of every apply call that takes it in.
async fn measure_chain(label: &str, author: &mut Replica, reader: &mut Replica, edit: &str) {
    let known = reader.stored_vector().await;
    author.exec(edit).await;
    let started = Instant::now();
    let mut seals = 0;
    loop {
        seals += 1;
        if !author.seal().await.unwrap().more {
            break;
        }
    }
    let sealing = started.elapsed();
    let ops = new_ops(author, &known).await;
    let bytes: usize = ops.iter().map(Vec::len).sum();
    let largest = ops.iter().map(Vec::len).max().unwrap_or(0);
    let most_changes = ops
        .iter()
        .map(
            |op| match Envelope::decode(op).unwrap().operation().unwrap().content {
                Content::Changes(changes) => changes.len(),
                _ => 0,
            },
        )
        .max()
        .unwrap_or(0);
    let chain = ops.iter().take_while(|op| continues(op)).count() + 1;
    println!(
        "{label}: {} operations, {bytes} bytes, largest {largest}, most changes {most_changes}, \
         sealed in {seals} calls over {sealing:?}, continuing chain {chain}, groups {:?}",
        ops.len(),
        group_sizes(&ops),
    );
    measure_apply(reader, &ops).await;
}

/// Stores `ops` on `reader` and prints what each apply call takes until nothing is left.
async fn measure_apply(reader: &mut Replica, ops: &[Vec<u8>]) {
    reader.receive(ops).await;
    loop {
        let started = Instant::now();
        let report = reader.apply_once().await.unwrap();
        println!(
            "  apply: {} operations, {} changes, {} sealed in {:?}",
            report.applied,
            report.changes,
            report.seal.as_ref().map_or(0, |seal| seal.sealed_ops),
            started.elapsed()
        );
        if !report.more {
            break;
        }
    }
}

/// The largest group a writer can force: the smallest changes a table with a unique rule seals
/// at the wire limits, over edits whose operations are re-sealed as one chain, so only the
/// group bounds end it. Prints the group and what applying it takes.
async fn measure_hostile_group(author: &mut Replica, reader: &mut Replica) {
    let known = reader.stored_vector().await;
    let previous = author.own_ops().await.pop().expect("the author has sealed");
    let mut ops = Vec::new();
    while ops.iter().map(Vec::len).sum::<usize>() < 2 * MAX_GROUP_BYTES
        && ops.len() <= MAX_OPERATIONS_PER_GROUP
    {
        author
            .exec("UPDATE music_library_items SET updated_at_ms = updated_at_ms + 1;")
            .await;
        while author.seal().await.unwrap().more {}
        ops = new_ops(author, &known).await;
    }
    let forged = reseal_chain(author, &previous, &ops, &vec![true; ops.len()], |_, _| {});
    println!(
        "hostile group: {} operations, {} bytes, groups {:?}",
        forged.len(),
        forged.iter().map(Vec::len).sum::<usize>(),
        group_sizes(&forged),
    );
    measure_apply(reader, &forged).await;
}

const SHORT_FIELDS: [&str; 3] = ["original_title", "original_artist", "original_album"];

/// Measures the chains of a dense Music library refresh and of operations near the wire bounds:
/// `cargo test -p ganbaru-sync --lib --release chain_sizes -- --ignored --nocapture`.
#[test]
#[ignore = "measurements are meaningful only in the release profile"]
fn chain_sizes_are_measured() {
    block_on(async {
        // The dense Music fixture: 6,000 items with short titles, artists, and albums.
        let mut dense_pair = replicas(2).await;
        let (author, reader) = dense_pair.split_at_mut(1);
        let discovery: String = (0..6_000).map(insert_item).collect();
        measure_chain(
            "dense discovery",
            &mut author[0],
            &mut reader[0],
            &discovery,
        )
        .await;
        let refresh = refresh_fields_sql(&SHORT_FIELDS, 18);
        measure_chain("dense refresh", &mut author[0], &mut reader[0], &refresh).await;
        let longest = refresh_fields_sql(&SHORT_FIELDS, METADATA_CHARS);
        measure_chain("longest refresh", &mut author[0], &mut reader[0], &longest).await;
        // Last on this pair: the reader then holds forged operations the author never stored.
        measure_hostile_group(&mut author[0], &mut reader[0]).await;

        // One field near 440 characters per change fills operations near both wire bounds.
        let mut wide_pair = replicas(2).await;
        let (author, reader) = wide_pair.split_at_mut(1);
        let discovery: String = (0..4_096).map(insert_item).collect();
        measure_chain("wide discovery", &mut author[0], &mut reader[0], &discovery).await;
        let wide = refresh_fields_sql(&["original_title"], 440);
        measure_chain("wide refresh", &mut author[0], &mut reader[0], &wide).await;
    });
}
