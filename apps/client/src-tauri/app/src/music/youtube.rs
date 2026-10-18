//! YouTube embed page, metadata lookup, and thumbnail caching.

pub(crate) mod metadata;
#[cfg(any(test, mobile))]
pub(crate) mod native_host;
pub(super) mod player_page;
pub(crate) mod thumbnail;
