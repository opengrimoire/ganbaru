use super::{MusicLibraryError, MusicLibraryResult};

#[derive(Clone, Copy)]
pub(super) struct BuiltInMusicPlaylist {
    pub(super) id: &'static str,
    pub(super) name: &'static str,
    pub(super) icon: &'static str,
}

pub(super) const BUILT_IN_MUSIC_PLAYLISTS: &[BuiltInMusicPlaylist] = &[
    BuiltInMusicPlaylist {
        id: "playlist-default-start-of-day",
        name: "Start of the day!",
        icon: "lucide:sunrise",
    },
    BuiltInMusicPlaylist {
        id: "playlist-default-work-focus",
        name: "Work (focus)",
        icon: "lucide:laptop",
    },
    BuiltInMusicPlaylist {
        id: "playlist-default-work-ganbare",
        name: "Work (ganbare!)",
        icon: "lucide:coffee",
    },
    BuiltInMusicPlaylist {
        id: "playlist-default-break-calm",
        name: "Break (calm)",
        icon: "lucide:armchair",
    },
    BuiltInMusicPlaylist {
        id: "playlist-default-break-active",
        name: "Break (active)",
        icon: "lucide:footprints",
    },
    BuiltInMusicPlaylist {
        id: "playlist-default-meditate",
        name: "Meditate",
        icon: "lucide:smile",
    },
    BuiltInMusicPlaylist {
        id: "playlist-default-exercise",
        name: "Exercise",
        icon: "lucide:sport-shoe",
    },
    BuiltInMusicPlaylist {
        id: "playlist-default-hygiene",
        name: "Hygiene",
        icon: "lucide:bath",
    },
    BuiltInMusicPlaylist {
        id: "playlist-default-chores",
        name: "Chores",
        icon: "lucide:shopping-cart",
    },
    BuiltInMusicPlaylist {
        id: "playlist-default-cooking",
        name: "Cooking",
        icon: "lucide:apple",
    },
    BuiltInMusicPlaylist {
        id: "playlist-default-commute",
        name: "Commute",
        icon: "lucide:bike",
    },
];

pub(super) fn built_in_music_playlist(id: &str) -> Option<&'static BuiltInMusicPlaylist> {
    BUILT_IN_MUSIC_PLAYLISTS
        .iter()
        .find(|playlist| playlist.id == id)
}

pub(super) fn reject_built_in_playlist_delete(id: &str) -> MusicLibraryResult<()> {
    if built_in_music_playlist(id).is_some() {
        return Err(MusicLibraryError::conflict(
            "built-in music playlists cannot be deleted",
        ));
    }
    Ok(())
}
