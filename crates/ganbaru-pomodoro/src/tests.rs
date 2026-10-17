mod aggregate_budget;
mod assignments;
mod calendar_history;
mod close_outcomes;
mod helpers;
mod history_replay;
mod mature_outcomes;
mod recovery;
mod run_snapshots;
mod validation_cases;

/// Runs persistence tests on a single-thread runtime without platform initialization.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create focus test runtime")
        .block_on(future)
}
