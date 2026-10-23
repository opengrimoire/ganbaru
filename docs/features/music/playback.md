# Music playback

## Native session owner

One Rust-owned Music session per device owns the active source, queue, source generation, eligibility, listening records, and playback boundaries. The Music panel, playlist builder, title bar, tray, media keys, and Calendar and Focus automation all issue commands to that owner. Browser media (YouTube and local video) remain frontend adapters that report observations and execute generation-tagged effects; they never choose the next track.

Why native: playback must survive closed panels, suspended WebView JavaScript, detached windows, and vault handoff, and automation must not race a second frontend decision. A single serialized owner makes queue progression, automation, and recovery deterministic.

Status: implemented in source. Physical desktop controls and Android background, Activity destruction, and lock-screen behavior still need acceptance before release readiness is claimed.

Key rules:

- Closing a visible panel does not stop playback. Explicit stop, end of queue, an automation decision, application exit, or a platform lifecycle boundary can.
- Commands serialize through one bounded owner. Checkpoints, listening outcomes, and action receipts commit together before decoder effects are published. A retried command with the same action identity never executes twice.
- Snooze and preference changes ask the owner to reconcile the queue; surfaces do not issue their own Next or Pause from a stale snapshot.
- Automatic soundtrack selection (Calendar and Focus) is native. See [Music automation](automation.md) for precedence and ownership.
- Automatic soundscape intent is recorded separately from manual intent. Startup shows the actual native output and never replays an automatic selection from a saved flag; only a freshly accepted phase or an explicit manual action can start it.
- Before audible output, source preparation rechecks phase, state version, vault lifecycle revision, and expiry, so obsolete work cannot become audible.

## Vault lifecycle

Vault changes revoke new playback admission immediately and wait a bounded time for the native owner to pause, stop its backend, and release its database context. Handoff waits for acknowledged shutdown of music and soundscapes or reports a timeout. A newer resume always wins over an older queued freeze, and reactivation rereads the authorized vault without resuming playback automatically. Focus and Music share these lifecycle revision rules.

## Transport and queue

The player supports play, pause, previous, next, seek where the backend supports it, volume, mute, one playback-order control, and playlist repeat. Queue identity stays stable across source transitions so controls never briefly target the previous item. The main player does not expose a speed control; playlist membership rate overrides still apply.

On desktop, the Music volume is an app volume apart from the system volume. On Android, the volume control is the phone's media volume, so it follows the hardware buttons and system controls, and Music plays at full app volume beneath it; mute stays an app control. Devices whose volume apps cannot change keep the app volume. Desktop soundscapes play relative to the Music volume; see [Soundscapes](soundscapes.md).

Playback order has three exclusive modes:

- **In order** follows stored playlist membership order.
- **Shuffle** plays a randomized pass through eligible tracks without repeats before repeating. It ignores Mix frequency and never mutates stored order.
- **Mix** is ongoing weighted selection with replacement. Frequency weights are 0.5, 0.75, 1 (Normal), 1.5, and 2. The last five distinct selections receive a soft penalty, so an immediate repeat is possible but uncommon. Mix never learns from skips or silently changes preferences, and playlist repeat does not apply while it is selected. Source queues use the same recent-repeat guard with equal weights.

Snoozed, disabled, missing, or unavailable items are skipped with an explainable reason. Clicking an inactive queue item starts it from its membership start; clicking the active item toggles play and pause where the surface communicates that.

### Track preferences

One compact track-preferences popover holds Mix frequency and Snooze with a shared scope selector (the playing playlist when the track belongs to it, otherwise Everywhere). Everywhere Snooze applies across playlists; Everywhere frequency updates every current membership of the track. Frequency is disabled when the track has no memberships, and preferences can be prepared while another order mode is active (a hint points to Mix). A frequency change applies optimistically, reverts on failure, and does not interrupt the current track.

Snooze offers 1 day, 1 week, and 1 month. Snoozing the playing track skips it when the scope applies to the current queue, and pauses if nothing else can play. Selecting the active duration again removes the snooze. Snoozed queue rows show a clock that removes the snooze without starting the track.

The current-track playlist button edits the track's memberships as a draft applied together on Save, and can open the playlist builder's Review on that track. See [Playlists and review](playlists-and-review.md) for membership settings.

## Resume and projections

Playback position, duration, status, queue context, and source generation persist with monotonic ordering, so a late save from an older source cannot overwrite newer state. Source transitions and listening outcomes commit immediately; position is checkpointed periodically.

Checkpoints are keyed by local device identity and contain only portable item and membership identifiers, bounded navigation history, selection, and playback settings. They never contain resolved file paths, Android content URIs, or decoder handles. A copied vault may carry another device's checkpoint, but it never becomes this device's live session; each device restores its own checkpoint paused. After an app launch the queue continues on the next track from its start, following the saved playback order and starting the queue or shuffle pass over when it was finished, so returning feels like a fresh start; Previous returns to the track that was playing. Reopening a vault during the same run, such as after a handoff or a vault switch, keeps the saved track and position. A restart is not reported as an interruption, but the first Play reloads the restored selection, including browser sources restored before their host was available. Ad hoc path or URL sources are transient and create no portable checkpoint.

