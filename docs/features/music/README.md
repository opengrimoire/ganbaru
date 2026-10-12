# Music

Music provides persistent local and YouTube playback, playlist organization, review, background sounds, and phase-based soundtrack automation. It supports focus without classifying a listener's music as productive or distracting.

## Current scope

| Capability | Status |
| --- | --- |
| Native app-level player with queue, order modes, repeat, volume, and resume | Implemented; physical platform acceptance pending |
| Canonical local and YouTube library, refresh, repair, artwork, search, and JSON/M3U8 transfer | Implemented |
| Playlist membership settings, review, snooze, and playlist management | Implemented |
| Desktop local audio, local video, YouTube, background sounds, tray, title-bar, and hardware controls | Implemented with platform codec limits |
| Android selected-folder local audio through Media3 | Implemented in source; release validation pending |
| Project and Calendar event phase soundtrack assignments | Implemented |
| Work-environment and sleep-alarm automation | Planned |
| YouTube player presentation that complies with embedding policy | Redesign required |

## Sources

Ganbaru AI plays local media selected by the user and YouTube through an embedded player. Music files stay in the user's own library; the vault stores library identity, playlists, settings, and playback state, never copies of the media.

Spotify is unsupported because its integration model does not fit a local open-source product. This is a product boundary, not a statement about current quotas.

## Player model

Playback belongs to one native session per device, not to the visible Music panel. Closing the panel keeps the queue, source, position, and appropriate background playback alive; reopening reconnects to the same state. The player can open above Calendar, Projects, or Notes, and the playlist builder is a full workspace. Mobile uses the same library and player contracts with an adaptive presentation.

## User control

Ganbaru AI does not infer whether a track is distracting. The user chooses playlists, membership settings, snoozes, and phase behavior. Listening statistics support recency and frequency behavior without becoming a productivity score.

## Documentation map

- [Library and sources](library-and-sources.md)
- [Playback](playback.md)
- [Background sounds](soundscapes.md)
- [Playlists and review](playlists-and-review.md)
- [Automation](automation.md)
- [Android native services](../../platforms/android/native-services-and-data.md)
- [Desktop tray](../../platforms/desktop/tray.md)
