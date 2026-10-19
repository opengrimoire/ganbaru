<script lang="ts">
  import CloudHail from "@lucide/svelte/icons/cloud-hail";
  import CloudRain from "@lucide/svelte/icons/cloud-rain";
  import CloudRainWind from "@lucide/svelte/icons/cloud-rain-wind";
  import Pause from "@lucide/svelte/icons/pause";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Play from "@lucide/svelte/icons/play";
  import Plus from "@lucide/svelte/icons/plus";
  import Volume2 from "@lucide/svelte/icons/volume-2";
  import IconPicker from "$lib/components/icon-picker/IconPicker.svelte";
  import { pickSoundscapeFile } from "$lib/api/music";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicSoundscapeDefinition, MusicSoundscapeGroup, MusicSoundscapePreset } from "$lib/music/soundscape/contracts";
  import { orderedGeneratedSounds } from "$lib/music/soundscape/presentation";
  import type { SoundscapeFilter } from "$lib/music/builder/view-state";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import MusicSoundscapeGroupIcon from "./MusicSoundscapeGroupIcon.svelte";
  import MusicSoundscapeLocalRow from "./MusicSoundscapeLocalRow.svelte";
  import MusicSoundscapePresetForm from "./MusicSoundscapePresetForm.svelte";
  import MusicSoundscapeSectionControls from "./MusicSoundscapeSectionControls.svelte";

  const { t } = getLocalization();
  const soundscape = getSoundscapeStore();
  let pendingDelete = $state<MusicSoundscapeDefinition | null>(null);
  let pendingGroupDelete = $state<MusicSoundscapeGroup | null>(null);
  let editingGroupId = $state<string | null>(null);
  let groupNameDraft = $state("");
  let groupIconDraft = $state("lucide:cloud-rain");
  let pendingSound = $state<{ path: string; groupId: string | null } | null>(null);
  let soundNameDraft = $state("");
  let soundIconDraft = $state("lucide:audio-lines");
  let editingPresetId = $state<string | null>(null);
  let pendingPresetDelete = $state<MusicSoundscapePreset | null>(null);
  let {
    filter = "all",
    compact = false,
    addRequest = 0,
    onPlaybackStart = () => undefined,
    onGroupRemoved = () => undefined,
  }: {
    filter?: SoundscapeFilter;
    compact?: boolean;
    addRequest?: number;
    onPlaybackStart?: () => void;
    onGroupRemoved?: () => void;
  } = $props();
  let handledAddRequest = $state(0);

  const generated = $derived(orderedGeneratedSounds(soundscape.definitions));
  const local = $derived(soundscape.definitions.filter((entry) => entry.sourceKind === "local-loop"));
  const ungrouped = $derived(local.filter((entry) => !entry.groupId || !soundscape.groups.some((group) => group.id === entry.groupId)));
  const selectedIds = $derived(soundscape.persisted?.activeIds ?? []);
  const activePresetId = $derived(soundscape.activePreset?.id ?? null);
  const canSavePreset = $derived(selectedIds.length > 0 && activePresetId === null);

  async function addLoop(groupId: string | null = null, replace?: MusicSoundscapeDefinition): Promise<void> {
    try {
      const path = await pickSoundscapeFile();
      if (!path || !soundscape.deviceId) return;
      if (!replace) {
        pendingSound = { path, groupId };
        soundNameDraft = (path.split(/[\\/]/).pop() ?? t("music.soundscape.localLoop")).replace(/\.[^.]+$/, "");
        soundIconDraft = soundscape.groups.find((group) => group.id === groupId)?.icon ?? "lucide:audio-lines";
        return;
      }
      await soundscape.saveDefinition({
        id: replace.id,
        sourceKind: "local-loop",
        generatedKind: null,
        bundledIdentity: null,
        name: replace.name,
        icon: replace.icon,
        groupId: replace.groupId,
        localPath: path,
        expectedVersion: replace.version,
        updatedAtMs: Date.now(),
      });
    } catch (error) {
      console.error("Could not add background sound", error);
    }
  }

  async function saveSound(): Promise<void> {
    const pending = pendingSound;
    const name = soundNameDraft.trim();
    if (!pending || !name) return;
    try {
      await soundscape.saveDefinition({
        id: crypto.randomUUID(),
        sourceKind: "local-loop",
        generatedKind: null,
        bundledIdentity: null,
        name,
        icon: soundIconDraft,
        groupId: pending.groupId,
        localPath: pending.path,
        expectedVersion: null,
        updatedAtMs: Date.now(),
      });
      pendingSound = null;
    } catch (error) {
      console.error("Could not save background sound", error);
    }
  }

  $effect(() => {
    if (addRequest <= handledAddRequest) return;
    handledAddRequest = addRequest;
    const groupId = filter.startsWith("group:") ? filter.slice(6) : null;
    void addLoop(groupId);
  });

  function editGroup(group?: MusicSoundscapeGroup): void {
    editingGroupId = group?.id ?? "new";
    groupNameDraft = group?.name ?? "";
    groupIconDraft = group?.icon ?? "lucide:cloud-rain";
  }

  async function saveGroup(): Promise<void> {
    const name = groupNameDraft.trim();
    if (!name || !editingGroupId) return;
    const current = soundscape.groups.find((entry) => entry.id === editingGroupId);
    try {
      await soundscape.saveGroup({
        id: current?.id ?? crypto.randomUUID(),
        name,
        icon: groupIconDraft,
        expectedVersion: current?.version ?? null,
        updatedAtMs: Date.now(),
      });
      editingGroupId = null;
    } catch (error) {
      console.error("Could not save background sound group", error);
    }
  }

  /** Plays a preset, or pauses it when it is the one already playing. */
  function choosePreset(preset: MusicSoundscapePreset): void {
    if (preset.id === activePresetId && soundscape.snapshot.status === "playing") {
      void soundscape.togglePlayback();
      return;
    }
    onPlaybackStart();
    void soundscape.playPreset(preset);
  }

  function toggleSound(id: string): void {
    if (!selectedIds.includes(id) || soundscape.snapshot.status !== "playing") onPlaybackStart();
    void soundscape.toggleSelection(id);
  }
