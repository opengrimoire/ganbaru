<script lang="ts">
  import Pause from "@lucide/svelte/icons/pause";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Play from "@lucide/svelte/icons/play";
  import Plus from "@lucide/svelte/icons/plus";
  import Volume2 from "@lucide/svelte/icons/volume-2";
  import { untrack } from "svelte";
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
  import MusicSoundscapePresetEditor from "./MusicSoundscapePresetEditor.svelte";
  import MusicSoundscapePresetTile from "./MusicSoundscapePresetTile.svelte";
  import MusicSoundscapeSectionControls from "./MusicSoundscapeSectionControls.svelte";
  import MusicSoundscapeSoundIcon from "./MusicSoundscapeSoundIcon.svelte";

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
  /** Preset being created (`preset: null`) or edited; the editor replaces the builder content while open. */
  let presetEditor = $state<{ preset: MusicSoundscapePreset | null } | null>(null);
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
  const showPresets = $derived(filter === "all" || filter === "presets");
  const showGenerated = $derived(filter === "all" || filter === "generated");
  const showLocal = $derived(filter !== "generated" && filter !== "presets");

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
    presetEditor = null;
    const groupId = filter.startsWith("group:") ? filter.slice(6) : null;
    void addLoop(groupId);
  });

  // Choosing another section in the sidebar leaves the preset editor.
  $effect(() => {
    void filter;
    untrack(() => presetEditor = null);
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

  function toggleSound(id: string): void {
    if (!selectedIds.includes(id) || soundscape.snapshot.status !== "playing") onPlaybackStart();
    void soundscape.toggleSelection(id);
  }
</script>

<div class="h-full min-h-0 overflow-y-auto px-4 py-4" aria-busy={soundscape.loading || soundscape.saving} data-music-scrollable="true">
  <div class="@container flex max-w-2xl flex-col gap-7">
    {#if presetEditor}
      {#key presetEditor}
        <MusicSoundscapePresetEditor preset={presetEditor.preset} onDone={() => presetEditor = null} onRemove={(entry) => pendingPresetDelete = entry} />
      {/key}
    {:else}
      <div class="flex flex-col gap-3">
        <div class="flex h-8 items-center gap-2">
          <Volume2 size={15} class="shrink-0 text-muted-foreground" />
          <input type="range" min="0" max="1" step="0.01" value={soundscape.snapshot.volume} disabled={!soundscape.persisted || soundscape.loading} class="min-w-24 flex-1 accent-primary disabled:opacity-40" aria-label={t("music.soundscape.volume")} oninput={(event) => { void soundscape.setVolume(Number(event.currentTarget.value)); }} />
          <span class="w-9 text-right text-xs tabular-nums text-muted-foreground">{Math.round(soundscape.snapshot.volume * 100)}%</span>
          <button type="button" disabled={selectedIds.length === 0 || soundscape.saving} aria-label={soundscape.snapshot.status === "playing" ? t("music.soundscape.pause") : t("music.soundscape.playSelected")} class="grid h-8 w-8 shrink-0 place-items-center rounded-lg text-muted-foreground hover:bg-accent/60 hover:text-foreground disabled:opacity-40" onclick={() => { if (soundscape.snapshot.status !== "playing") onPlaybackStart(); void soundscape.togglePlayback(); }}>{#if soundscape.snapshot.status === "playing"}<Pause size={15} />{:else}<Play size={15} />{/if}</button>
        </div>
        {#if soundscape.error}
          <Notice class="text-xs" message={t("music.soundscape.genericError")} />
        {/if}
      </div>

      {#if showPresets}
        <section aria-labelledby="soundscape-presets">
          <div class="mb-2 flex h-8 items-center gap-2">
            <h2 id="soundscape-presets" class="min-w-0 flex-1 truncate text-xs font-semibold">{t("music.soundscape.choice.presets")}</h2>
          </div>
          <div class="grid grid-cols-2 gap-2 @sm:grid-cols-3">
            {#each soundscape.presets as preset (preset.id)}
              <MusicSoundscapePresetTile {preset} {onPlaybackStart} onEdit={(entry) => presetEditor = { preset: entry }} onRemove={(entry) => pendingPresetDelete = entry} />
            {/each}
            <button type="button" disabled={!soundscape.persisted || soundscape.saving} class="flex min-h-28 min-w-0 flex-col items-center justify-center gap-2 rounded-xl px-3 text-center text-muted-foreground hover:bg-secondary/40 hover:text-foreground disabled:opacity-40" onclick={() => presetEditor = { preset: null }}>
              <Plus size={22} strokeWidth={1.5} />
              <span class="max-w-full truncate text-xs font-medium">{t("music.soundscape.preset.new")}</span>
            </button>
          </div>
        </section>
      {/if}

      {#if showGenerated}
        <section aria-labelledby="generated-soundscapes">
          <div id="generated-soundscapes" class="mb-2 flex items-center gap-2"><MusicSoundscapeSectionControls title={t("music.soundscape.generated")} section="generated" idPrefix="builder-soundscape" /></div>
          <div class="grid grid-cols-2 gap-2 @sm:grid-cols-3">
            {#each generated as definition (definition.id)}
              {@const selected = selectedIds.includes(definition.id)}
              <button type="button" aria-pressed={selected} aria-label={selected && soundscape.persisted?.multipleEnabled ? t("music.soundscape.stop", t(`music.soundscape.rainName.${definition.generatedKind ?? "brown"}`)) : selected && soundscape.snapshot.status === "playing" ? t("music.soundscape.pause") : t("music.soundscape.play", t(`music.soundscape.rainName.${definition.generatedKind ?? "brown"}`))} disabled={soundscape.saving} class={selected ? "flex min-h-28 min-w-0 flex-col items-center justify-center gap-2 rounded-xl bg-primary/10 px-3 text-center hover:bg-primary/15 disabled:opacity-50" : "flex min-h-28 min-w-0 flex-col items-center justify-center gap-2 rounded-xl bg-secondary/40 px-3 text-center hover:bg-secondary/70 disabled:opacity-50"} onclick={() => toggleSound(definition.id)}>
                <MusicSoundscapeSoundIcon {definition} size={27} />
                <span class="max-w-full truncate text-xs font-medium">{t(`music.soundscape.rainName.${definition.generatedKind ?? "brown"}`)}</span>
                <span class="max-w-full truncate text-[0.68rem] text-muted-foreground">{t(`music.soundscape.generatedName.${definition.generatedKind ?? "brown"}`)}</span>
              </button>
            {/each}
          </div>
        </section>
      {/if}

      {#if showLocal}
        <section class="flex flex-col gap-4" aria-labelledby="local-soundscapes">
          <div class="-mb-2 flex items-center gap-2">
            <div id="local-soundscapes" class="flex min-w-0 flex-1"><MusicSoundscapeSectionControls title={t("music.soundscape.localLoops")} section="local" idPrefix="builder-soundscape" /></div>
            <button type="button" class="inline-flex h-8 shrink-0 items-center gap-1 rounded-lg px-2 text-xs text-muted-foreground hover:bg-accent hover:text-foreground" onclick={() => editGroup()}><Plus size={14} />{t("music.soundscape.newGroup")}</button>
            {#if !compact}<button type="button" class="inline-flex h-8 shrink-0 items-center gap-1 rounded-lg px-2 text-xs text-muted-foreground hover:bg-accent hover:text-foreground" onclick={() => { void addLoop(filter.startsWith("group:") ? filter.slice(6) : null); }}><Plus size={14} />{t("music.soundscape.addLoop")}</button>{/if}
          </div>

          {#if pendingSound}
            <form class="rounded-xl bg-secondary/35 p-3" onsubmit={(event) => { event.preventDefault(); void saveSound(); }}>
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
            <form class="rounded-xl bg-secondary/35 p-3" onsubmit={(event) => { event.preventDefault(); void saveGroup(); }}>
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
            <div>
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
                {@const sounds = local.filter((entry) => entry.groupId === group.id)}
                <div class="mb-1 flex h-8 items-center gap-1.5 text-muted-foreground"><MusicSoundscapeGroupIcon icon={group.icon} size={13} /><h3 class="min-w-0 flex-1 truncate text-[0.68rem] font-medium">{group.name}</h3><button type="button" class="grid h-7 w-7 shrink-0 place-items-center rounded-lg hover:bg-accent hover:text-foreground" aria-label={t("music.soundscape.addToGroup", group.name)} onclick={() => { void addLoop(group.id); }}><Plus size={14} /></button><button type="button" class="grid h-7 w-7 shrink-0 place-items-center rounded-lg hover:bg-accent hover:text-foreground" aria-label={t("music.soundscape.editGroup")} onclick={() => editGroup(group)}><Pencil size={13} /></button></div>
                <div class="grid grid-cols-2 gap-2 @sm:grid-cols-3">
                  {#each sounds as definition (definition.id)}
                    <MusicSoundscapeLocalRow {definition} onRepair={(entry) => { void addLoop(entry.groupId, entry); }} onRemove={(entry) => pendingDelete = entry} {onPlaybackStart} />
                  {/each}
                  {#if sounds.length === 0}{@render addSoundTile(group.id)}{/if}
                </div>
              {/if}
            </div>
          {/each}

          {#if ungrouped.length > 0 && (filter === "all" || filter === "local")}
            <div>
              <h3 class="mb-1 flex h-8 items-center text-[0.68rem] font-medium text-muted-foreground">{t("music.soundscape.ungrouped")}</h3>
              <div class="grid grid-cols-2 gap-2 @sm:grid-cols-3">{#each ungrouped as definition (definition.id)}<MusicSoundscapeLocalRow {definition} onRepair={(entry) => { void addLoop(null, entry); }} onRemove={(entry) => pendingDelete = entry} {onPlaybackStart} />{/each}</div>
            </div>
          {:else if soundscape.groups.length === 0 && editingGroupId !== "new" && !pendingSound}
            <div class="grid grid-cols-2 gap-2 @sm:grid-cols-3">{@render addSoundTile(null)}</div>
          {/if}
        </section>
      {/if}
    {/if}
  </div>
</div>

{#snippet addSoundTile(groupId: string | null)}
  <button type="button" class="flex min-h-28 min-w-0 flex-col items-center justify-center gap-2 rounded-xl px-3 text-center text-muted-foreground hover:bg-secondary/40 hover:text-foreground" onclick={() => { void addLoop(groupId); }}>
    <Plus size={22} strokeWidth={1.5} />
    <span class="max-w-full truncate text-xs font-medium">{t("music.soundscape.addFirstLoop")}</span>
  </button>
{/snippet}

{#if pendingDelete}
  <ConfirmDialog title={t("music.soundscape.removeTitle")} message={t("music.soundscape.removeMessage", pendingDelete.name)} confirmLabel={t("music.soundscape.remove")} cancelLabel={t("common.cancel")} onConfirm={() => { const definition = pendingDelete; pendingDelete = null; if (definition) void soundscape.removeDefinition(definition).catch((error) => console.error("Could not remove background sound", error)); }} onCancel={() => pendingDelete = null} />
{/if}
{#if pendingPresetDelete}
  <ConfirmDialog title={t("music.soundscape.preset.removeTitle")} message={t("music.soundscape.preset.removeMessage", pendingPresetDelete.name)} confirmLabel={t("music.soundscape.preset.remove")} cancelLabel={t("common.cancel")} onConfirm={() => { const preset = pendingPresetDelete; pendingPresetDelete = null; if (presetEditor?.preset?.id === preset?.id) presetEditor = null; if (preset) void soundscape.removePreset(preset).catch((error) => console.error("Could not remove background sound preset", error)); }} onCancel={() => pendingPresetDelete = null} />
{/if}
{#if pendingGroupDelete}
  <ConfirmDialog title={t("music.soundscape.removeGroup")} message={t("music.soundscape.removeGroupMessage", pendingGroupDelete.name)} confirmLabel={t("music.soundscape.removeGroup")} cancelLabel={t("common.cancel")} onConfirm={() => { const group = pendingGroupDelete; pendingGroupDelete = null; editingGroupId = null; if (group) void soundscape.removeGroup(group).then(() => { if (filter === `group:${group.id}`) onGroupRemoved(); }).catch((error) => console.error("Could not remove background sound group", error)); }} onCancel={() => pendingGroupDelete = null} />
{/if}
