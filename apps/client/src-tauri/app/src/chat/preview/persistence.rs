use super::*;
use std::future::Future;
use tokio::sync::watch;

/// Longest preview title kept in memory or SQLite, in characters because the
/// schema CHECK uses SQLite `length()`, which counts characters.
pub(super) const MAX_PREVIEW_TITLE_CHARS: usize = 2_000;
/// Minimum spacing between SQLite writes of page-driven tab metadata.
pub(super) const PREVIEW_PERSIST_INTERVAL: Duration = Duration::from_millis(500);

/// Truncates a page-controlled title to the persisted limit.
pub(super) fn bounded_preview_title(title: &str) -> String {
    title.chars().take(MAX_PREVIEW_TITLE_CHARS).collect()
}

/// Bounds a page-controlled URL to `MAX_BROWSER_URL_BYTES`. An oversized URL keeps
/// its origin and as much of its path as fits, and drops the query and fragment.
pub(super) fn bounded_preview_url(url: &reqwest::Url) -> String {
    let value = url.as_str();
    if value.len() <= MAX_BROWSER_URL_BYTES {
        return value.to_string();
    }
    let mut bounded = url.origin().ascii_serialization();
    for character in url.path().chars() {
        if bounded.len() + character.len_utf8() > MAX_BROWSER_URL_BYTES {
            break;
        }
        bounded.push(character);
    }
    bounded
}

/// Bounds a stored URL read back from SQLite before it reaches the webview or model.
pub(super) fn bounded_stored_preview_url(value: &str) -> String {
    match reqwest::Url::parse(value) {
        Ok(url) => bounded_preview_url(&url),
        Err(_) => {
            let mut end = value.len().min(MAX_BROWSER_URL_BYTES);
            while !value.is_char_boundary(end) {
                end -= 1;
            }
            value[..end].to_string()
        }
    }
}

/// Writes the latest value at most once per `interval`, skipping values equal to
/// the last write, and flushes the final value once the sender is dropped.
pub(super) async fn coalesce_writes<T, W, F>(
    mut updates: watch::Receiver<T>,
    interval: Duration,
    mut write: W,
) where
    T: Clone + PartialEq,
    W: FnMut(T) -> F,
    F: Future<Output = ()>,
{
    let mut last_written = updates.borrow_and_update().clone();
    while updates.changed().await.is_ok() {
        let latest = updates.borrow_and_update().clone();
        if latest != last_written {
            write(latest.clone()).await;
            last_written = latest;
        }
        tokio::time::sleep(interval).await;
    }
}

/// Starts the per-tab persister for page-driven metadata. It only updates an
/// existing row, so a write that lands after the tab closes cannot recreate it.
pub(super) fn spawn_tab_persister(
    pool: SqlitePool,
    initial: BrowserTabRead,
) -> Arc<watch::Sender<BrowserTabRead>> {
    let (sender, updates) = watch::channel(initial);
    tauri::async_runtime::spawn(coalesce_writes(
        updates,
        PREVIEW_PERSIST_INTERVAL,
        move |read: BrowserTabRead| {
            let pool = pool.clone();
            async move {
                if let Err(error) = update_tab_metadata(&pool, &read).await {
                    eprintln!(
                        "Could not persist browser preview tab {}: {}",
                        read.tab_id, error.message
                    );
                }
            }
        },
    ));
    Arc::new(sender)
}

