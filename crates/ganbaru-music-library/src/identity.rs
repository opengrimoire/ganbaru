//! Content-derived keys for Music rows whose identity follows from their content.
//!
//! Two devices that discover the same file, follow the same YouTube playlist, or add the same item
//! to the same playlist compute the same key, so concurrent creation converges on one row. Roots,
//! playlists, presets, user soundscapes, and snoozes are user decisions and keep random keys.
//! Rows created before these rules keep their stored keys; writers match by identity key first.

use sha2::{Digest, Sha256};

/// Returns `music-{kind}-{hex}`, where `hex` is the SHA-256 of `kind` followed by a zero byte and
/// each value in order.
pub fn stable_id(kind: &str, values: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    for value in values {
        hasher.update([0]);
        hasher.update(value.as_bytes());
    }
    format!("music-{kind}-{:x}", hasher.finalize())
}

/// Returns a root-relative media path with forward slashes, the stored and keyed form.
pub fn normalize_relative_path(relative_path: &str) -> String {
    relative_path.replace('\\', "/")
}

/// Key of a local media item first discovered at `relative_path` under `root_id`.
pub fn local_item_id(root_id: &str, relative_path: &str) -> String {
    stable_id("item", &[root_id, &normalize_relative_path(relative_path)])
}

/// Key of an imported item that is not a YouTube video, derived from its portable identity key.
pub fn import_item_id(identity_key: &str) -> String {
    stable_id("import-item", &[identity_key])
}

/// Key of the library item for one YouTube video.
pub fn youtube_item_id(video_id: &str) -> String {
    format!("music-youtube-{video_id}")
}

/// Key of the location of a media file at `relative_path` under `root_id`.
pub fn location_id(root_id: &str, relative_path: &str) -> String {
    stable_id(
        "location",
        &[root_id, &normalize_relative_path(relative_path)],
    )
}

/// Identity key of the source collection that mirrors a local root.
pub fn local_root_identity_key(root_id: &str) -> String {
    format!("local-root:{root_id}")
}

/// Identity key of the source collection that mirrors a YouTube playlist.
pub fn youtube_playlist_identity_key(playlist_id: &str) -> String {
    format!("youtube-playlist:{playlist_id}")
}

/// Key of a source collection with the given identity key.
pub fn collection_id(identity_key: &str) -> String {
    stable_id("collection", &[identity_key])
}

/// Key of the membership of `item_id` in `playlist_id`, unique per pair.
pub fn membership_id(playlist_id: &str, item_id: &str) -> String {
    stable_id("membership", &[playlist_id, item_id])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_stable_id_hashes_kind_then_zero_separated_values() {
        assert_eq!(
            stable_id("item", &["root-1", "a/b.mp3"]),
            "music-item-0bc7749f6ec41f483604e58a2f6ed2a6973325d6ffc88e6abd80f492f4413af5"
        );
        assert_ne!(
            stable_id("item", &["root-1a", "/b.mp3"]),
            stable_id("item", &["root-1", "a/b.mp3"]),
            "the separator keeps value boundaries distinct"
        );
    }

    #[test]
    fn identity_local_keys_reproduce_scanned_ids_and_normalize_separators() {
        assert_eq!(
            local_item_id("root-1", "a/b.mp3"),
            "music-item-0bc7749f6ec41f483604e58a2f6ed2a6973325d6ffc88e6abd80f492f4413af5"
        );
        assert_eq!(
            local_item_id("root-1", "a\\b.mp3"),
            local_item_id("root-1", "a/b.mp3")
        );
        assert_eq!(
            location_id("root-1", "a\\b.mp3"),
            "music-location-b607cb434e270354620db1cb7a2da220d0f9debeeb2025977343157f07a22330"
        );
        assert_ne!(
            local_item_id("root-1", "a/b.mp3"),
            local_item_id("root-2", "a/b.mp3")
        );
    }

    #[test]
    fn identity_collection_membership_and_import_keys_match_the_formula() {
        assert_eq!(local_root_identity_key("root-1"), "local-root:root-1");
        assert_eq!(
            youtube_playlist_identity_key("PL123"),
            "youtube-playlist:PL123"
        );
        assert_eq!(
            collection_id(&local_root_identity_key("root-1")),
            "music-collection-b340de7f2cec6f37f126f7933ef29463ff3bb4d1de69ae49f84adc8e044cbb2c"
        );
        assert_eq!(
            membership_id("playlist-1", "item-1"),
            "music-membership-68a9cb5e741a46357c216fbd5f6781003c432cb851c3712fd66265868b6939bf"
        );
        assert_ne!(
            membership_id("playlist-1", "item-1"),
            membership_id("item-1", "playlist-1")
        );
        assert_eq!(
            import_item_id("local-file:sha256:abc"),
            "music-import-item-4ca95b8c1fb675c8ba7cc15d3accf03f303d8127c419590992ca97aad4b01ff3"
        );
        assert_eq!(youtube_item_id("dQw4w9WgXcQ"), "music-youtube-dQw4w9WgXcQ");
    }

    #[test]
    fn identity_derived_keys_fit_the_id_limit() {
        let longest = [
            local_item_id("r", "p"),
            location_id("r", "p"),
            collection_id("c"),
            membership_id("p", "i"),
            import_item_id("k"),
        ]
        .into_iter()
        .map(|id| id.len())
        .max()
        .unwrap_or_default();
        assert!(longest <= crate::validation::MAX_ID_BYTES);
    }
}
