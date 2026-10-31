//! The discovery keys of the vault this process syncs. Each sync pass follows them from the
//! authorization log into a device-local file and publishes them here, where the advertiser,
//! coordinator discovery, and Android background passes read them without opening the database.

use crate::peers::discovery_keys::DiscoveryKeys;
use ganbaru_handoff::discovery::DiscoveryKey;
use std::sync::{Mutex, PoisonError};

static PUBLISHED: Mutex<Option<DiscoveryKeys>> = Mutex::new(None);

/// Publishes the keys the latest pass followed, or clears them while the vault has no sync space.
pub fn publish(keys: Option<DiscoveryKeys>) {
    *PUBLISHED.lock().unwrap_or_else(PoisonError::into_inner) = keys;
}

/// The key this device advertises under for `vault_id`, or `None` before a pass published one.
#[cfg(desktop)]
pub fn advertised(vault_id: &str) -> Option<DiscoveryKey> {
    published_in(&PUBLISHED, vault_id).map(|keys| keys.advertised())
}

/// The keys to browse for `vault_id` at `now_ms`: the current one, then the one a removal
/// replaced during its first day. Empty before a pass published keys.
pub fn browsed(vault_id: &str, now_ms: u64) -> Vec<DiscoveryKey> {
    published_in(&PUBLISHED, vault_id)
        .map(|keys| keys.browsed(now_ms))
        .unwrap_or_default()
}

fn published_in(slot: &Mutex<Option<DiscoveryKeys>>, vault_id: &str) -> Option<DiscoveryKeys> {
    slot.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .filter(|keys| keys.vault_id == vault_id)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(vault_id: &str) -> DiscoveryKeys {
        DiscoveryKeys {
            vault_id: vault_id.to_string(),
            current: [3; 32],
            previous: Some([2; 32]),
            seen_at_ms: 1_000,
        }
    }

    #[test]
    fn published_keys_answer_only_for_their_vault() {
        let slot = Mutex::new(Some(keys("vault-a")));
        assert_eq!(published_in(&slot, "vault-a"), Some(keys("vault-a")));
        assert_eq!(published_in(&slot, "vault-b"), None);
        *slot.lock().expect("slot") = None;
        assert_eq!(published_in(&slot, "vault-a"), None);
    }
}
