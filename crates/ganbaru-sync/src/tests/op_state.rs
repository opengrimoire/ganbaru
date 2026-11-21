//! The stored operation lifecycle, its legal moves, and the SQL guards of its writers.

use sqlx::{AssertSqlSafe, Row};

use super::{block_on, migrated_pool};
use crate::op_state::{self, IllegalOpMove, OpMove, OpState, transition};
use crate::store::HoldReason;

const REASONS: [HoldReason; 3] = [
    HoldReason::NewerFormat,
    HoldReason::NewerManifest,
    HoldReason::Invalid,
];

fn states() -> Vec<OpState> {
    let mut states = vec![OpState::Waiting, OpState::Applied];
    states.extend(REASONS.map(OpState::Held));
    states.extend([1, 2].map(|revision| OpState::Excluded { revision }));
    states
}

fn holds() -> [Option<HoldReason>; 4] {
    [None, Some(REASONS[0]), Some(REASONS[1]), Some(REASONS[2])]
}

fn moves() -> Vec<OpMove> {
    let mut moves = vec![OpMove::Apply, OpMove::HoldInvalid, OpMove::DropGenesis];
    moves.extend(holds().map(OpMove::Reclassify));
    moves.extend(holds().map(OpMove::Release));
    moves.extend([1, 2].map(|revision| OpMove::Exclude { revision }));
    moves
}

/// Every legal move as `(from, move, to)`; every other pair is refused.
fn legal_moves() -> Vec<(OpState, OpMove, OpState)> {
    let waiting_or = |hold: Option<HoldReason>| hold.map_or(OpState::Waiting, OpState::Held);
    let mut legal = vec![
        (OpState::Waiting, OpMove::Apply, OpState::Applied),
        (
            OpState::Waiting,
            OpMove::HoldInvalid,
            OpState::Held(HoldReason::Invalid),
        ),
    ];
    for from in [
        OpState::Held(HoldReason::NewerFormat),
        OpState::Held(HoldReason::NewerManifest),
    ] {
        for hold in holds() {
            legal.push((from, OpMove::Reclassify(hold), waiting_or(hold)));
        }
    }
    for revision in [1, 2] {
        for from in states() {
            if from != (OpState::Excluded { revision }) {
                legal.push((
                    from,
                    OpMove::Exclude { revision },
                    OpState::Excluded { revision },
                ));
            }
        }
        for hold in holds() {
            legal.push((
                OpState::Excluded { revision },
                OpMove::Release(hold),
                waiting_or(hold),
            ));
        }
    }
    for from in [
        OpState::Waiting,
        OpState::Applied,
        OpState::Held(HoldReason::NewerFormat),
        OpState::Held(HoldReason::NewerManifest),
        OpState::Held(HoldReason::Invalid),
    ] {
        legal.push((
            from,
            OpMove::DropGenesis,
            OpState::Held(HoldReason::Invalid),
        ));
    }
    legal
}

#[test]
fn every_state_takes_exactly_its_legal_moves() {
    let legal = legal_moves();
    assert_eq!(legal.len(), 35);
    for from in states() {
        for attempted in moves() {
            let expected = legal
                .iter()
                .find(|(source, step, _)| *source == from && *step == attempted)
                .map(|(_, _, to)| *to)
                .ok_or(IllegalOpMove { from, attempted });
            assert_eq!(
                transition(from, attempted),
                expected,
                "{from:?} under {attempted:?}"
            );
        }
    }
}

#[test]
fn every_state_round_trips_through_its_stored_columns() {
    for state in states() {
        let (column, reason) = state.columns();
        let excluded = state
            .excluded_revision()
            .map(|revision| i64::try_from(revision).expect("revision"));
        assert_eq!(
            OpState::from_columns(column, reason, excluded).expect("decode"),
            state
        );
    }
    assert_eq!(OpState::stored(None, None), OpState::Waiting);
    for hold in holds() {
        assert_eq!(
            OpState::stored(hold, Some(3)),
            OpState::Excluded { revision: 3 },
            "an excluded operation is held as invalid whatever it was classified as"
        );
    }
}

#[test]
fn contradictory_stored_columns_are_corrupt() {
    for (state, reason, excluded) in [
        ("waiting", Some("invalid"), None),
        ("waiting", None, Some(1)),
        ("applied", Some("newer_format"), None),
        ("applied", None, Some(1)),
        ("held", None, None),
        ("held", Some("newer_format"), Some(1)),
        ("held", Some("newer_manifest"), Some(1)),
        ("held", Some("invalid"), Some(0)),
        ("held", Some("invalid"), Some(-1)),
        ("held", Some("unknown"), None),
        ("excluded", None, Some(1)),
    ] {
        assert!(
            OpState::from_columns(state, reason, excluded).is_err(),
            "{state} {reason:?} {excluded:?}"
        );
    }
}

#[test]
fn each_writer_guard_selects_exactly_the_states_its_move_leaves() {
    block_on(async {
        let pool = migrated_pool().await;
        let mut conn = pool.acquire().await.expect("connection");
        sqlx::raw_sql(
            "CREATE TEMP TABLE op_states (state TEXT NOT NULL, hold_reason TEXT,
                excluded_revision INTEGER)",
        )
        .execute(&mut *conn)
        .await
        .expect("create table");
        for from in states() {
            let (state, reason) = from.columns();
            sqlx::query("DELETE FROM op_states")
                .execute(&mut *conn)
                .await
                .expect("clear");
            sqlx::query("INSERT INTO op_states VALUES (?, ?, ?)")
                .bind(state)
                .bind(reason)
                .bind(
                    from.excluded_revision()
                        .map(|revision| i64::try_from(revision).expect("revision")),
                )
                .execute(&mut *conn)
                .await
                .expect("insert");
            for attempted in moves() {
                let sql = format!("SELECT ({}) FROM op_states", attempted.source());
                let mut query = sqlx::query(AssertSqlSafe(sql));
                if let OpMove::Exclude { revision } = attempted {
                    query = query.bind(i64::try_from(revision).expect("revision"));
                }
                let selected: bool = query
                    .fetch_one(&mut *conn)
                    .await
                    .expect("evaluate guard")
                    .try_get(0)
                    .expect("guard value");
                assert_eq!(
                    selected,
                    transition(from, attempted).is_ok(),
                    "guard of {attempted:?} on {from:?}"
                );
            }
        }
    });
}

#[test]
fn a_guarded_move_that_changes_no_row_or_several_is_corrupt() {
    assert!(op_state::expect_one(1, OpMove::Apply).is_ok());
    for affected in [0, 2] {
        assert!(op_state::expect_one(affected, OpMove::Apply).is_err());
    }
    assert!(op_state::target(OpState::Applied, OpMove::Apply).is_err());
    assert_eq!(
        op_state::target(OpState::Waiting, OpMove::Apply).expect("legal"),
        OpState::Applied
    );
}
