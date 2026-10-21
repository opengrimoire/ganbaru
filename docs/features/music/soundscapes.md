# Soundscapes

**Implemented on desktop.** A soundscape is the set of non-music sounds playing alongside Music. The UI calls this feature Soundscapes (Paisajes sonoros), its parts sounds, and saved mixes presets; it never calls it background sound, since music plays in the background too. Soundscapes play through a native audio engine independently of Music playback. Platforms without the engine omit these controls.

## Playback

Presets are the only thing that plays, one at a time. The soundscape has no volume of its own: it plays at the Music volume setting times the preset level, so one volume control raises or lowers everything. This applies even while Music is paused or empty, and muting Music silences both. The soundscape control picks a preset and shows its level as a slider from -100% to +100% (a multiplier from 0 to 2, where 0% matches Music); output never exceeds full scale. Adjusting that slider saves into the preset, so the next time it plays it starts at the same level. New presets start at 0%; the seeded rains start at -50% (Windy rain), -25% (Rain), and +25% (Soft rain). Simultaneous sounds share the preset's output to avoid sudden loudness.

Keeping a single volume control and preset-only playback is deliberate: listeners never balance two absolute volumes or rebuild a layered selection by hand.

## Sounds

Three generated noise textures are presented as rain-like sounds from soft to bright: brown, pink, and white noise. Sound and preset tiles show only names; the real noise type stays available as a hint in the builder because these are generated noise, not rain recordings.

Users can add their own local audio loops with a chosen name and icon (using the same icon picker as playlists). Sounds can be organized into named groups or left in Other sounds; a group organizes sounds but never forces them to play together. Original files stay in place. A missing file can be repaired by choosing a new location on the current device, and editing a sound's name, icon, or group never requires the file.

Sounds are managed only in the builder. Pressing a sound there plays it through its one-sound preset, creating that preset the first time, and pressing it again pauses it. Make preset creates the same one-sound preset without playing it.

## Presets

A preset is a named, icon-tagged mix of up to 16 sounds. Each sound has a level from 0 to 100 within the mix, and the preset has its overall level relative to Music. Each generated sound starts with a one-sound preset so the soundscape controls are never empty. Presets are created and edited in the builder's Presets section through a modal editor. While it is open, Music and the soundscape pause and the draft plays as an isolated preview, which can be turned off; the preview never changes what is playing, and saving or leaving brings back exactly what was playing. The playing preset is highlighted in the builder and the soundscape controls. Projects and events choose a preset for any phase.

## Storage

The vault stores sound definitions, groups and membership, presets with their level and ordered sounds with levels, and the active preset with whether it should play. Local file locations stay in device-local app state, never in the vault, so a copied vault shows another device's local sounds as missing until a file is chosen here. Removing a group moves its sounds to Other sounds. Removing a sound never deletes its file and removes it from every preset. A preset left without sounds stays, so phases that use it report it as unavailable instead of silently changing meaning; removing a preset leaves those phases unavailable until another preset is chosen.

## Automation

Phase automation selects one specified preset, replacing what the listener was playing. Automatic and manual intent are tracked separately, and saved automatic intent alone never starts audio at startup. See [Music automation](automation.md).
