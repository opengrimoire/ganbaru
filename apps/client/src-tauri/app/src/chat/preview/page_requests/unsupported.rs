//! Platforms without a page request bridge keep their engine's own handling.

use super::*;

/// No request is ever held on these platforms.
pub(super) enum HeldRequest {}

impl HeldRequest {
    pub(super) fn answer(self, _answer: PageAnswer) {
        match self {}
    }
}

pub(super) fn install(_webview: &tauri::Webview, _route: PageRequestRoute) -> ChatResult<()> {
    Ok(())
}
