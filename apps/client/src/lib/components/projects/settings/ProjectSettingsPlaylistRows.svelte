<script lang="ts">
  import MusicPlaylistSelect from "$lib/components/music/MusicPlaylistSelect.svelte";
  import MusicSoundSelect from "$lib/components/music/soundscape/MusicSoundSelect.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { behaviorUsesPlaylist, completeMusicAssignmentDrafts, updateMusicAssignmentDraft } from "$lib/music/assignment-draft";
  import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
  import type { MusicActivityPhase, MusicContextAssignmentDraft } from "$lib/music/context-assignment";
  import { soundChoiceFields, soundChoiceFromAssignment, type MusicSoundChoice } from "$lib/music/sound-choice";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";

  let {
    assignments,
    playlists,
    onChange,
    disabled = false,
    loadingPlaylists = false,
  }: {
    assignments: readonly MusicContextAssignmentDraft[];
    playlists: readonly MusicPlaylistSummary[];
    onChange: (assignments: MusicContextAssignmentDraft[]) => void;
    disabled?: boolean;
    loadingPlaylists?: boolean;
  } = $props();

  const { t } = getLocalization();
  const showSounds = platformHasCapability(BUILD_PLATFORM_PROFILE, "music.soundscapes");
  const phaseNames = $derived({
    focus: t("music.assignment.phase.focus"),
    "short-break": t("music.assignment.phase.short-break"),
    "long-break": t("music.assignment.phase.long-break"),
  });
  const playlistLabels = $derived({
    focus: t("projects.settings.focusPlaylist"),
    "short-break": t("projects.settings.shortBreakPlaylist"),
    "long-break": t("projects.settings.longBreakPlaylist"),
  });
  const soundLabels = $derived({
    focus: t("projects.settings.focusSound"),
    "short-break": t("projects.settings.shortBreakSound"),
    "long-break": t("projects.settings.longBreakSound"),
  });
  const phaseAssignments = $derived(completeMusicAssignmentDrafts(assignments));

  /** Stage playlist phase choices, with automatic playback or silence, keeping each phase's soundscape. */
  function selectPlaylist(phase: MusicActivityPhase, playlistId: string | null): void {
    if (disabled || loadingPlaylists) return;
    onChange(phaseAssignments.map((assignment): MusicContextAssignmentDraft => {
      const selectedId = assignment.phase === phase
        ? playlistId
        : behaviorUsesPlaylist(assignment.behavior) ? assignment.playlistId : null;
      return {
        ...assignment,
        playlistId: selectedId,
        behavior: selectedId ? "play-automatically" : "pause-music",
      };
    }));
  }

  /** Stage one phase's soundscape without touching its music. */
  function selectSound(phase: MusicActivityPhase, choice: MusicSoundChoice): void {
    if (disabled) return;
    onChange(updateMusicAssignmentDraft(phaseAssignments, phase, soundChoiceFields(choice)));
  }
</script>

<div class="phase-music">
  <div class={showSounds ? "phase-music-grid with-sounds" : "phase-music-grid"}>
    {#each phaseAssignments as assignment (assignment.phase)}
      <span class="phase-music-label min-w-0 truncate text-[0.866667rem] text-foreground">{showSounds ? phaseNames[assignment.phase] : playlistLabels[assignment.phase]}</span>
      <MusicPlaylistSelect
        label={playlistLabels[assignment.phase]}
        value={behaviorUsesPlaylist(assignment.behavior) ? assignment.playlistId : null}
        {playlists}
        onChange={(playlistId) => selectPlaylist(assignment.phase, playlistId)}
        {disabled}
        loading={loadingPlaylists}
        emptyLabel={t("common.none")}
      />
      {#if showSounds}
        <MusicSoundSelect
          label={soundLabels[assignment.phase]}
          value={soundChoiceFromAssignment(assignment)}
          onChange={(choice) => selectSound(assignment.phase, choice)}
          {disabled}
        />
      {/if}
    {/each}
  </div>
</div>

<style>
  .phase-music { container-type: inline-size; }

  .phase-music-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 11rem;
    align-items: center;
    gap: 0.5rem 1rem;
    padding-inline: 0.25rem;
  }

  .phase-music-grid.with-sounds {
    grid-template-columns: minmax(0, 1fr) 11rem 11rem;
    column-gap: 0.5rem;
  }

  @container (width < 32rem) {
    .phase-music-grid.with-sounds { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .with-sounds .phase-music-label { grid-column: 1 / -1; margin-top: 0.25rem; }
  }

  @container (width < 20rem) {
    .phase-music-grid:not(.with-sounds) { grid-template-columns: minmax(0, 1fr); }
  }
</style>
