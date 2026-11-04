//! Members of an apply group: a removal cutoff or exclusion inside a group keeps the part
//! before it, a member that cannot apply rolls the whole group back, a member waiting on
//! another writer holds the group until that writer's operation applies, and a fork re-seal
//! keeps the group of every operation it replays.

use ganbaru_sync_contracts::{ChangeAction, Content, Envelope, RowKey, VersionVector};

use super::block_on;
use super::exclusion::{deliver, edit};
use super::groups::{
    continues, continuing_ops, insert_note, move_and_nest_sql, nested, new_ops, notes_of, replicas,
    reseal_chain, shared_tasks, take, task_parents,
};
use super::replica::{Replica, converge, quoted};
use crate::StoreOutcome;

async fn has_note(replica: &mut Replica, writer: usize, index: usize) -> bool {
    replica
        .count(&format!(
            "SELECT count(*) FROM quick_notes WHERE id = {}",
            quoted(&format!("note-{writer}-{index:04}"))
        ))
        .await
        == 1
}

/// The operation `replica` sealed last.
async fn last_own(replica: &mut Replica) -> Vec<u8> {
    replica
        .own_ops()
        .await
        .pop()
        .expect("the replica has sealed")
}

/// Seals one operation per edit on `replica` and returns them with the operation before them.
async fn seal_each(replica: &mut Replica, edits: &[String]) -> (Vec<u8>, Vec<Vec<u8>>) {
    let previous = last_own(replica).await;
    let known = replica.stored_vector().await;
    for sql in edits {
        replica.exec(sql).await;
        assert_eq!(replica.seal().await.unwrap().sealed_ops, 1);
    }
    let ops = new_ops(replica, &known).await;
    assert_eq!(ops.len(), edits.len());
    (previous, ops)
}

#[test]
fn a_removal_cutoff_inside_a_group_applies_the_part_before_it() {
    block_on(async {
        let mut anchor = Replica::anchor(0, 301).await;
        let mut removed = Replica::new(1, 301).await;
        let mut honest = Replica::new(2, 301).await;
        edit(&mut removed, &insert_note(1, 9_999)).await;
        deliver(&mut anchor, &mut removed).await;
        deliver(&mut honest, &mut anchor).await;
        let forged = continuing_ops(&mut removed, 3).await;

        // The anchor stores only the first operation of the group before the removal, so the
        // cutoff falls inside the group, and the group waits for the rest until then.
        assert_eq!(take(&mut anchor, &forged[..1]).await, 0);
        let device = removed.device_id.clone();
        anchor.remove(&device).await.unwrap();
        anchor.apply_all().await.unwrap();
        assert!(
            has_note(&mut anchor, 1, 0).await,
            "the part before the cutoff applies without the rest"
        );

        // The rest arrives later and the selected state excludes it.
        take(&mut anchor, &forged[1..]).await;
        assert_eq!(notes_of(&mut anchor, 1).await, 2);

        // A member that learns the removal with every operation stored applies the same part.
        deliver(&mut honest, &mut anchor).await;
        assert!(has_note(&mut honest, 1, 0).await);
        assert_eq!(notes_of(&mut honest, 1).await, 2);
        for replica in [&mut anchor, &mut honest] {
            replica.check_invariants().await;
        }
    });
}

#[test]
fn a_member_that_cannot_apply_rolls_its_group_back_and_holds_its_writer() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0].exec(&insert_note(0, 9_999)).await;
        converge(&mut replicas).await;
        let edits = [
            insert_note(0, 0),
            "UPDATE quick_notes SET title = 'Edited' WHERE id = 'note-0-9999';".to_owned(),
        ];
        let (previous, ops) = seal_each(&mut replicas[0], &edits).await;
        // The second member writes a row nobody created, which only apply finds out.
        let forged = reseal_chain(
            &replicas[0],
            &previous,
            &ops,
            &[true, false],
            |index, op| {
                if index == 1
                    && let Content::Changes(changes) = &mut op.content
                {
                    assert_eq!(changes[0].action, ChangeAction::Write);
                    changes[0].row = RowKey::new("note-0-missing").unwrap();
                }
            },
        );

        let before = replicas[1].domain_dump().await;
        assert!(
            replicas[1]
                .receive(&forged)
                .await
                .iter()
                .all(|outcome| matches!(outcome, StoreOutcome::Stored { held: None }))
        );
        let report = replicas[1].apply_once().await.unwrap();
        assert_eq!(report.applied, 0);
        assert_eq!(report.held, 1);
        assert_eq!(
            replicas[1].domain_dump().await,
            before,
            "nothing of the group stays"
        );
        assert!(!has_note(&mut replicas[1], 0, 0).await);
        let held = replicas[1]
            .count("SELECT count(*) FROM sync_ops WHERE state = 'held'")
            .await;
        assert_eq!(held, 1);
        replicas[1].check_invariants().await;
    });
}

