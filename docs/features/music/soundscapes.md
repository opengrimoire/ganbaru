# Background sounds

**Implemented on desktop.** Background sounds play through a native audio engine independently of Music playback. Platforms without the engine omit these controls.

## Playback

One sound at a time is the default; layered mode lets listeners toggle up to 16 sounds individually. An overall volume changes every sound, and each section has a relative adjustment from -100% to +100% (a multiplier from 0 to 2, with 0% leaving the level unchanged). Simultaneous layers are normalized to avoid sudden loudness. The first-use overall volume is 10%; later changes are retained.

## Sounds

Three generated noise textures are presented as rain-like sounds from soft to bright: brown, pink, and white noise. Their real noise types stay visible in the builder because they are generated noise, not rain recordings.

Users can add their own local audio loops with a chosen name and icon (using the same icon picker as playlists). Sounds can be organized into named groups or left in Other sounds; a group organizes sounds but never forces them to play together. Original files stay in place. A missing file can be repaired by choosing a new location on the current device, and editing a sound's name, icon, or group never requires the file.

## Storage

The vault stores sound definitions, groups and membership, overall and section volumes, ordered playback selections, and the layering preference. Local file locations are device-specific. Removing a group moves its sounds to Other sounds. Removing a sound never deletes its file and removes it from the current selection.

## Automation

Phase automation selects one specified sound and never silently adds a layer to the listener's selection. Automatic and manual intent are tracked separately, and saved automatic intent alone never starts audio at startup. See [Music automation](automation.md).
