# Background sounds

**Implemented on desktop.** Background sounds play through a native audio engine independently of Music playback. Platforms without the engine omit these controls.

## Playback

One sound at a time is the default; layered mode lets listeners toggle up to 16 sounds individually. An overall volume changes every sound, and each section has a relative adjustment from -100% to +100% (a multiplier from 0 to 2, with 0% leaving the level unchanged). Simultaneous layers are normalized to avoid sudden loudness. The first-use overall volume is 10%; later changes are retained.

## Sounds

Three generated noise textures are presented as rain-like sounds from soft to bright: brown, pink, and white noise. Sound and preset tiles show only names; the real noise type stays available as a hint in the builder because these are generated noise, not rain recordings.

Users can add their own local audio loops with a chosen name and icon (using the same icon picker as playlists). Sounds can be organized into named groups or left in Other sounds; a group organizes sounds but never forces them to play together. Original files stay in place. A missing file can be repaired by choosing a new location on the current device, and editing a sound's name, icon, or group never requires the file.

## Presets

A preset is a named set of up to 16 sounds with an icon. Presets store only which sounds play, not volumes; overall and section levels stay global. Presets are created and edited only in the builder's Presets section, by picking their sounds explicitly in a modal editor. While it is open, Music and background sounds pause and the draft plays as an isolated preview, which can be turned off; the preview never becomes the saved selection, and saving or leaving brings back exactly what was playing. Presets play from the builder and the background sound controls, and the preset that matches the current selection is highlighted. Playing a preset starts its available sounds together and turns on layered mode when it has more than one sound. Projects and events can choose a preset for any phase.

## Storage

The vault stores sound definitions, groups and membership, presets and their ordered sounds, overall and section volumes, ordered playback selections, and the layering preference. Local file locations are device-specific. Removing a group moves its sounds to Other sounds. Removing a sound never deletes its file and removes it from the current selection and from every preset. A preset left without sounds stays, so phases that use it report it as unavailable instead of silently changing meaning; removing a preset leaves those phases unavailable until another sound is chosen.

## Automation

Phase automation selects one specified sound or preset, replacing the listener's selection, and never silently adds a layer to it. Automatic and manual intent are tracked separately, and saved automatic intent alone never starts audio at startup. See [Music automation](automation.md).
