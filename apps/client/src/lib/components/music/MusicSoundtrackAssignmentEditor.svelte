<script lang="ts">
  import CirclePause from "@lucide/svelte/icons/circle-pause";
  import Headphones from "@lucide/svelte/icons/headphones";
  import ListMusic from "@lucide/svelte/icons/list-music";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import VolumeX from "@lucide/svelte/icons/volume-x";
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    MUSIC_ACTIVITY_PHASES,
    behaviorUsesPlaylist,
    completeMusicAssignmentDrafts,
    nextMusicAssignmentPhase,
    updateMusicAssignmentDraft,
  } from "$lib/music/assignment-draft";
  import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
  import { systemMusicPlaylistName } from "$lib/music/playlists/system";
  import type { MusicSoundscapeDefinition } from "$lib/music/soundscape/contracts";
  import type {
    MusicActivityPhase,
    MusicAssignmentBehavior,
    MusicContextAssignmentDraft,
    MusicSoundscapeBehavior,
  } from "$lib/music/context-assignment";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import { cn } from "$lib/utils";
  import MusicPlaylistSelect from "./MusicPlaylistSelect.svelte";

  export interface MusicSoundscapeOption {
    id: string;
    name: string;
  }

  let {
    assignments,
    playlists,
    onChange,
    inheritedAssignments = [],
    soundscapes = [],
    disabled = false,
    loadingPlaylists = false,
    title,
    description,
  }: {
    assignments: readonly MusicContextAssignmentDraft[];
    playlists: readonly MusicPlaylistSummary[];
    onChange: (assignments: MusicContextAssignmentDraft[]) => void;
    inheritedAssignments?: readonly MusicContextAssignmentDraft[];
    soundscapes?: readonly MusicSoundscapeOption[];
    disabled?: boolean;
    loadingPlaylists?: boolean;
    title?: string;
    description?: string;
  } = $props();

  const { t } = getLocalization();
  const soundscapeStore = getSoundscapeStore();
  const MUSIC_ASSIGNMENT_BEHAVIORS: readonly MusicAssignmentBehavior[] = [
    "inherit",
    "play-automatically",
    "prepare-silently",
    "pause-music",
    "keep-current-music",
  ];
  const SOUNDSCAPE_BEHAVIORS: readonly MusicSoundscapeBehavior[] = [
    "inherit",
    "play-selected",
    "pause-soundscape",
    "keep-current-soundscape",
  ];
  let activePhase = $state<MusicActivityPhase>("focus");
  const completeAssignments = $derived(completeMusicAssignmentDrafts(assignments));
  const completeInherited = $derived(completeMusicAssignmentDrafts(inheritedAssignments));
  const behaviorOptions = $derived(MUSIC_ASSIGNMENT_BEHAVIORS.map((behavior) => ({
    value: behavior,
    label: t(`music.assignment.behavior.${behavior}`),
    summary: t(`music.assignment.behaviorSummary.${behavior}`),
  })));
  const availableSoundscapes = $derived(soundscapes.length > 0 ? soundscapes : soundscapeStore.definitions);
  const soundscapeOptions = $derived([
    { value: "none", label: t("music.assignment.noSoundscape") },
    ...availableSoundscapes.map((soundscape) => ({ value: soundscape.id, label: soundscapeName(soundscape) })),
  ]);
  const soundscapeBehaviorOptions = $derived(SOUNDSCAPE_BEHAVIORS.map((behavior) => ({
    value: behavior,
    label: t(`music.assignment.soundscapeBehavior.${behavior}`),
    summary: t(`music.assignment.soundscapeBehaviorSummary.${behavior}`),
  })));

  function setBehavior(phase: MusicActivityPhase, value: string): void {
    if (!MUSIC_ASSIGNMENT_BEHAVIORS.includes(value as MusicAssignmentBehavior)) return;
    onChange(updateMusicAssignmentDraft(assignments, phase, { behavior: value as MusicAssignmentBehavior }));
  }

  function soundscapeName(soundscape: MusicSoundscapeOption | MusicSoundscapeDefinition): string {
    return "generatedKind" in soundscape && soundscape.generatedKind
      ? t(`music.soundscape.rainName.${soundscape.generatedKind}`)
      : soundscape.name;
  }

  function setPlaylist(phase: MusicActivityPhase, playlistId: string | null): void {
    onChange(updateMusicAssignmentDraft(assignments, phase, { playlistId }));
  }

  function setSoundscape(phase: MusicActivityPhase, value: string): void {
    onChange(updateMusicAssignmentDraft(assignments, phase, { soundscapeId: value === "none" ? null : value }));
  }

  function setSoundscapeBehavior(phase: MusicActivityPhase, value: string): void {
    if (!SOUNDSCAPE_BEHAVIORS.includes(value as MusicSoundscapeBehavior)) return;
    onChange(updateMusicAssignmentDraft(assignments, phase, { soundscapeBehavior: value as MusicSoundscapeBehavior }));
  }

  function phaseIcon(phase: MusicActivityPhase) {
    if (phase === "focus") return Headphones;
    if (phase === "short-break") return Sparkles;
    return CirclePause;
  }

  function inheritedSummary(phase: MusicActivityPhase): string {
    const inherited = completeInherited.find((entry) => entry.phase === phase);
    if (!inherited || inherited.behavior === "inherit") return t("music.assignment.noInheritedValue");
    const playlist = inherited.playlistId ? playlists.find((entry) => entry.id === inherited.playlistId) : null;
    return playlist
      ? `${t(`music.assignment.behavior.${inherited.behavior}`)} · ${systemMusicPlaylistName(playlist.id, playlist.name, t)}`
      : t(`music.assignment.behavior.${inherited.behavior}`);
  }

  function handlePhaseKeydown(event: KeyboardEvent, phase: MusicActivityPhase): void {
    if (!MUSIC_ACTIVITY_PHASES.includes(phase)) return;
    const key = event.key;
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(key)) return;
    event.preventDefault();
    activePhase = nextMusicAssignmentPhase(phase, key as import("$lib/music/assignment-draft").MusicPhaseNavigationKey);
    queueMicrotask(() => document.querySelector<HTMLButtonElement>(`[data-music-phase-tab="${activePhase}"]`)?.focus());
  }
