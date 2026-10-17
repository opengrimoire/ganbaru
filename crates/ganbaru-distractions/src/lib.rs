//! Distraction blocker domain independent of Tauri and the WebView: wire contracts, desktop
//! application name rules and protected applications, usage limit budgets, usage sample
//! normalization, and evidence-bounded elapsed accounting.
//! Callers own observation, enforcement, close authorization, state files, and the device spool.

pub mod accounting;
pub mod contracts;
pub mod limits;
pub mod rules;
pub mod usage;

#[cfg(test)]
mod test_support {
    /// Runs one async test body on a current-thread Tokio runtime.
    pub(crate) fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("create distractions test runtime")
            .block_on(future)
    }
}