async fn update_tab_metadata(pool: &SqlitePool, read: &BrowserTabRead) -> ChatResult<()> {
    sqlx::query(
        "UPDATE chat_preview_tabs
         SET current_url = ?, title = ?, loading_state = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND thread_id = ?",
    )
    .bind(&read.current_url)
    .bind(&read.title)
    .bind(if read.loading { "loading" } else { "loaded" })
    .bind(&read.tab_id)
    .bind(read.thread_id.as_str())
    .execute(pool)
    .await
    .map_err(|_| persistence_error())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn preview_titles_are_bounded_by_characters() {
        assert_eq!(bounded_preview_title("Docs"), "Docs");
        let exact = "é".repeat(MAX_PREVIEW_TITLE_CHARS);
        assert_eq!(bounded_preview_title(&exact), exact);
        let oversized = "漢".repeat(MAX_PREVIEW_TITLE_CHARS + 5);
        let bounded = bounded_preview_title(&oversized);
        assert_eq!(bounded.chars().count(), MAX_PREVIEW_TITLE_CHARS);
        assert!(oversized.starts_with(&bounded));
    }

    #[test]
    fn preview_urls_keep_origin_and_fit_the_byte_limit() {
        let short = reqwest::Url::parse("http://localhost:5173/a?b=c#d").unwrap();
        assert_eq!(bounded_preview_url(&short), short.as_str());

        let base = "http://localhost:5173/";
        let exact = format!("{base}{}", "a".repeat(MAX_BROWSER_URL_BYTES - base.len()));
        let exact = reqwest::Url::parse(&exact).unwrap();
        assert_eq!(bounded_preview_url(&exact), exact.as_str());

        let oversized = format!(
            "{base}{}?{}",
            "%E6%BC%A2".repeat(MAX_BROWSER_URL_BYTES),
            "q".repeat(100)
        );
        let oversized = reqwest::Url::parse(&oversized).unwrap();
        let bounded = bounded_preview_url(&oversized);
        assert!(bounded.len() <= MAX_BROWSER_URL_BYTES);
        assert!(bounded.starts_with("http://localhost:5173/%E6%BC%A2"));
        assert!(!bounded.contains('?'));

        let stored = "x".repeat(MAX_BROWSER_URL_BYTES + 3);
        assert_eq!(
            bounded_stored_preview_url(&stored).len(),
            MAX_BROWSER_URL_BYTES
        );
        let multibyte = "é".repeat(MAX_BROWSER_URL_BYTES);
        assert!(bounded_stored_preview_url(&multibyte).len() <= MAX_BROWSER_URL_BYTES);
    }

    #[tokio::test(start_paused = true)]
    async fn rapid_metadata_updates_are_coalesced_and_the_final_value_persists() {
        let (sender, updates) = watch::channel(String::new());
        let writes = Arc::new(AtomicUsize::new(0));
        let last = Arc::new(Mutex::new(String::new()));
        let task = tokio::spawn({
            let writes = writes.clone();
            let last = last.clone();
            coalesce_writes(updates, PREVIEW_PERSIST_INTERVAL, move |value: String| {
                writes.fetch_add(1, Ordering::SeqCst);
                *last.lock().unwrap() = value;
                async {}
            })
        });

        let started = tokio::time::Instant::now();
        for index in 0..10_000 {
            sender.send_replace(format!("title {index}"));
            if index % 100 == 0 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }
        drop(sender);
        task.await.unwrap();

        let elapsed = started.elapsed();
        let allowed = elapsed.as_millis() / PREVIEW_PERSIST_INTERVAL.as_millis() + 2;
        assert!(writes.load(Ordering::SeqCst) as u128 <= allowed);
        assert_eq!(*last.lock().unwrap(), "title 9999");
    }

    #[tokio::test(start_paused = true)]
    async fn unchanged_metadata_is_not_rewritten() {
        let (sender, updates) = watch::channel("same".to_string());
        let writes = Arc::new(AtomicUsize::new(0));
        let task = tokio::spawn({
            let writes = writes.clone();
            coalesce_writes(updates, PREVIEW_PERSIST_INTERVAL, move |_: String| {
                writes.fetch_add(1, Ordering::SeqCst);
                async {}
            })
        });
        for _ in 0..5 {
            sender.send_replace("same".to_string());
            tokio::time::sleep(PREVIEW_PERSIST_INTERVAL).await;
        }
        drop(sender);
        task.await.unwrap();
        assert_eq!(writes.load(Ordering::SeqCst), 0);
    }
}
