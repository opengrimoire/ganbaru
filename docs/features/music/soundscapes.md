# Soundscapes

**Implemented on desktop.** A soundscape is a named mix of non-music layers playing alongside Music. The UI uses only two words: Soundscapes (Paisajes sonoros) for the mixes that play and Layers (Capas) for their parts. It never calls them background sound, since music plays in the background too, and it has no separate preset, sound, or group concepts. Soundscapes play through a native audio engine independently of Music playback. Platforms without the engine omit these controls.

## Playback

Soundscapes are the only thing that plays, one at a time. A soundscape has no volume of its own: it plays at the Music volume setting times the soundscape level, so one volume control raises or lowers everything. This applies even while Music is paused or empty, and muting Music silences both. The soundscape control picks a soundscape and shows its level as a slider from -100% to +100% (a multiplier from 0 to 2, where 0% matches Music); output never exceeds full scale. Adjusting that slider saves into the soundscape, so the next time it plays it starts at the same level. Like Music, a soundscape never resumes on its own after an app launch or vault switch; the last soundscape stays selected until it is played again. New soundscapes start at 0%; the seeded rains start at -50% (Rain (heavy)), -25% (Rain (moderate)), and +25% (Rain (soft)). Simultaneous layers share the soundscape's output to avoid sudden loudness.

The preference `Pause if music is paused` (on by default) links the soundscape to Music pauses in one direction. Any Music pause pauses a playing soundscape, and resuming Music resumes it only if that pause paused it; a soundscape the listener paused stays paused. Pausing or resuming the soundscape alone never affects Music. A soundscape paused with Music is not recovered as interrupted output. Queue end, errors, and stopping Music are not pauses.

Keeping a single volume control and soundscape-only playback is deliberate: listeners never balance two absolute volumes or rebuild a layered selection by hand.

## Layers

Three generated layers are named for what they are: Noise (brown), Noise (pink), and Noise (white), so they stay together wherever layers are sorted by name. The seeded rain soundscapes give them their rain character, from soft (brown) to heavy (white).

Users can add their own local audio loops as layers with a chosen name and icon (using the same icon picker as playlists). Original files stay in place. A missing file can be repaired by choosing a new location on the current device, and editing a layer's name or icon never requires the file.

Layers are managed only in the builder, in one list with generated layers first. Pressing a layer there plays it on its own, outside every soundscape, and pressing it again stops it. A layer playing on its own replaces the playing soundscape, which stays chosen but silent; it is never saved, so it does not come back after a restart, and pausing it stops it. Layers and soundscapes are highlighted independently, and the soundscape level is in standby while a layer plays on its own. A layer never becomes a soundscape; soundscapes are made only with New soundscape.

## Mixing

A soundscape mixes up to 16 layers. Each layer has a level from 0 to 100 within the mix, and the soundscape has its overall level relative to Music. Each generated layer starts with a one-layer rain soundscape so the soundscape controls are never empty. Soundscapes are created and edited in the builder through a modal editor, which asks before discarding unsaved changes. While it is open, Music and the soundscape pause and the draft plays as an isolated preview, which can be turned off. Choosing or removing a layer in the draft never restarts the layers it keeps: added layers fade in and removed ones fade out while the rest settle at their new share. The preview never changes what is playing, and saving or leaving brings back exactly what was playing. The playing soundscape is highlighted in the builder and the soundscape controls; while the preview plays, nothing behind the editor shows as playing. Projects and events choose a soundscape for any phase.

## Storage

The vault stores layer definitions, soundscapes with their level and ordered layers with levels, and the active soundscape with whether it should play. Internal names call soundscapes presets and layers soundscape definitions. Local file locations stay in device-local app state, never in the vault, so a copied vault, or a linked device, shows another device's local layers as missing until a file is chosen here. Soundscapes, their layers, and levels replicate between linked devices; concurrent edits to one soundscape can merge to more than 16 layers, which are all kept and editable while the first 16 available in order play, and no edit adds a layer past 16, and deleting a layer on one device removes it from every soundscape everywhere. Removing a layer never deletes its file and removes it from every soundscape. A soundscape left without layers stays, so phases that use it report it as unavailable instead of silently changing meaning; removing a soundscape leaves those phases unavailable until another one is chosen.

## Automation

Phase automation selects one specified soundscape, replacing what the listener was playing. Automatic and manual intent are tracked separately, and saved automatic intent alone never starts audio at startup. See [Music automation](automation.md).