</script>

<div class="h-full min-h-0 overflow-y-auto px-4 py-4" aria-busy={soundscape.loading || soundscape.saving} data-music-scrollable="true">
  <div class="mb-5 flex flex-wrap items-center gap-x-4 gap-y-2 px-1">
    <div class="flex min-w-36 flex-1 items-center gap-2"><Volume2 size={15} class="shrink-0 text-muted-foreground" /><input type="range" min="0" max="1" step="0.01" value={soundscape.snapshot.volume} disabled={!soundscape.persisted || soundscape.loading} class="min-w-24 flex-1 accent-primary disabled:opacity-40" aria-label={t("music.soundscape.volume")} oninput={(event) => { void soundscape.setVolume(Number(event.currentTarget.value)); }} /><span class="w-9 text-right text-xs tabular-nums text-muted-foreground">{Math.round(soundscape.snapshot.volume * 100)}%</span></div>
    <button type="button" disabled={selectedIds.length === 0 || soundscape.saving} aria-label={soundscape.snapshot.status === "playing" ? t("music.soundscape.pause") : t("music.soundscape.playSelected")} class="grid h-8 w-8 place-items-center rounded-lg text-muted-foreground hover:bg-accent/60 hover:text-foreground disabled:opacity-40" onclick={() => { if (soundscape.snapshot.status !== "playing") onPlaybackStart(); void soundscape.togglePlayback(); }}>{#if soundscape.snapshot.status === "playing"}<Pause size={15} />{:else}<Play size={15} />{/if}</button>
  </div>
  {#if soundscape.error}
    <Notice class="mb-3 px-1 text-xs" message={t("music.soundscape.genericError")} />
  {/if}

  {#if filter === "all"}
    <section class="mb-7 max-w-2xl" aria-labelledby="soundscape-presets">
      <div class="mb-3 flex h-8 items-center gap-2">
        <h2 id="soundscape-presets" class="min-w-0 flex-1 truncate text-xs font-semibold">{t("music.soundscape.choice.presets")}</h2>
        {#if canSavePreset && editingPresetId !== "new"}<button type="button" disabled={soundscape.saving} class="inline-flex h-8 items-center gap-1 rounded-lg px-2 text-xs text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-40" onclick={() => editingPresetId = "new"}><Plus size={14} />{t("music.soundscape.preset.saveCurrent")}</button>{/if}
      </div>
      {#if editingPresetId === "new"}
        <div class="mb-3"><MusicSoundscapePresetForm currentIds={selectedIds} onDone={() => editingPresetId = null} /></div>
      {:else if soundscape.presets.length === 0}
        <p class="px-1 text-xs text-muted-foreground">{t("music.soundscape.preset.empty")}</p>
      {/if}
      <div class="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-2">
        {#each soundscape.presets as preset (preset.id)}
          {#if editingPresetId === preset.id}
            <div class="col-span-full"><MusicSoundscapePresetForm {preset} currentIds={selectedIds} onDone={() => editingPresetId = null} onRemove={(entry) => pendingPresetDelete = entry} /></div>
          {:else}
            {@const selected = preset.id === activePresetId}
            {@const playable = soundscape.presetIsPlayable(preset)}
            <div class={selected ? "flex min-w-0 items-center rounded-xl bg-primary/10" : "flex min-w-0 items-center rounded-xl bg-secondary/40"}>
              <button type="button" aria-pressed={selected} aria-label={selected && soundscape.snapshot.status === "playing" ? t("music.soundscape.pause") : t("music.soundscape.play", preset.name)} disabled={!playable || soundscape.saving} class="flex min-w-0 flex-1 items-center gap-2.5 rounded-xl py-2.5 pl-3 text-left disabled:opacity-50" onclick={() => choosePreset(preset)}>
                <MusicSoundscapeGroupIcon icon={preset.icon} size={18} />
                <span class="flex min-w-0 flex-col">
                  <span class="truncate text-xs font-medium">{preset.name}</span>
                  <span class="truncate text-[0.68rem] text-muted-foreground">{playable ? t("music.soundscape.choice.presetCount", preset.soundscapeIds.length) : t("music.soundscape.needsRepair")}</span>
                </span>
              </button>
              <button type="button" class="mr-1.5 grid h-7 w-7 shrink-0 place-items-center rounded-lg text-muted-foreground hover:bg-accent hover:text-foreground" aria-label={t("music.soundscape.preset.edit", preset.name)} onclick={() => editingPresetId = preset.id}><Pencil size={13} /></button>
            </div>
          {/if}
        {/each}
      </div>
    </section>
  {/if}

  {#if filter === "all" || filter === "generated"}
    <section aria-labelledby="generated-soundscapes">
      <div id="generated-soundscapes" class="mb-3 max-w-2xl"><MusicSoundscapeSectionControls title={t("music.soundscape.generated")} section="generated" idPrefix="builder-soundscape" /></div>
      <div class="grid max-w-2xl grid-cols-3 gap-2">
        {#each generated as definition (definition.id)}
          {@const selected = selectedIds.includes(definition.id)}
          <button type="button" aria-pressed={selected} aria-label={selected && soundscape.persisted?.multipleEnabled ? t("music.soundscape.stop", t(`music.soundscape.rainName.${definition.generatedKind ?? "brown"}`)) : selected && soundscape.snapshot.status === "playing" ? t("music.soundscape.pause") : t("music.soundscape.play", t(`music.soundscape.rainName.${definition.generatedKind ?? "brown"}`))} disabled={soundscape.saving} class={selected ? "flex min-h-28 flex-col items-center justify-center gap-2 rounded-xl bg-primary/10 px-2 text-center hover:bg-primary/15 disabled:opacity-50" : "flex min-h-28 flex-col items-center justify-center gap-2 rounded-xl bg-secondary/40 px-2 text-center hover:bg-secondary/70 disabled:opacity-50"} onclick={() => toggleSound(definition.id)}>
            {#if definition.generatedKind === "brown"}<CloudHail size={27} strokeWidth={1.4} />{:else if definition.generatedKind === "pink"}<CloudRain size={27} strokeWidth={1.4} />{:else}<CloudRainWind size={27} strokeWidth={1.4} />{/if}
            <span class="text-xs font-medium">{t(`music.soundscape.rainName.${definition.generatedKind ?? "brown"}`)}</span>
            <span class="text-[0.68rem] text-muted-foreground">{t(`music.soundscape.generatedName.${definition.generatedKind ?? "brown"}`)}</span>
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if filter !== "generated"}
    <section class:mt-7={filter === "all"} aria-labelledby="local-soundscapes">
      <div class="mb-3 flex items-center gap-2">
        <div id="local-soundscapes" class="min-w-0 flex-1"><MusicSoundscapeSectionControls title={t("music.soundscape.localLoops")} section="local" idPrefix="builder-soundscape" /></div>
        <button type="button" class="inline-flex h-8 items-center gap-1 rounded-lg px-2 text-xs text-muted-foreground hover:bg-accent hover:text-foreground" onclick={() => editGroup()}><Plus size={14} />{t("music.soundscape.newGroup")}</button>
        {#if !compact}<button type="button" class="inline-flex h-8 items-center gap-1 rounded-lg px-2 text-xs text-muted-foreground hover:bg-accent hover:text-foreground" onclick={() => { void addLoop(filter.startsWith("group:") ? filter.slice(6) : null); }}><Plus size={14} />{t("music.soundscape.addLoop")}</button>{/if}
      </div>

      {#if pendingSound}
        <form class="mb-4 max-w-2xl rounded-xl bg-secondary/35 p-3" onsubmit={(event) => { event.preventDefault(); void saveSound(); }}>
          <div class="flex items-center gap-2">
            <IconPicker value={soundIconDraft} onChange={(value) => soundIconDraft = value} ariaLabel={t("music.soundscape.soundIcon")} showUpload={false} showRemove={false}>
              {#snippet trigger({ open, toggle, panelId })}<button type="button" class="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-background text-foreground hover:bg-accent" aria-label={t("music.soundscape.soundIcon")} aria-haspopup="dialog" aria-expanded={open} aria-controls={panelId} onclick={toggle}><MusicSoundscapeGroupIcon icon={soundIconDraft} /></button>{/snippet}
            </IconPicker>
            <input class="field h-9 min-w-0 flex-1 px-3 text-xs" bind:value={soundNameDraft} aria-label={t("music.soundscape.name")} placeholder={t("music.soundscape.name")} maxlength="200" />
          </div>
          <div class="mt-2 flex justify-end gap-2"><button type="button" class="h-8 rounded-lg px-3 text-xs hover:bg-accent" onclick={() => pendingSound = null}>{t("common.cancel")}</button><button type="submit" disabled={!soundNameDraft.trim() || soundscape.saving} class="h-8 rounded-lg bg-primary px-3 text-xs text-primary-foreground disabled:opacity-40">{t("music.soundscape.addLoop")}</button></div>
        </form>
      {/if}

      {#if editingGroupId === "new"}
        <form class="mb-4 rounded-xl bg-secondary/35 p-3" onsubmit={(event) => { event.preventDefault(); void saveGroup(); }}>
          <div class="flex items-center gap-2">
            <IconPicker value={groupIconDraft} onChange={(value) => groupIconDraft = value} ariaLabel={t("music.soundscape.groupIconLabel")} showUpload={false} showRemove={false}>
              {#snippet trigger({ open, toggle, panelId })}<button type="button" class="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-background text-foreground hover:bg-accent" aria-label={t("music.soundscape.groupIconLabel")} aria-haspopup="dialog" aria-expanded={open} aria-controls={panelId} onclick={toggle}><MusicSoundscapeGroupIcon icon={groupIconDraft} /></button>{/snippet}
            </IconPicker>
            <input class="field h-9 min-w-0 flex-1 px-3 text-xs" bind:value={groupNameDraft} aria-label={t("music.soundscape.groupName")} placeholder={t("music.soundscape.groupName")} maxlength="80" />
          </div>
          <div class="mt-2 flex justify-end gap-2"><button type="button" class="h-8 rounded-lg px-3 text-xs hover:bg-accent" onclick={() => editingGroupId = null}>{t("common.cancel")}</button><button type="submit" disabled={!groupNameDraft.trim() || soundscape.saving} class="h-8 rounded-lg bg-primary px-3 text-xs text-primary-foreground disabled:opacity-40">{t("music.soundscape.saveName")}</button></div>
        </form>
      {/if}

      {#each soundscape.groups.filter((entry) => filter === "all" || filter === "local" || filter === `group:${entry.id}`) as group (group.id)}
        <div class="mb-4">
          {#if editingGroupId === group.id}
            <form class="rounded-xl bg-secondary/35 p-3" onsubmit={(event) => { event.preventDefault(); void saveGroup(); }}>
              <div class="flex items-center gap-2">
                <IconPicker value={groupIconDraft} onChange={(value) => groupIconDraft = value} ariaLabel={t("music.soundscape.groupIconLabel")} showUpload={false} showRemove={false}>
                  {#snippet trigger({ open, toggle, panelId })}<button type="button" class="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-background text-foreground hover:bg-accent" aria-label={t("music.soundscape.groupIconLabel")} aria-haspopup="dialog" aria-expanded={open} aria-controls={panelId} onclick={toggle}><MusicSoundscapeGroupIcon icon={groupIconDraft} /></button>{/snippet}
                </IconPicker>
                <input class="field h-9 min-w-0 flex-1 px-3 text-xs" bind:value={groupNameDraft} aria-label={t("music.soundscape.groupName")} maxlength="80" />
              </div>
              <div class="mt-2 flex items-center gap-2"><button type="button" class="h-8 rounded-lg px-2 text-xs text-destructive hover:bg-accent" onclick={() => pendingGroupDelete = group}>{t("music.soundscape.removeGroup")}</button><span class="flex-1"></span><button type="button" class="h-8 rounded-lg px-3 text-xs hover:bg-accent" onclick={() => editingGroupId = null}>{t("common.cancel")}</button><button type="submit" disabled={!groupNameDraft.trim() || soundscape.saving} class="h-8 rounded-lg bg-primary px-3 text-xs text-primary-foreground disabled:opacity-40">{t("music.soundscape.saveName")}</button></div>
            </form>
          {:else}
            <div class="flex h-9 items-center gap-2 px-2"><MusicSoundscapeGroupIcon icon={group.icon} size={16} /><h3 class="min-w-0 flex-1 truncate text-xs font-medium">{group.name}</h3><button type="button" class="grid h-7 w-7 place-items-center rounded-lg text-muted-foreground hover:bg-accent" aria-label={t("music.soundscape.addToGroup", group.name)} onclick={() => { void addLoop(group.id); }}><Plus size={14} /></button><button type="button" class="grid h-7 w-7 place-items-center rounded-lg text-muted-foreground hover:bg-accent" aria-label={t("music.soundscape.editGroup")} onclick={() => editGroup(group)}><Pencil size={13} /></button></div>
            <div class="grid max-w-2xl grid-cols-3 gap-2">
              {#each local.filter((entry) => entry.groupId === group.id) as definition (definition.id)}
                <MusicSoundscapeLocalRow {definition} onRepair={(entry) => { void addLoop(entry.groupId, entry); }} onRemove={(entry) => pendingDelete = entry} {onPlaybackStart} />
              {/each}
            </div>
          {/if}
        </div>
      {/each}

      {#if ungrouped.length > 0 && (filter === "all" || filter === "local")}
        <h3 class="mb-1 px-2 text-[0.68rem] font-medium text-muted-foreground">{t("music.soundscape.ungrouped")}</h3>
        <div class="grid max-w-2xl grid-cols-3 gap-2">{#each ungrouped as definition (definition.id)}<MusicSoundscapeLocalRow {definition} onRepair={(entry) => { void addLoop(null, entry); }} onRemove={(entry) => pendingDelete = entry} {onPlaybackStart} />{/each}</div>
      {:else if soundscape.groups.length === 0 && editingGroupId !== "new"}
        <button type="button" class="flex h-16 w-full items-center justify-center gap-2 rounded-lg text-xs text-muted-foreground hover:bg-accent/40" onclick={() => { void addLoop(); }}><Plus size={16} />{t("music.soundscape.addFirstLoop")}</button>
      {/if}
    </section>
  {/if}
</div>

{#if pendingDelete}
  <ConfirmDialog title={t("music.soundscape.removeTitle")} message={t("music.soundscape.removeMessage", pendingDelete.name)} confirmLabel={t("music.soundscape.remove")} cancelLabel={t("common.cancel")} onConfirm={() => { const definition = pendingDelete; pendingDelete = null; if (definition) void soundscape.removeDefinition(definition).catch((error) => console.error("Could not remove background sound", error)); }} onCancel={() => pendingDelete = null} />
{/if}
{#if pendingPresetDelete}
  <ConfirmDialog title={t("music.soundscape.preset.removeTitle")} message={t("music.soundscape.preset.removeMessage", pendingPresetDelete.name)} confirmLabel={t("music.soundscape.preset.remove")} cancelLabel={t("common.cancel")} onConfirm={() => { const preset = pendingPresetDelete; pendingPresetDelete = null; editingPresetId = null; if (preset) void soundscape.removePreset(preset).catch((error) => console.error("Could not remove background sound preset", error)); }} onCancel={() => pendingPresetDelete = null} />
{/if}
{#if pendingGroupDelete}
  <ConfirmDialog title={t("music.soundscape.removeGroup")} message={t("music.soundscape.removeGroupMessage", pendingGroupDelete.name)} confirmLabel={t("music.soundscape.removeGroup")} cancelLabel={t("common.cancel")} onConfirm={() => { const group = pendingGroupDelete; pendingGroupDelete = null; editingGroupId = null; if (group) void soundscape.removeGroup(group).then(() => { if (filter === `group:${group.id}`) onGroupRemoved(); }).catch((error) => console.error("Could not remove background sound group", error)); }} onCancel={() => pendingGroupDelete = null} />
{/if}
