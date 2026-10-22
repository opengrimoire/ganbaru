//! Whether an event starts a playlist or soundscape, for Calendar indicators.

use ganbaru_db::impl_sqlite_from_row;

pub(super) const EVENT_MUSIC_SQL: &str = "SELECT owner_id, owner_kind, phase, behavior,
       playlist_id IS NOT NULL AS has_playlist, soundscape_behavior,
       soundscape_preset_id IS NOT NULL AS has_soundscape_preset
     FROM music_context_assignments
     WHERE owner_kind IN ('event-override', 'event-snapshot')
       AND owner_id IN (SELECT value FROM json_each(?1))
     ORDER BY owner_id, owner_kind, phase";

pub(super) const EVENT_MUSIC_TEXT_COLUMNS: &[&str] = &[
    "owner_id",
    "owner_kind",
    "phase",
    "behavior",
    "soundscape_behavior",
];

const OVERRIDE: &str = "event-override";
const SNAPSHOT: &str = "event-snapshot";
const FOCUS_PHASE: &str = "focus";
const POMODORO_PHASES: &[&str] = &["focus", "short-break", "long-break"];
const INHERIT: &str = "inherit";

/// One event music assignment reduced to the fields that decide playback.
pub(crate) struct EventMusicAssignment {
    pub owner_id: String,
    pub owner_kind: String,
    pub phase: String,
    pub behavior: String,
    pub has_playlist: bool,
    pub soundscape_behavior: String,
    pub has_soundscape_preset: bool,
}
impl_sqlite_from_row!(EventMusicAssignment {
    owner_id,
    owner_kind,
    phase,
    behavior,
    has_playlist,
    soundscape_behavior,
    has_soundscape_preset,
});

/// Playback an event starts in at least one of its phases.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct EventMusicFlags {
    pub playlist: bool,
    pub soundscape: bool,
}

/// Resolve each phase like the native session: an override decides music or soundscape
/// only when its own behavior is set, otherwise the project snapshot does. Without
/// Pomodoro the event runs as a single Focus phase.
pub(crate) fn event_music_flags<'a>(
    assignments: impl IntoIterator<Item = &'a EventMusicAssignment> + Clone,
    pomodoro: bool,
) -> EventMusicFlags {
    let phases: &[&str] = if pomodoro {
        POMODORO_PHASES
    } else {
        &[FOCUS_PHASE]
    };
    let find = |kind: &str, phase: &str| {
        assignments
            .clone()
            .into_iter()
            .find(|row| row.owner_kind == kind && row.phase == phase)
    };
    let mut flags = EventMusicFlags::default();
    for phase in phases {
        let override_row = find(OVERRIDE, phase);
        let snapshot_row = find(SNAPSHOT, phase);
        let music = override_row
            .filter(|row| row.behavior != INHERIT)
            .or(snapshot_row);
        let sound = override_row
            .filter(|row| row.soundscape_behavior != INHERIT)
            .or(snapshot_row);
        flags.playlist |= music.is_some_and(plays_playlist);
        flags.soundscape |= sound.is_some_and(plays_soundscape);
    }
    flags
}

fn plays_playlist(row: &EventMusicAssignment) -> bool {
    matches!(
        row.behavior.as_str(),
        "play-automatically" | "prepare-silently"
    ) && row.has_playlist
}

fn plays_soundscape(row: &EventMusicAssignment) -> bool {
    row.soundscape_behavior == "play-selected" && row.has_soundscape_preset
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: &str, phase: &str, behavior: &str, sound: &str) -> EventMusicAssignment {
        EventMusicAssignment {
            owner_id: "event".into(),
            owner_kind: kind.into(),
            phase: phase.into(),
            behavior: behavior.into(),
            has_playlist: matches!(behavior, "play-automatically" | "prepare-silently"),
            soundscape_behavior: sound.into(),
            has_soundscape_preset: sound == "play-selected",
        }
    }

    #[test]
    fn snapshot_playlist_plays_when_override_inherits() {
        let rows = [
            row(SNAPSHOT, "focus", "play-automatically", "inherit"),
            row(OVERRIDE, "focus", "inherit", "inherit"),
        ];
        let flags = event_music_flags(&rows, false);
        assert!(flags.playlist);
        assert!(!flags.soundscape);
    }

    #[test]
    fn override_pause_silences_snapshot_playlist() {
        let rows = [
            row(SNAPSHOT, "focus", "play-automatically", "inherit"),
            row(OVERRIDE, "focus", "pause-music", "inherit"),
        ];
        assert_eq!(event_music_flags(&rows, false), EventMusicFlags::default());
    }

    #[test]
    fn soundscape_resolves_independently_of_music() {
        let rows = [
            row(SNAPSHOT, "focus", "play-automatically", "play-selected"),
            row(OVERRIDE, "focus", "pause-music", "inherit"),
        ];
        let flags = event_music_flags(&rows, false);
        assert!(!flags.playlist);
        assert!(flags.soundscape);
    }

    #[test]
    fn override_keep_soundscape_hides_snapshot_preset() {
        let rows = [
            row(SNAPSHOT, "focus", "inherit", "play-selected"),
            row(OVERRIDE, "focus", "inherit", "keep-current-soundscape"),
        ];
        assert_eq!(event_music_flags(&rows, false), EventMusicFlags::default());
    }

    #[test]
    fn break_music_counts_only_with_pomodoro() {
        let rows = [row(
            OVERRIDE,
            "short-break",
            "play-automatically",
            "inherit",
        )];
        assert_eq!(event_music_flags(&rows, false), EventMusicFlags::default());
        assert!(event_music_flags(&rows, true).playlist);
    }

    #[test]
    fn playlist_behavior_without_playlist_plays_nothing() {
        let mut playing = row(OVERRIDE, "focus", "play-automatically", "inherit");
        playing.has_playlist = false;
        assert_eq!(
            event_music_flags(&[playing], false),
            EventMusicFlags::default()
        );
    }
}
