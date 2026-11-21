//! The writer lifecycle: its derivation from replicated facts, its legal moves, the CHECK that
//! keeps the stored state and cutoffs consistent, and the guarded update that changes it.

use ganbaru_sync_contracts::{SpaceId, WriterId};
use sqlx::SqliteConnection;

use super::{block_on, migrated_pool};
use crate::log::{self, IllegalWriterMove, WriterState};

const FORK_CUTOFFS: [Option<u64>; 3] = [None, Some(0), Some(3)];
const REMOVAL_CUTOFFS: [Option<u64>; 4] = [None, Some(0), Some(2), Some(5)];

fn states() -> Vec<WriterState> {
    let mut states = vec![WriterState::Active];
    for cutoff in [0, 2, 5] {
        states.push(WriterState::Revoked { cutoff });
        states.push(WriterState::Forked { cutoff });
    }
    states
}

#[test]
fn a_fork_wins_over_a_removal_and_neither_leaves_the_writer_active() {
    for fork in FORK_CUTOFFS {
        for removal in REMOVAL_CUTOFFS {
            let expected = match (fork, removal) {
                (Some(cutoff), _) => WriterState::Forked { cutoff },
                (None, Some(cutoff)) => WriterState::Revoked { cutoff },
                (None, None) => WriterState::Active,
            };
            assert_eq!(
                WriterState::derive(fork, removal),
                expected,
                "{fork:?} {removal:?}"
            );
        }
    }
}

#[test]
fn a_forked_writer_never_leaves_forked_and_its_cutoff_only_falls() {
    for from in states() {
        for next in states() {
            let expected = match (from, next) {
                (WriterState::Forked { .. }, WriterState::Active | WriterState::Revoked { .. }) => {
                    Err(IllegalWriterMove::LeavesForked)
                }
                (WriterState::Forked { cutoff: was }, WriterState::Forked { cutoff: now })
                    if now > was =>
                {
                    Err(IllegalWriterMove::RaisesForkCutoff)
                }
                _ => Ok(next),
            };
            assert_eq!(from.transition(next), expected, "{from:?} to {next:?}");
        }
    }
}

#[test]
fn every_derived_state_is_a_legal_move_from_a_state_derived_from_fewer_facts() {
    // Removals come and go with the selected chain, while fork evidence only grows and falls.
    for removal_before in REMOVAL_CUTOFFS {
        for removal_after in REMOVAL_CUTOFFS {
            for (before, after) in [
                (None, None),
                (None, Some(3)),
                (Some(3), Some(3)),
                (Some(3), Some(0)),
            ] {
                let from = WriterState::derive(before, removal_before);
                let next = WriterState::derive(after, removal_after);
                assert_eq!(from.transition(next), Ok(next), "{from:?} to {next:?}");
            }
        }
    }
}

/// Column triples `(state, cutoff_seq, removal_cutoff_seq)` the decoder and the CHECK refuse.
const CONTRADICTIONS: [(&str, Option<u64>, Option<u64>); 8] = [
    ("active", Some(2), None),
    ("active", None, Some(2)),
    ("revoked", None, Some(2)),
    ("revoked", Some(2), None),
    ("revoked", Some(2), Some(3)),
    ("forked", None, None),
    ("forked", None, Some(2)),
    ("retired", Some(2), None),
];

#[test]
fn stored_columns_decode_only_when_state_and_cutoffs_agree() {
    for fork in FORK_CUTOFFS {
        for removal in REMOVAL_CUTOFFS {
            let state = WriterState::derive(fork, removal);
            let (name, cutoff) = state.columns().expect("columns");
            let cutoff = cutoff.map(|seq| u64::try_from(seq).expect("cutoff"));
            assert_eq!(
                WriterState::from_columns(name, cutoff, removal).expect("decode"),
                state
            );
            assert_eq!(state.cutoff(), cutoff);
        }
    }
    for (state, cutoff, removal) in CONTRADICTIONS {
        assert!(
            WriterState::from_columns(state, cutoff, removal).is_err(),
            "{state} {cutoff:?} {removal:?}"
        );
    }
}

const SPACE: [u8; 16] = [7; 16];

fn writer(seed: u8) -> WriterId {
    WriterId::from_slice(&[seed; 16]).expect("writer id")
}

async fn insert_writer(
    conn: &mut SqliteConnection,
    seed: u8,
    state: &str,
    cutoff: Option<u64>,
    removal: Option<u64>,
) -> Result<(), sqlx::Error> {
    let seq = |value: Option<u64>| value.map(|seq| i64::try_from(seq).expect("sequence"));
    sqlx::query(
        "INSERT INTO sync_writers
            (space_id, writer_id, public_key, device_id, certificate, state, cutoff_seq,
             stored_seq, applied_seq, head_hash, head_clock, created_at_ms, removal_cutoff_seq)
         VALUES (?, ?, ?, 'device-a', x'00', ?, ?, 6, 0, zeroblob(32), 0, 0, ?)",
    )
    .bind(SPACE.as_slice())
    .bind(writer(seed).as_bytes().as_slice())
    .bind([seed; 32].as_slice())
    .bind(state)
    .bind(seq(cutoff))
    .bind(seq(removal))
    .execute(&mut *conn)
    .await
    .map(|_| ())
}

