<script lang="ts">
  import { onMount } from "svelte";
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Music from "@lucide/svelte/icons/eighth-notes";
  import MusicPlaylistSelect from "$lib/components/music/MusicPlaylistSelect.svelte";
  import MusicSoundSelect from "$lib/components/music/soundscape/MusicSoundSelect.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    chooseEventPlaylist,
    chooseEventSound,
    effectiveEventPlaylistId,
    effectiveEventSound,
    eventMusicPhases,
    eventSoundPlays,
    restoreEventMusic,
    silenceEventMusic,
    snapshotHasEventMusic,
  } from "$lib/music/event-playlists";
  import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
  import type { MusicActivityPhase, MusicContextAssignmentDraft } from "$lib/music/context-assignment";
  import { systemMusicPlaylistName } from "$lib/music/playlists/system";
  import type { MusicSoundChoice } from "$lib/music/sound-choice";
  import { presetDisplayName } from "$lib/music/soundscape/presets";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";

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
  const soundscape = getSoundscapeStore();
  const showSounds = platformHasCapability(BUILD_PLATFORM_PROFILE, "music.soundscapes");
  const triggerClass = "rounded border-0 bg-black/5 font-normal text-event-panel-input-text hover:bg-black/10 disabled:hover:bg-black/5 dark:bg-black/15 dark:hover:bg-black/25 dark:disabled:hover:bg-black/15";
  const phases = $derived(eventMusicPhases(pomodoroEnabled));
  /** Pomodoro events with soundscapes show one row per phase with a playlist and a sound column. */
  const phaseColumns = $derived(showSounds && pomodoroEnabled);
  const rows = $derived(phases.map((phase) => {
    const sound = effectiveEventSound(overrides, snapshots, phase);
    const playlistId = effectiveEventPlaylistId(overrides, snapshots, phase);
    return {
      phase,
      label: pomodoroEnabled ? t(`music.assignment.phase.${phase}`) : t("calendar.eventPanel.musicPlaylist"),
      playlistId,
      sound,
      plays: playlistId !== null || (showSounds && eventSoundPlays(sound)),
    };
  }));
  const enabled = $derived(rows.some((row) => row.plays));
  /** Names the playlist and sound without Pomodoro, or which phases play something with it. */
  const summary = $derived.by(() => {
    if (!enabled) return "";
    if (!pomodoroEnabled) {
      const row = rows[0];
      const playlist = playlists.find((entry) => entry.id === row?.playlistId);
      const parts = [
        row?.playlistId ? (playlist ? systemMusicPlaylistName(playlist.id, playlist.name, t) : t("music.assignment.missingPlaylist")) : null,
        showSounds && row && eventSoundPlays(row.sound) ? soundName(row.sound) : null,
      ];
      return parts.filter((part) => part !== null).join(" · ");
    }
    const plays = (phase: MusicActivityPhase) => rows.some((row) => row.phase === phase && row.plays);
    return t("calendar.eventPanel.musicPhases", plays("focus"), plays("short-break"), plays("long-break"));
  });

  onMount(() => {
    if (showSounds) void soundscape.ensureCatalog();
  });

  /** Name of the preset a phase plays, for the collapsed summary. */
  function soundName(choice: MusicSoundChoice): string {
    if (choice.kind !== "preset") return "";
    const preset = soundscape.presets.find((entry) => entry.id === choice.presetId);
    return preset ? presetDisplayName(preset, t) : t("music.soundscape.choice.missing");
  }

  /** Silence the event, or bring back the project music and sounds when the event plays nothing. */
  function toggle(): void {
    if (disabled || loading) return;
    if (enabled) {
      onOverridesChange(silenceEventMusic(overrides, snapshots, phases));
    } else if (snapshotHasEventMusic(snapshots, phases)) {
      onOverridesChange(restoreEventMusic(overrides, phases));
    } else if (!expanded) {
      onExpand();
    }
  }

  function selectPlaylist(phase: MusicActivityPhase, playlistId: string | null): void {
    if (disabled || loading) return;
    onOverridesChange(chooseEventPlaylist(overrides, snapshots, phase, playlistId));
  }

  function selectSound(phase: MusicActivityPhase, choice: MusicSoundChoice): void {
    if (disabled || loading) return;
    onOverridesChange(chooseEventSound(overrides, snapshots, phase, choice));
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
    <div transition:slide={{ duration: 180, easing: cubicOut }} data-section="music" class="music-body py-2.5 pl-1" style="background-color: var(--panel-bg);">
      <div class={phaseColumns ? "music-grid phase-columns" : "music-grid"}>
        {#if error}
          <Notice class="col-span-full px-1 text-[0.65rem]" message={error} {onRetry} />
        {/if}
        {#each rows as row (row.phase)}
          <span class="music-label truncate text-[0.8rem] text-foreground">{row.label}</span>
          <MusicPlaylistSelect
            label={phaseColumns ? t("calendar.eventPanel.musicPhasePlaylist", row.label) : row.label}
            value={row.playlistId}
            {playlists}
            onChange={(playlistId) => selectPlaylist(row.phase, playlistId)}
            {disabled}
            {loading}
            emptyLabel={t("common.none")}
            {triggerClass}
          />
          {#if phaseColumns}
            <MusicSoundSelect
              label={t("calendar.eventPanel.musicPhaseSound", row.label)}
              value={row.sound}
              onChange={(choice) => selectSound(row.phase, choice)}
              disabled={disabled || loading}
              {triggerClass}
            />
          {/if}
        {/each}
        {#if showSounds && !pomodoroEnabled && rows[0]}
          {@const row = rows[0]}
          <span class="truncate text-[0.8rem] text-foreground">{t("calendar.eventPanel.musicSound")}</span>
          <MusicSoundSelect
            label={t("calendar.eventPanel.musicSound")}
            value={row.sound}
            onChange={(choice) => selectSound(row.phase, choice)}
            disabled={disabled || loading}
            {triggerClass}
          />
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .music-body { container-type: inline-size; }

  .music-grid {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    gap: 0.375rem 1.25rem;
  }

  .music-grid.phase-columns {
    grid-template-columns: auto minmax(0, 1fr) minmax(0, 1fr);
    column-gap: 0.5rem;
  }

  @container (width < 24rem) {
    .music-grid.phase-columns { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .phase-columns .music-label { grid-column: 1 / -1; margin-top: 0.25rem; }
  }
</style>