#[test]
fn a_member_over_pending_captures_rolls_its_group_back_until_they_seal() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0].exec(&insert_note(0, 9_999)).await;
        converge(&mut replicas).await;
        let edits = [
            insert_note(0, 0),
            "UPDATE quick_notes SET title = 'Remote' WHERE id = 'note-0-9999';".to_owned(),
        ];
        let (previous, ops) = seal_each(&mut replicas[0], &edits).await;
        let forged = reseal_chain(&replicas[0], &previous, &ops, &[true, false], |_, _| {});
        // The receiver edits the row only the second member touches and has not sealed it.
        replicas[1]
            .exec("UPDATE quick_notes SET title = 'Local' WHERE id = 'note-0-9999';")
            .await;
        replicas[1].receive(&forged).await;

        // Without a writer the capture cannot seal, so the whole group waits.
        let before = replicas[1].domain_dump().await;
        let receiver = &mut replicas[1];
        let report = receiver
            .engine
            .apply(&mut receiver.conn, &receiver.ctx, None, receiver.now_ms)
            .await
            .unwrap();
        assert!(report.pending_captures);
        assert_eq!(report.applied, 0);
        assert_eq!(replicas[1].domain_dump().await, before);
        assert!(!has_note(&mut replicas[1], 0, 0).await);

        // With its writer the receiver seals the capture first and the group applies whole.
        replicas[1].apply_all().await.unwrap();
        assert!(has_note(&mut replicas[1], 0, 0).await);
        converge(&mut replicas).await;
        for replica in &mut replicas {
            replica.check_invariants().await;
        }
        assert_eq!(
            replicas[0].domain_dump().await,
            replicas[1].domain_dump().await
        );
    });
}

#[test]
fn a_member_waiting_on_another_writer_holds_its_group_until_that_operation_applies() {
    block_on(async {
        let mut replicas = replicas(3).await;
        for (index, replica) in replicas.iter_mut().take(2).enumerate() {
            replica.exec(&insert_note(index, 9_999)).await;
        }
        converge(&mut replicas).await;
        let previous = last_own(&mut replicas[0]).await;
        let known = replicas[0].stored_vector().await;
        replicas[0].exec(&insert_note(0, 0)).await;
        replicas[0].seal().await.unwrap();
        // The second member of the group follows an operation of another writer.
        let other_known = replicas[1].stored_vector().await;
        replicas[1].exec(&insert_note(1, 0)).await;
        replicas[1].seal().await.unwrap();
        let other = new_ops(&mut replicas[1], &other_known).await;
        assert_eq!(take(&mut replicas[0], &other).await, other.len());
        replicas[0]
            .exec("UPDATE quick_notes SET title = 'After' WHERE id = 'note-1-0000';")
            .await;
        replicas[0].seal().await.unwrap();
        let ops = new_ops(&mut replicas[0], &known).await;
        assert_eq!(ops.len(), 2);
        let forged = reseal_chain(&replicas[0], &previous, &ops, &[true, false], |_, _| {});

        assert_eq!(take(&mut replicas[2], &forged).await, 0);
        assert!(!has_note(&mut replicas[2], 0, 0).await);
        assert_eq!(
            take(&mut replicas[2], &other).await,
            other.len() + forged.len(),
            "the group applies once the operation it waits on applies"
        );
        assert!(has_note(&mut replicas[2], 0, 0).await);
        replicas[2].check_invariants().await;
    });
}

#[test]
fn a_fork_reseal_keeps_the_group_of_every_operation_it_replays() {
    block_on(async {
        let mut a = shared_tasks(1).await.pop().unwrap();
        let mut clone = a.clone_installation(3).await;
        let mut hub = Replica::new(2, 112).await;
        let old = clone.writer();
        let keep_through = clone.stored_vector().await.get(&old);
        edit(
            &mut a,
            "UPDATE project_tasks SET title = 'Kept' WHERE id = 'task-p';",
        )
        .await;
        clone.exec(&move_and_nest_sql()).await;
        clone.seal().await.unwrap();
        let mut known = VersionVector::new();
        known.advance(old, keep_through);
        let replayed = new_ops(&mut clone, &known).await;
        let flags: Vec<bool> = replayed.iter().map(|op| continues(op)).collect();
        assert!(
            flags.len() >= 2 && flags[0],
            "the edit should seal as one group over several operations: {flags:?}"
        );

        hub.pull(&mut a).await;
        clone.rotate();
        let report = clone
            .engine
            .reseal_fork(
                &mut clone.conn,
                &clone.ctx,
                old,
                keep_through,
                &mut clone.identity.local(),
                clone.now_ms,
            )
            .await
            .unwrap();
        assert_eq!(report.replaced_ops, replayed.len());
        let replacements: Vec<bool> = clone
            .own_ops()
            .await
            .iter()
            .filter(|op| {
                let operation = Envelope::decode(op).unwrap().operation().unwrap();
                !matches!(operation.content, Content::Genesis(_))
            })
            .take(replayed.len())
            .map(|op| continues(op))
            .collect();
        assert_eq!(replacements, flags);

        hub.pull(&mut clone).await;
        assert_eq!(task_parents(&mut hub).await, nested());
        hub.check_invariants().await;
    });
}
