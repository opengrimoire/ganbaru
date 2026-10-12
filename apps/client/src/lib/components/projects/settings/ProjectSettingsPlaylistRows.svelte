<script lang="ts">
  import MusicPlaylistSelect from "$lib/components/music/MusicPlaylistSelect.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { behaviorUsesPlaylist, completeMusicAssignmentDrafts } from "$lib/music/assignment-draft";
  import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
  import type { MusicActivityPhase, MusicContextAssignmentDraft } from "$lib/music/context-assignment";

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
  const phaseLabels = $derived({
    focus: t("projects.settings.focusPlaylist"),
    "short-break": t("projects.settings.shortBreakPlaylist"),
    "long-break": t("projects.settings.longBreakPlaylist"),
  });
  const phaseAssignments = $derived(completeMusicAssignmentDrafts(assignments));

  /** Stage playlist-only phase choices, with automatic playback or silence. */
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
        soundscapeId: null,
        soundscapeBehavior: "inherit",
      };
    }));
  }
</script>

{#each phaseAssignments as assignment (assignment.phase)}
  <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
    <span class="min-w-0 flex-1 text-[0.866667rem] text-foreground">{phaseLabels[assignment.phase]}</span>
    <MusicPlaylistSelect
      label={phaseLabels[assignment.phase]}
      value={behaviorUsesPlaylist(assignment.behavior) ? assignment.playlistId : null}
      {playlists}
      onChange={(playlistId) => selectPlaylist(assignment.phase, playlistId)}
      {disabled}
      loading={loadingPlaylists}
      emptyLabel={t("common.none")}
      class="w-44 shrink-0 max-[480px]:w-full"
    />
  </div>
{/each}
