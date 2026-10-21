# Music automation

Music automation selects playlist and soundscape behavior for Pomodoro phases and Calendar occurrences without moving per-track configuration into Calendar events. All automatic selection happens in the native Music session (see [Playback](playback.md)); the frontend only renders provenance and accepted output.

## Phase assignments

Assignments are defined independently for focus, short break, long break, and the no-Pomodoro context of a timed event. Each phase can:

- Inherit from a lower-precedence source.
- Play a selected playlist automatically.
- Prepare a selected playlist without starting it.
- Pause music.
- Keep the current music.
- Select one soundscape preset, pause, keep, or inherit the soundscape independently where supported.

Assignments store playlist and soundscape intent only. Start, end, skip, rate, volume, weighting, and enabled state remain playlist-membership behavior.

The Project settings UI exposes a playlist and, where soundscapes are supported, a soundscape for Focus, Short break, and Long break under Event defaults. A chosen playlist plays automatically and an empty choice means silence; each phase boundary starts a fresh track, avoiding the previous one when alternatives exist. The soundscape choice is Silence (the default) or a preset, so a phase can play music without a soundscape or a soundscape without music. The Calendar event panel uses the same choices for event overrides: one Playlist and one Soundscape row without Pomodoro, or a playlist and soundscape per phase with Pomodoro. Each choice shows the effective value and resolves separately; choosing the project's value again clears that part of the override, and Silence over a project preset is stored as keeping the current soundscape. The section toggle stores an explicit pause, which stops any soundscape. The section toggle silences both music and the soundscape or restores the project choices. The broader assignment record supports the full behavior list above. Configuring soundscapes inside playlists is planned.

## Ownership and precedence

Assignment sources:

- **Project default:** the reusable project setting.
- **Event snapshot:** project intent copied when an event is created or explicitly refreshed. Editing the project later does not rewrite existing snapshots.
- **Event override:** an explicit event-specific choice, always distinguishable from inherited intent.
- **Work environment:** planned.

Precedence is event override, then a future work-environment assignment, then the event's project snapshot. This holds even for assignments that only select a soundscape. Missing or deleted playlists and presets produce an explicit unavailable result instead of silently selecting something else.

## Calendar activation (desktop)

When a timed event without Pomodoro is active, the native owner reads canonical Calendar occurrences and resolves its assignment. If events overlap, the one ending earliest wins, then creation order and stable identity. The owner schedules the next known boundary and refreshes on edits. All-day, cancelled, and Focus-configured events cannot manufacture a Focus phase. A failed Calendar read leaves unrelated playback running.

## Focus activation

An open committed Focus run takes priority over Calendar automation. Committed Focus transitions apply their assignment through the native owner with a bounded validity lease tied to the run, segment, and vault generation. Duplicate delivery cannot restart a track or extend its lease; an expired or revoked lease pauses playback still owned by that effect, and delayed older effects cannot revive it. Scheduled Calendar projections alone never authorize Focus playback.

A background-only assignment leaves main music ownership unchanged. The accepted state commits before decoder delivery.

## Manual control and ownership

Manual actions always win until the next accepted activation. Music keeps a separate manual-intent revision so a manual action supersedes any pending automation pause or resume.

- The preference `Pause if the focus session is paused` pauses Music only when it was playing and records that Focus caused the pause. Resume restarts Music only if that ownership still applies.
- Desktop Focus completion audio attenuates and pauses playing Music, keeping its position and persisted volume. Music resumes after the completion sound only if the same session, track, manual revision, and Focus context still own the pause. A manual control, new track, or changed assignment supersedes it.
- Review playback in the playlist builder defers automatic takeover. Leaving Review lets the owner reconcile eligible automation without treating the deferred activation as consumed.

## Soundscapes

Desktop soundscapes run independently of playlist media, and each phase resolves them separately, so music can continue while the sound changes or vice versa. Automation replaces what is playing with the available sounds of one specified preset; it never silently mixes them into the listener's current preset. A phase or event that chooses no preset stops a soundscape an earlier phase or event started, so a preset never carries over; a soundscape started by hand keeps playing until a phase picks a preset or a stored pause stops it. Keep-current rows behave as choosing nothing. Manual changes during a phase stay temporary and never rewrite project or event assignments. Platforms without the engine omit these actions. See [Soundscapes](soundscapes.md).

## Planned integrations

- Work environments can provide context-specific phase overrides once their model exists.
- A sleep alarm can request a wake-up playlist after dismissal.
- An edge panel can expose transport state without owning automation.

See [Projects settings and scheduling](../projects/settings-and-scheduling.md), [Pomodoro](../pomodoro/README.md), and [Work environments](../work-environments.md).