async fn space(conn: &mut SqliteConnection) -> SpaceId {
    let space = SpaceId::from_slice(&SPACE).expect("space id");
    sqlx::query("INSERT INTO sync_spaces (space_id, vault_id, created_at_ms) VALUES (?, 'vault', 0)")
        .bind(SPACE.as_slice())
        .execute(&mut *conn)
        .await
        .expect("space");
    log::ensure_space(conn, space).await.expect("space exists");
    space
}

#[test]
fn the_schema_refuses_every_state_its_cutoffs_contradict() {
    block_on(async {
        let pool = migrated_pool().await;
        let mut conn = pool.acquire().await.expect("connection");
        space(&mut conn).await;
        let mut seed = 0;
        for fork in FORK_CUTOFFS {
            for removal in REMOVAL_CUTOFFS {
                seed += 1;
                let (state, cutoff) = WriterState::derive(fork, removal)
                    .columns()
                    .expect("columns");
                let cutoff = cutoff.map(|seq| u64::try_from(seq).expect("cutoff"));
                insert_writer(&mut conn, seed, state, cutoff, removal)
                    .await
                    .unwrap_or_else(|error| panic!("{state} {cutoff:?} {removal:?}: {error}"));
            }
        }
        for (state, cutoff, removal) in CONTRADICTIONS {
            seed += 1;
            assert!(
                insert_writer(&mut conn, seed, state, cutoff, removal)
                    .await
                    .is_err(),
                "{state} {cutoff:?} {removal:?}"
            );
        }
    });
}

async fn add_evidence(conn: &mut SqliteConnection, seed: u8, seq: i64) {
    sqlx::query(
        "INSERT INTO sync_fork_evidence
            (space_id, writer_id, seq, first_envelope, second_envelope, received_at_ms)
         VALUES (?, ?, ?, x'01', x'02', 0)",
    )
    .bind(SPACE.as_slice())
    .bind(writer(seed).as_bytes().as_slice())
    .bind(seq)
    .execute(&mut *conn)
    .await
    .expect("evidence");
}

async fn stored(conn: &mut SqliteConnection, space: SpaceId, seed: u8) -> log::WriterRow {
    log::writer(conn, space, writer(seed))
        .await
        .expect("read writer")
        .expect("writer")
}

#[test]
fn settling_writes_the_derived_state_with_the_removal_cutoff() {
    block_on(async {
        let pool = migrated_pool().await;
        let mut conn = pool.acquire().await.expect("connection");
        let space = space(&mut conn).await;
        insert_writer(&mut conn, 1, "active", None, None)
            .await
            .expect("writer");

        let row = stored(&mut conn, space, 1).await;
        let revoked = log::settle_state(&mut conn, space, &row, Some(2))
            .await
            .expect("remove");
        assert_eq!(revoked, WriterState::Revoked { cutoff: 2 });
        let row = stored(&mut conn, space, 1).await;
        assert_eq!((row.state, row.removal_cutoff), (revoked, Some(2)));

        // A reselected chain that no longer removes the device, or no longer names it, makes
        // the writer active again.
        let active = log::settle_state(&mut conn, space, &row, None)
            .await
            .expect("lift");
        assert_eq!(active, WriterState::Active);
        let row = stored(&mut conn, space, 1).await;
        assert_eq!((row.state, row.removal_cutoff), (WriterState::Active, None));

        // Fork evidence wins over the removal and outlives it.
        add_evidence(&mut conn, 1, 4).await;
        let forked = log::settle_state(&mut conn, space, &row, Some(2))
            .await
            .expect("fork");
        assert_eq!(forked, WriterState::Forked { cutoff: 3 });
        let row = stored(&mut conn, space, 1).await;
        let still = log::settle_state(&mut conn, space, &row, None)
            .await
            .expect("lift removal");
        assert_eq!(still, WriterState::Forked { cutoff: 3 });
        let row = stored(&mut conn, space, 1).await;
        assert_eq!((row.state, row.removal_cutoff), (still, None));
    });
}

#[test]
fn settling_from_a_stale_read_changes_nothing() {
    block_on(async {
        let pool = migrated_pool().await;
        let mut conn = pool.acquire().await.expect("connection");
        let space = space(&mut conn).await;
        insert_writer(&mut conn, 1, "active", None, None)
            .await
            .expect("writer");
        let stale = stored(&mut conn, space, 1).await;
        let current = stored(&mut conn, space, 1).await;
        log::settle_state(&mut conn, space, &current, Some(2))
            .await
            .expect("remove");

        assert!(
            log::settle_state(&mut conn, space, &stale, Some(5))
                .await
                .is_err()
        );
        let row = stored(&mut conn, space, 1).await;
        assert_eq!(
            (row.state, row.removal_cutoff),
            (WriterState::Revoked { cutoff: 2 }, Some(2))
        );
    });
}

#[test]
fn settling_refuses_to_leave_a_forked_state_fork_evidence_no_longer_backs() {
    block_on(async {
        let pool = migrated_pool().await;
        let mut conn = pool.acquire().await.expect("connection");
        let space = space(&mut conn).await;
        insert_writer(&mut conn, 1, "forked", Some(3), None)
            .await
            .expect("writer");
        let row = stored(&mut conn, space, 1).await;
        assert!(log::settle_state(&mut conn, space, &row, None).await.is_err());
        assert_eq!(
            stored(&mut conn, space, 1).await.state,
            WriterState::Forked { cutoff: 3 }
        );
    });
}
