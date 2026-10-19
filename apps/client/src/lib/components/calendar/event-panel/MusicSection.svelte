<script lang="ts">
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Music from "@lucide/svelte/icons/music";
  import MusicPlaylistSelect from "$lib/components/music/MusicPlaylistSelect.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    chooseEventPlaylist,
    effectiveEventPlaylistId,
    eventMusicPhases,
    restoreEventPlaylists,
    silenceEventPlaylists,
    snapshotHasEventPlaylist,
  } from "$lib/music/event-playlists";
  import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
  import type { MusicActivityPhase, MusicContextAssignmentDraft } from "$lib/music/context-assignment";
  import { systemMusicPlaylistName } from "$lib/music/playlists/system";

  let {
    overrides,
    snapshots,
    playlists,
    pomodoroEnabled,
    expanded,
    disabled = false,
    loading = false,
    error = null,
    onOverridesChange,
    onExpand,
    onRetry,
  }: {
    overrides: readonly MusicContextAssignmentDraft[];
    snapshots: readonly MusicContextAssignmentDraft[];
    playlists: readonly MusicPlaylistSummary[];
    pomodoroEnabled: boolean;
    expanded: boolean;
    disabled?: boolean;
    loading?: boolean;
    error?: string | null;
    onOverridesChange: (overrides: MusicContextAssignmentDraft[]) => void;
    onExpand: () => void;
    onRetry: () => void;
  } = $props();

  const { t } = getLocalization();
  const phases = $derived(eventMusicPhases(pomodoroEnabled));
  const rows = $derived(phases.map((phase) => ({
    phase,
    label: pomodoroEnabled ? t(`music.assignment.phase.${phase}`) : t("calendar.eventPanel.musicPlaylist"),
    playlistId: effectiveEventPlaylistId(overrides, snapshots, phase),
  })));
  const enabled = $derived(rows.some((row) => row.playlistId !== null));
  /** Names the single playlist without Pomodoro, or which phases play music with it. */
  const summary = $derived.by(() => {
    if (!enabled) return "";
    if (!pomodoroEnabled) {
      const playlist = playlists.find((entry) => entry.id === rows[0]?.playlistId);
      return playlist ? systemMusicPlaylistName(playlist.id, playlist.name, t) : t("music.assignment.missingPlaylist");
    }
    const plays = (phase: MusicActivityPhase) => rows.some((row) => row.phase === phase && row.playlistId !== null);
    return t("calendar.eventPanel.musicPhases", plays("focus"), plays("short-break"), plays("long-break"));
  });

  /** Silence the event, or bring back the project playlists when the event has none. */
  function toggle(): void {
    if (disabled || loading) return;
    if (enabled) {
      onOverridesChange(silenceEventPlaylists(overrides, snapshots, phases));
    } else if (snapshotHasEventPlaylist(snapshots, phases)) {
      onOverridesChange(restoreEventPlaylists(overrides, phases));
    } else if (!expanded) {
      onExpand();
    }
  }

  function select(phase: MusicActivityPhase, playlistId: string | null): void {
    if (disabled || loading) return;
    onOverridesChange(chooseEventPlaylist(overrides, snapshots, phase, playlistId));
  }
</script>

<div class="flex flex-col overflow-hidden rounded-none" style="background-color: var(--panel-contrast);">
  <div class="section-header flex items-stretch">
    <button onclick={toggle}
      aria-label={t("calendar.eventPanel.music")}
      aria-pressed={enabled}
      class="flex w-10 shrink-0 items-center justify-center
        {enabled ? 'bg-black/3 text-foreground dark:bg-black/30' : 'text-muted-foreground/50'}">
      <Music size={14} />
    </button>
    <button onclick={onExpand}
      aria-expanded={expanded}
      class="flex min-w-0 flex-1 items-center gap-2.5 px-3 py-2 text-left">
      <span class="translate-y-[1.13px] text-[0.8rem] {enabled ? 'text-foreground' : 'text-muted-foreground'}">{t("calendar.eventPanel.music")}</span>
      <span class="ml-auto min-w-0 translate-y-[1.13px] truncate text-[0.733333rem] text-muted-foreground">{summary}</span>
    </button>
  </div>
  {#if expanded}
    <div transition:slide={{ duration: 180, easing: cubicOut }} data-section="music" class="grid grid-cols-[auto_minmax(0,1fr)] items-center gap-x-3 gap-y-1.5 py-2.5 pl-1" style="background-color: var(--panel-bg);">
      {#if error}
        <Notice class="col-span-2 px-1 text-[0.65rem]" message={error} {onRetry} />
      {/if}
      {#each rows as row (row.phase)}
        <span class="truncate text-[0.8rem] text-foreground">{row.label}</span>
        <MusicPlaylistSelect
          label={row.label}
          value={row.playlistId}
          {playlists}
          onChange={(playlistId) => select(row.phase, playlistId)}
          {disabled}
          {loading}
          emptyLabel={t("common.none")}
          triggerClass="rounded border-0 bg-black/5 font-normal text-event-panel-input-text hover:bg-black/10 disabled:hover:bg-black/5 dark:bg-black/15 dark:hover:bg-black/25 dark:disabled:hover:bg-black/15"
        />
      {/each}
    </div>
  {/if}
</div>