</script>

<section class="music-assignment-editor min-w-0" aria-label={title ?? t("music.assignment.title")}>
  {#if title || description}
    <header class="mb-3">
      {#if title}<h3 class="text-sm font-semibold text-foreground">{title}</h3>{/if}
      {#if description}<p class="mt-1 max-w-2xl text-[0.72rem] leading-relaxed text-muted-foreground">{description}</p>{/if}
    </header>
  {/if}

  <div class="phase-tabs mb-2 rounded-xl bg-secondary/55 p-1" role="tablist" aria-label={t("music.assignment.choosePhase")}>
    {#each MUSIC_ACTIVITY_PHASES as phase}
      <button data-music-phase-tab={phase} id={`music-phase-tab-${phase}`} type="button" role="tab" aria-selected={activePhase === phase} aria-controls={`music-phase-panel-${phase}`} tabindex={activePhase === phase ? 0 : -1} onclick={() => { activePhase = phase; }} onkeydown={(event) => handlePhaseKeydown(event, phase)} class={cn("min-w-0 flex-1 rounded-lg px-2 py-1.5 text-[0.68rem] font-medium transition-colors", activePhase === phase ? "bg-background text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground")}>
        <span class="block truncate">{t(`music.assignment.phase.${phase}`)}</span>
      </button>
    {/each}
  </div>

  <div class="phase-grid grid min-w-0 grid-cols-3 gap-2">
    {#each completeAssignments as assignment (assignment.phase)}
      {@const Icon = phaseIcon(assignment.phase)}
      <div id={`music-phase-panel-${assignment.phase}`} role="tabpanel" aria-labelledby={`music-phase-tab-${assignment.phase}`} class:hidden-phase={activePhase !== assignment.phase} class="phase-card min-w-0 rounded-xl border border-border/65 bg-card/55 p-2.5 shadow-sm">
        <div class="mb-2 flex min-w-0 items-center gap-2">
          <span class="grid h-7 w-7 shrink-0 place-items-center rounded-lg bg-primary/10 text-primary"><Icon size={14} /></span>
          <div class="min-w-0 flex-1"><h4 class="truncate text-xs font-semibold">{t(`music.assignment.phase.${assignment.phase}`)}</h4><p class="truncate text-[0.62rem] text-muted-foreground">{t(`music.assignment.phaseSummary.${assignment.phase}`)}</p></div>
        </div>

        <div class="space-y-2">
          <Select
            label={t("music.assignment.behaviorLabel")}
            value={assignment.behavior}
            options={behaviorOptions}
            onChange={(value) => setBehavior(assignment.phase, value)}
            {disabled}
            class="w-full"
          />

          {#if assignment.behavior === "inherit"}
            <div class="rounded-lg bg-secondary/55 px-2.5 py-2 text-[0.66rem] leading-relaxed text-muted-foreground">
              <span class="mb-0.5 block font-medium text-foreground/80">{t("music.assignment.inheritedFromContext")}</span>
              {inheritedSummary(assignment.phase)}
            </div>
          {:else if behaviorUsesPlaylist(assignment.behavior)}
            <MusicPlaylistSelect
              value={assignment.playlistId}
              {playlists}
              onChange={(playlistId) => setPlaylist(assignment.phase, playlistId)}
              label={t("music.assignment.playlistForPhase", t(`music.assignment.phase.${assignment.phase}`))}
              {disabled}
              loading={loadingPlaylists}
            />
          {:else}
            <div class="flex min-h-8 items-center gap-2 rounded-lg bg-secondary/45 px-2.5 text-[0.66rem] text-muted-foreground">
              {#if assignment.behavior === "pause-music"}<VolumeX size={13} />{:else}<ListMusic size={13} />{/if}
              <span>{t(`music.assignment.behaviorSummary.${assignment.behavior}`)}</span>
            </div>
          {/if}

          <Select label={t("music.assignment.soundscapeBehaviorLabel")} value={assignment.soundscapeBehavior} options={soundscapeBehaviorOptions} onChange={(value) => setSoundscapeBehavior(assignment.phase, value)} {disabled} class="w-full" />
          {#if assignment.soundscapeBehavior === "play-selected"}
            <Select label={t("music.assignment.soundscapeLabel")} value={assignment.soundscapeId ?? "none"} options={soundscapeOptions} onChange={(value) => setSoundscape(assignment.phase, value)} {disabled} class="w-full" />
          {/if}
        </div>
      </div>
    {/each}
  </div>
</section>

<style>
  .music-assignment-editor {
    container-type: inline-size;
  }

  .phase-tabs {
    display: none;
  }

  @container (max-width: 650px) {
    .phase-tabs {
      display: flex;
    }

    .phase-grid {
      display: block;
    }

    .phase-card.hidden-phase {
      display: none;
    }
  }
</style>
