//! Calendar persistence, iCalendar import and export, native reads, scoped edits, deletion and Undo, and recurrence expansion.
//!
//! The crate is Tauri-free. Functions take an authorized SQLite pool or transaction, and
//! platform services such as the device date are injected by the application.

pub mod calendars;
pub mod conversion;
pub mod description;
pub mod events;
pub mod geo;
pub mod import;
pub mod keys;
pub mod reads;
pub mod recurrence;
mod stored_number;
pub mod value_bound;
pub mod work_gate;

#[cfg(test)]
mod test_support {
    /// Runs one async test body on a current-thread runtime.
    pub(crate) fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("create calendar test runtime")
            .block_on(future)
    }
}
