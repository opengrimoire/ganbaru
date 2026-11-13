//! Chat assignment budgets across replicas: a budget arrives with its limits, concurrent sets of
//! one assignment's budget merge into one row, a limit raise on one device and a used amount
//! the executing device records concurrently both survive, and a cleared budget can be set
//! again.

use super::assignments::{ASSIGNMENT, coordination_sql};
use super::*;
use crate::manifest::vault::chat::BUDGET_KEY_PREFIX;

/// A budget's token limit, working seconds limit, used tokens, and used working seconds.
type BudgetState = (Option<i64>, Option<i64>, i64, i64);

/// The derived id of [`ASSIGNMENT`]'s budget.
fn budget_id() -> String {
    key(BUDGET_KEY_PREFIX, &[ASSIGNMENT])
}

/// SQL setting [`ASSIGNMENT`]'s budget to `tokens` and `seconds`, set by [`OWNER`] at `set_at`.
fn set_sql(tokens: Option<i64>, seconds: Option<i64>, set_at: &str) -> String {
    let limit = |value: Option<i64>| value.map_or_else(|| "NULL".to_owned(), |v| v.to_string());
    format!(
        "INSERT INTO chat_assignment_budgets
            (id, assignment_id, token_limit, working_seconds_limit, set_by_participant_id, set_at)
         VALUES ({id}, {assignment}, {tokens}, {seconds}, {owner}, {set_at})
         ON CONFLICT (id) DO UPDATE SET
            token_limit = excluded.token_limit,
            working_seconds_limit = excluded.working_seconds_limit,
            set_by_participant_id = excluded.set_by_participant_id,
            set_at = excluded.set_at;",
        id = quoted(&budget_id()),
        assignment = quoted(ASSIGNMENT),
        tokens = limit(tokens),
        seconds = limit(seconds),
        owner = quoted(OWNER),
        set_at = quoted(set_at),
    )
}

/// SQL recording `tokens` used, the way the executing device does: the amount only grows.
fn used_sql(tokens: i64) -> String {
    format!(
        "UPDATE chat_assignment_budgets
         SET used_counted_tokens = max(used_counted_tokens, {tokens})
         WHERE id = {id} AND used_counted_tokens < {tokens};",
        id = quoted(&budget_id()),
    )
}

/// SQL clearing [`ASSIGNMENT`]'s budget.
fn clear_sql() -> String {
    format!(
        "DELETE FROM chat_assignment_budgets WHERE id = {};",
        quoted(&budget_id())
    )
}

/// [`ASSIGNMENT`]'s budget, if one is set.
async fn budget(replica: &mut Replica) -> Option<BudgetState> {
    sqlx::query_as::<_, (Option<i64>, Option<i64>, i64, i64)>(
        "SELECT token_limit, working_seconds_limit, used_counted_tokens, used_working_seconds
         FROM chat_assignment_budgets WHERE assignment_id = ?",
    )
    .bind(ASSIGNMENT)
    .fetch_optional(&mut replica.conn)
    .await
    .unwrap()
}

/// Replicas that share [`ASSIGNMENT`].
async fn assignment_replicas(count: usize) -> Vec<Replica> {
    let mut replicas = conversation_replicas(count).await;
    replicas[0].exec(&coordination_sql()).await;
    converge(&mut replicas).await;
    replicas
}

#[test]
fn chat_assignment_budgets_round_trip_with_their_limits() {
    block_on(async {
        let mut replicas = assignment_replicas(2).await;
        replicas[0]
            .exec(&set_sql(Some(20_000), Some(600), &at(5, 0)))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_eq!(
            budget(&mut replicas[1]).await,
            Some((Some(20_000), Some(600), 0, 0))
        );
        assert_eq!(
            replicas[1]
                .scalar_text(
                    "SELECT id || ' ' || set_by_participant_id FROM chat_assignment_budgets"
                )
                .await,
            Some(format!("{} {OWNER}", budget_id()))
        );
    });
}

#[test]
fn chat_concurrent_budget_sets_merge_into_one_row() {
    block_on(async {
        let mut replicas = assignment_replicas(3).await;
        replicas[0]
            .exec(&set_sql(Some(20_000), None, &at(5, 0)))
            .await;
        replicas[1].exec(&set_sql(None, Some(900), &at(5, 1))).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let first = budget(&mut replicas[0]).await;
        assert!(
            first == Some((Some(20_000), None, 0, 0)) || first == Some((None, Some(900), 0, 0)),
            "one whole set wins: {first:?}"
        );
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM chat_assignment_budgets")
                    .await,
                1
            );
            assert_eq!(budget(replica).await, first);
        }
    });
}

#[test]
fn chat_a_budget_raise_and_a_concurrent_used_amount_both_survive() {
    block_on(async {
        let mut replicas = assignment_replicas(3).await;
        replicas[0]
            .exec(&set_sql(Some(20_000), None, &at(5, 0)))
            .await;
        converge(&mut replicas).await;

        replicas[1]
            .exec(&set_sql(Some(40_000), None, &at(6, 0)))
            .await;
        replicas[0].exec(&used_sql(20_000)).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                budget(replica).await,
                Some((Some(40_000), None, 20_000, 0)),
                "replica {}",
                replica.index
            );
        }
    });
}

#[test]
fn chat_a_cleared_budget_can_be_set_again() {
    block_on(async {
        let mut replicas = assignment_replicas(2).await;
        replicas[0]
            .exec(&set_sql(Some(20_000), None, &at(5, 0)))
            .await;
        replicas[0].exec(&used_sql(5_000)).await;
        converge(&mut replicas).await;

        replicas[1].exec(&clear_sql()).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_eq!(budget(&mut replicas[0]).await, None);

        replicas[0]
            .exec(&set_sql(None, Some(1_200), &at(7, 0)))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let first = budget(&mut replicas[0]).await;
        assert!(
            matches!(first, Some((None, Some(1_200), _, 0))),
            "the new limits apply: {first:?}"
        );
        for replica in &mut replicas {
            assert_eq!(budget(replica).await, first, "replica {}", replica.index);
        }
    });
}