Resume respects membership start and end boundaries, never resumes an ended item as paused, and reports changed or unsupported media as recoverable instead of seeking blindly.

WebViews receive small notices and read retained, ordered frames through typed commands, with at most one unacknowledged frame per window. Re-reading or re-acknowledging a frame after a lost response cannot replay queue intent, and an expired frame cannot start browser playback. Vault handoff or a changed native context revokes retained frames and sends a terminal invalidation; the frontend then drops pending browser work and old presentation before subscribing to the new context.

The configured primary window alone hosts browser decoders. Detached windows show snapshots and send native commands without creating another player. Losing the primary host does not stop native audio; browser-owned playback pauses with an explicit unavailable-host state and returns paused when the host does.

## Desktop local audio

Desktop local audio uses a Rust audio backend for MP3, FLAC, AAC or ALAC in MP4/M4A, OGG Vorbis, and WAV/PCM. Unsupported formats fail explicitly. The native session observes decoder position and completion independently of the WebView, and tray and operating-system controls enter the same owner directly.

Each decoder operation (inspection, preparation, settings, and accepted autoplay) runs in one admitted worker with a bounded acknowledgement wait. A timed-out operation keeps its admission until it actually finishes, so slow I/O cannot pile up waiting workers; further operations fail explicitly meanwhile. A late cancelled result stops its prepared decoder.

Decoder and invalid-source errors may advance the queue. Worker, device, seek, expired-authority, and uncertain-delivery failures instead retain the selected item and position as an interruption; explicit Play reloads that selection rather than trusting an unknown decoder.

## Local video

Desktop local video uses the platform WebView media stack, served through an app-owned, token-gated loopback source that serves only registered files and supports byte ranges. Codec support therefore follows the installed WebView. Ganbaru does not ship a separate native video framework without measured evidence that the WebView path is inadequate. Registrations are bounded and released when no longer needed.

## Android playback

Android local audio uses ExoPlayer inside a Media3 session service, which owns background playback, audio focus, media notification, headset disconnection, lock-screen controls, and trusted system controllers. Rust still owns queue selection and membership boundaries; Kotlin does not keep a competing queue, and trusted controllers can send only transport intent. The media notification shows the library title, artist, and album (including user edits), with the file's tags and embedded artwork filling anything the library leaves empty, and offers play and pause, previous, next, and seeking. The desktop audio backend and loopback media host are not compiled on Android.

Native effects reach the service through a bounded queue drained on the main thread. Success means the SDK effect actually ran (a Load acknowledgement means the decoder was reset and resolution scheduled; Stop means the decoder is stopped and cleared). Every delivery carries its vault revision and, for Focus-owned playback, its phase lease, and is rechecked before execution and after document resolution, so stale work cannot start after a freeze, phase change, or service restart. Document resolution has a 30-second elapsed-realtime deadline; expiry is an interruption, not a failed-track skip. Service termination is likewise an interruption that retains the selection. Canonical Pause wins over late Playing reports.

See the [Media3 background playback contract](https://developer.android.com/media/media3/session/background-playback) and [Android native services](../../platforms/android/native-services-and-data.md).

## YouTube playback and redesign gap

YouTube plays through an embedded IFrame player on a host page that supplies the required HTTP embedding context: a token-gated loopback host on desktop, and a separate native WebView on Android. Embedding failures and owner restrictions are surfaced explicitly. A YouTube item is eligible unless its resolution state is embedding-blocked or unavailable, and resolution state replicates with the library, so a video resolved on one linked device plays on another with no refresh; a video still resolving or timed out is attempted. Playlist eligible and unavailable counts and review issues use the same rule, while a timed-out video also appears as a review issue. If the queue reaches a browser-owned source while its host is unavailable, the item stays selected and playback pauses; the session does not skip it or promise background YouTube playback.

The current presentation disables pointer input on the embedded frame and places a transparent Ganbaru hit target over it. [YouTube Required Minimum Functionality](https://developers.google.com/youtube/terms/required-minimum-functionality) forbids overlays over the embedded player, so this is an unresolved redesign gap and must not be described as compliant. A redesign must keep usable Ganbaru controls without covering required player interaction and be reviewed against the current policy. The gap never justifies scraping, proxying, downloading, or bypassing embedding restrictions.

## Errors and recovery

Errors distinguish invalid input, missing permission, missing or ambiguous file, unsupported format, decode failure, network failure, embedding restriction, and unavailable source. Retry never duplicates queue items or revives a stale source generation.

When part of a playlist is unavailable, the player continues with the eligible subset and quietly identifies omissions. A non-empty playlist with no playable tracks links to its review workspace. Selecting an empty playlist does not interrupt current playback.

## Accessibility

Transport controls have stable accessible names and keyboard operation. Media status is conveyed through text and state, not motion alone. Fullscreen and compact surfaces keep an exit path and do not trap focus.

## Acceptance

Manual desktop checks live in [Music desktop acceptance](../../testing/music.md); Android checks live in the [Android acceptance matrix](../../testing/android.md#music).
