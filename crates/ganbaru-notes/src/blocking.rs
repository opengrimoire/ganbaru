//! Bounded blocking workers for Notes filesystem work.
//!
//! Notes commands run on the async executor, so reads, scans, hashing, and writes whose cost
//! depends on user files run here instead. Each call takes one permit for one operation, never
//! for a whole import, so a long Notion import interleaves with attaching a file or reading
//! working Markdown rather than holding them back. The semaphore is first in, first out.

use std::sync::{Arc, LazyLock};

use tokio::sync::Semaphore;

/// Notes filesystem operations that may run at once across the process.
const MAX_NOTES_FILESYSTEM_WORKERS: usize = 4;

static NOTES_FILESYSTEM_WORKERS: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(MAX_NOTES_FILESYSTEM_WORKERS)));

/// Runs one blocking Notes filesystem operation on a worker thread.
///
/// `work` must not wait for another `run_blocking` call. Native dialogs do not belong here:
/// a dialog left open would hold a permit for as long as the user takes.
pub async fn run_blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    run_gated(&NOTES_FILESYSTEM_WORKERS, work).await
}

async fn run_gated<T, F>(gate: &Arc<Semaphore>, work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    let permit = Arc::clone(gate)
        .acquire_owned()
        .await
        .map_err(|error| format!("wait for a Notes file worker: {error}"))?;
    tokio::task::spawn_blocking(move || {
        // The worker owns the permit, so a dropped caller cannot free it while work still runs.
        let _permit = permit;
        work()
    })
    .await
    .map_err(|error| format!("Notes file worker failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    #[tokio::test]
    async fn workers_never_exceed_the_permit_count_and_return_permits_on_errors() {
        const LIMIT: usize = 2;
        let gate = Arc::new(Semaphore::new(LIMIT));
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let (release, released) = mpsc::channel::<()>();
        let released = Arc::new(Mutex::new(released));

        let mut tasks = Vec::new();
        for index in 0..LIMIT + 2 {
            let gate = Arc::clone(&gate);
            let running = Arc::clone(&running);
            let peak = Arc::clone(&peak);
            let released = Arc::clone(&released);
            tasks.push(tokio::spawn(async move {
                run_gated(&gate, move || {
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    let waited = released
                        .lock()
                        .expect("release channel lock")
                        .recv_timeout(Duration::from_secs(10));
                    running.fetch_sub(1, Ordering::SeqCst);
                    waited.map_err(|error| error.to_string())?;
                    if index % 2 == 0 {
                        Ok(index)
                    } else {
                        Err(format!("work {index} failed"))
                    }
                })
                .await
            }));
        }

        while running.load(Ordering::SeqCst) < LIMIT {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(running.load(Ordering::SeqCst), LIMIT);
        for _ in 0..LIMIT + 2 {
            release.send(()).expect("release a worker");
        }

        let mut failures = 0;
        for task in tasks {
            if task.await.expect("task joins").is_err() {
                failures += 1;
            }
        }
        assert_eq!(failures, (LIMIT + 2) / 2);
        assert_eq!(peak.load(Ordering::SeqCst), LIMIT);
        assert_eq!(gate.available_permits(), LIMIT);
    }
}
