# Music library and sources

## Canonical model

SQLite owns canonical media items, source collections, local roots and locations, playlist memberships, skip ranges, review state, snoozes, listening statistics, and playback state.

A media item represents stable content identity. Locations and source collections describe where it can currently be found. Playlist membership describes how that item behaves in one playlist. Keeping identity, location, and per-playlist settings separate lets a file move or a source go offline without losing authored choices.

## Local sources on desktop

Users select a folder and the app scans supported audio and video files without moving them. Scans read folders in bounded batches, are deterministic and cancellable, skip symbolic links inside the folder, and stop with an error past two million files and folders without marking unseen media missing; a newer scan safely supersedes an older one.

The library stores a stable root plus relative location and file metadata to detect moved, missing, changed, or ambiguous media. Refresh preserves user-authored title, artist, album, artwork, review, snooze, and playlist choices when identity can still be established. Missing files stay visible with a repair path, and relinking previews its effect and never silently binds ambiguous matches. A relink scan can be cancelled while it runs and stops with an error past two million files and folders, so picking a whole disk by mistake ends instead of running unbounded.

The folder chosen for a root is device-local state, not vault data. A source whose root has no folder on this device, or whose folder is gone, shows that state with a choose-folder action instead of an empty list, and Refresh offers the same action rather than skipping it silently. A root that was never linked on this device is bound directly and scanned; a moved folder goes through relink, which also binds the replacement folder. Folders, replacement files, and background audio files come from the native picker on this device; a typed or pasted local file plays after one native confirmation.

## Local sources on Android

Android uses the system directory-tree picker, retains the granted read permission, and stores the tree identity plus relative document identities. It never fabricates desktop-style absolute paths. Scans are bounded by item count and depth and run off the main thread. A lost permission is repaired by selecting the folder again. Android currently supports audio only, not local video.

## YouTube sources

Users can add YouTube video and playlist links. Playlist links resolve into canonical video items, so Ganbaru owns queue order without requiring a YouTube Data API key. Resolution has a timeout and explicit ready, unavailable, embedding-blocked, and timed-out states. Missing metadata keeps a stable fallback label.

Owner-disabled embedding is never bypassed by scraping streams, proxying media, impersonating clients, or download tools. The recovery action opens the video on YouTube.

## Artwork and metadata

Local artwork comes from a user override, a matching sidecar image, a common album-cover file, or embedded metadata, with explicit overrides winning. Discovery searches the track folder, common artwork subfolders, and parent folders up to the source root. It follows symbolic links to linked folders or covers with each resolved folder read once and capped, and accepts a sidecar only when its contents start with a supported image signature. Sidecar paths stored in the vault are refused when they are absolute, rooted, or contain `..`. Missing or invalid artwork never prevents playback. Managed overrides are vault assets; sidecars stay with the music. Stored title, artist, and album text is capped at 500 characters; longer tag or import text is truncated, so a crafted file or transfer never blocks a scan or bloats the library.

YouTube thumbnails are kept in a size-bounded, device-local cache outside the vault that expires within 30 days. They are not transformed and are excluded from vault backups and transfers.

## Source health

Sources report refresh state, availability, missing permission, missing files, ambiguous matches, unsupported media, and embedding failures. Refresh and relink live with source management rather than being repeated on every playlist. Repair never removes playlist membership or authored metadata because a source is temporarily offline.

## Search

Search is a rebuildable index over titles, artists, albums, source collections, relative locations, and playlist metadata. Indexed text is never authoritative.

## Import and export

Ganbaru exports and imports playlists as versioned JSON and as M3U8. Transfers carry source identity, playlists, membership configuration, and user-authored metadata, never media bytes.

- JSON preserves logical roots and source identities, never device-local absolute paths or Android content URIs. Imports require the current format, including every current playlist field. Unsupported membership kinds are skipped with diagnostics; malformed supported records fail validation.
- M3U8 knowingly loses weights, snoozes, assignments, listening signals, and focus guidance. Relative M3U paths can be mapped to a selected Android root, and Android exports omit content URIs because they are not portable paths. M3U paths match library locations literally, with either path separator and case-insensitively on Windows roots.
- Parsing, matching, preview, and export assembly run natively. Previews return counts, mapping requirements, and bounded diagnostics instead of loading the whole library into the WebView. Unsafe relative paths cannot escape their logical root.
- An import commits against the reviewed library revision; relevant changes since review require another review. The import and its action receipt commit together, so retrying an uncertain result never duplicates playlists.
- Exports read one consistent SQLite snapshot.
- Transfers are bounded (currently 8 MiB, 500 playlists or roots, and 10,000 memberships, plus a shared read allowance). Exceeding a limit fails explicitly; a library is never silently truncated.

Android uses the bounded system document picker and Downloads adapter.

## Privacy

Local file names, paths, listening history, and playlist choices stay local. YouTube receives only the requests its embedded player and metadata surfaces require when the user loads a YouTube source. Ganbaru adds no analytics around them.
