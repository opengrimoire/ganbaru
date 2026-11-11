//! When a live Codex session reloads its MCP configuration before a turn.

use crate::models::ProviderInternalMcpConfig;
use sha2::{Digest, Sha256};

/// What a live session knows about the MCP configuration its app server loaded at open.
///
/// The app server receives the internal endpoint URL through `-c` overrides and its token
/// through the spawn environment, so both are fixed for the life of the process. The digest
/// detects a driver whose endpoint was replaced after open; the flag keeps the reload that the
/// user setting or MCP launch arguments ask for on every turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::codex) struct CodexMcpReloadState {
    endpoint_digest: [u8; 32],
    reload_every_turn: bool,
}

impl CodexMcpReloadState {
    /// The state for a session that opened with `endpoint`.
    pub(in crate::codex) fn at_open(
        endpoint: Option<&ProviderInternalMcpConfig>,
        reload_every_turn: bool,
    ) -> Self {
        Self {
            endpoint_digest: endpoint_digest(endpoint),
            reload_every_turn,
        }
    }

    /// Reports whether the next turn reloads MCP configuration for `endpoint`, and records
    /// `endpoint` as loaded when it does.
    pub(in crate::codex) fn take_reload(
        &mut self,
        endpoint: Option<&ProviderInternalMcpConfig>,
    ) -> bool {
        let digest = endpoint_digest(endpoint);
        let changed = digest != self.endpoint_digest;
        self.endpoint_digest = digest;
        changed || self.reload_every_turn
    }
}

/// SHA-256 over the length-prefixed endpoint URL and token, or over nothing without an endpoint.
fn endpoint_digest(endpoint: Option<&ProviderInternalMcpConfig>) -> [u8; 32] {
    let mut hasher = Sha256::new();
    if let Some(endpoint) = endpoint {
        for part in [endpoint.url.as_bytes(), endpoint.bearer_token.as_bytes()] {
            hasher.update((part.len() as u64).to_be_bytes());
            hasher.update(part);
        }
    }
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(url: &str, token: &str) -> ProviderInternalMcpConfig {
        ProviderInternalMcpConfig {
            name: "ganbaru-chat".to_string(),
            url: url.to_string(),
            bearer_token: token.to_string(),
            organizational_authority: false,
        }
    }

    #[test]
    fn unchanged_endpoint_skips_the_reload() {
        let first = endpoint("http://127.0.0.1:41827/mcp", "token-a");
        let mut state = CodexMcpReloadState::at_open(Some(&first), false);
        assert!(!state.take_reload(Some(&first)));
        let mut without = CodexMcpReloadState::at_open(None, false);
        assert!(!without.take_reload(None));
    }

    #[test]
    fn changed_url_or_token_reloads_once() {
        let first = endpoint("http://127.0.0.1:41827/mcp", "token-a");
        let mut state = CodexMcpReloadState::at_open(Some(&first), false);
        let rotated = endpoint("http://127.0.0.1:41827/mcp", "token-b");
        assert!(state.take_reload(Some(&rotated)));
        assert!(!state.take_reload(Some(&rotated)));
        let moved = endpoint("http://127.0.0.1:41828/mcp", "token-b");
        assert!(state.take_reload(Some(&moved)));
        assert!(state.take_reload(None));
    }

    #[test]
    fn length_prefix_keeps_url_and_token_apart() {
        let joined = endpoint("http://a/b", "c");
        let shifted = endpoint("http://a/", "bc");
        assert_ne!(
            endpoint_digest(Some(&joined)),
            endpoint_digest(Some(&shifted))
        );
    }

    #[test]
    fn reload_setting_reloads_every_turn() {
        let first = endpoint("http://127.0.0.1:41827/mcp", "token-a");
        let mut state = CodexMcpReloadState::at_open(Some(&first), true);
        assert!(state.take_reload(Some(&first)));
        assert!(state.take_reload(Some(&first)));
    }
}
