<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import { onMount } from "svelte";
  import MusicBuilderDialog from "$lib/components/music/builder/MusicBuilderDialog.svelte";
  import IconPicker from "$lib/components/icon-picker/IconPicker.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Switch from "$lib/components/ui/Switch.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { MAX_SOUNDSCAPE_LAYERS, type MusicSoundscapeDefinition, type MusicSoundscapePreset } from "$lib/music/soundscape/contracts";
  import { orderedGeneratedSounds } from "$lib/music/soundscape/presentation";
  import { togglePresetDraftSound } from "$lib/music/soundscape/presets";
  import { getMusicPlayer } from "$lib/stores/music-player.svelte";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import MusicSoundscapeGroupIcon from "./MusicSoundscapeGroupIcon.svelte";
  import MusicSoundscapeSoundIcon from "./MusicSoundscapeSoundIcon.svelte";

  const DEFAULT_PRESET_ICON = "lucide:layers";
  /** Mirrors the native preset name limit so the field never accepts a name the store rejects. */
  const MAX_PRESET_NAME_CHARS = 80;

  /**
   * Creates or edits a soundscape preset by picking its sounds explicitly. While it is open, Music and the soundscape
   * pause and the draft plays in an isolated preview; leaving brings back what was playing, unchanged.
   */
  let {
    preset = null,
    onDone,
    onRemove,
  }: {
    preset?: MusicSoundscapePreset | null;
    onDone: (preset: MusicSoundscapePreset | null) => void;
    onRemove?: (preset: MusicSoundscapePreset) => void;
  } = $props();

  const { t } = getLocalization();
  const soundscape = getSoundscapeStore();
  const player = getMusicPlayer();
  // Drafts start from the preset once; later store refreshes must not overwrite what the person is choosing.
  let name = $state(initialName());
  let icon = $state(initialIcon());
  let soundIds = $state<string[]>(initialSoundIds());
  const atLimit = $derived(soundIds.length >= MAX_SOUNDSCAPE_LAYERS);
  const canSave = $derived(name.trim().length > 0 && soundIds.length > 0 && !soundscape.saving);
  let listening = $state(true);
  const sections = $derived(soundSections(soundscape.definitions));

  onMount(() => {
    const resumesMusic = player.isPlaying;
    if (resumesMusic) void player.pausePlayback("system");
    void soundscape.beginPreview();
    return () => {
      void soundscape.endPreview();
      if (resumesMusic && !player.isPlaying) void player.playPlayback("system");
    };
  });

  $effect(() => {
    void soundscape.previewSounds(listening ? soundIds : []);
  });

  function initialName(): string {
    return preset?.name ?? "";
  }

  function initialIcon(): string {
    return preset?.icon ?? DEFAULT_PRESET_ICON;
  }

  function initialSoundIds(): string[] {
    return preset ? [...preset.soundscapeIds] : [];
  }

  function soundSections(definitions: readonly MusicSoundscapeDefinition[]): { key: string; title: string; icon: string | null; sounds: MusicSoundscapeDefinition[] }[] {
    const local = definitions.filter((entry) => entry.sourceKind === "local-loop");
    const grouped = soundscape.groups.map((group) => ({ key: group.id, title: group.name, icon: group.icon, sounds: local.filter((entry) => entry.groupId === group.id) }));
    const ungrouped = local.filter((entry) => !entry.groupId || !soundscape.groups.some((group) => group.id === entry.groupId));
    return [
      { key: "generated", title: t("music.soundscape.generated"), icon: null, sounds: orderedGeneratedSounds(definitions) },
      ...grouped,
      { key: "ungrouped", title: t("music.soundscape.ungrouped"), icon: null, sounds: ungrouped },
    ].filter((section) => section.sounds.length > 0);
  }

  function soundName(definition: MusicSoundscapeDefinition): string {
    return definition.generatedKind ? t(`music.soundscape.rainName.${definition.generatedKind}`) : definition.name;
  }

  /** Tile tooltip: the repair state when the file is missing, otherwise the real noise type of a generated sound. */
  function soundHint(definition: MusicSoundscapeDefinition): string | undefined {
    if (definition.availability !== "available") return t("music.soundscape.needsRepair");
    return definition.generatedKind ? t(`music.soundscape.generatedName.${definition.generatedKind}`) : undefined;
  }


  async function save(): Promise<void> {
    if (!canSave) return;
    try {
      const saved = await soundscape.savePreset({
        id: preset?.id ?? crypto.randomUUID(),
        name: name.trim(),
        icon,
        soundscapeIds: [...soundIds],
        expectedVersion: preset?.version ?? null,
        updatedAtMs: Date.now(),
      });
      onDone(saved);
    } catch (error) {
      console.error("Could not save soundscape preset", error);
    }
  }
</script>

<MusicBuilderDialog
  title={preset ? t("music.soundscape.preset.editTitle") : t("music.soundscape.preset.new")}
  titleId="soundscape-preset-editor"
  description={t("music.soundscape.preset.soundsHint")}
  size="large"
  dismissDisabled={soundscape.saving}
  onDismiss={() => onDone(null)}
>
  <div class="@container flex flex-col gap-5">
    <div class="flex items-center gap-2">
      <IconPicker value={icon} onChange={(value) => icon = value} ariaLabel={t("music.soundscape.preset.icon")} showUpload={false} showRemove={false}>
        {#snippet trigger({ open, toggle, panelId })}<button type="button" class="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-secondary/40 text-foreground hover:bg-accent" aria-label={t("music.soundscape.preset.icon")} aria-haspopup="dialog" aria-expanded={open} aria-controls={panelId} onclick={toggle}><MusicSoundscapeGroupIcon {icon} /></button>{/snippet}
      </IconPicker>
      <input data-dialog-autofocus class="field h-9 min-w-0 flex-1 px-3 text-xs" bind:value={name} aria-label={t("music.soundscape.preset.name")} placeholder={t("music.soundscape.preset.name")} maxlength={MAX_PRESET_NAME_CHARS} onkeydown={(event) => { if (event.key === "Enter") { event.preventDefault(); void save(); } }} />
    </div>

    <section aria-labelledby="soundscape-preset-sounds">
      <div class="flex h-8 items-center gap-2">
        <h3 id="soundscape-preset-sounds" class="min-w-0 flex-1 truncate text-xs font-semibold">{t("music.soundscape.preset.sounds")}</h3>
        <span class="min-w-0 truncate text-xs text-muted-foreground" aria-hidden="true">{t("music.soundscape.preset.listen")}</span>
        <Switch size="compact" checked={listening} ariaLabel={t("music.soundscape.preset.listen")} onChange={(value) => listening = value} />
      </div>
      {#if atLimit}<Notice class="mt-2 text-xs" tone="warning" role="status" copy={false} message={t("music.soundscape.preset.limitReached", MAX_SOUNDSCAPE_LAYERS)} />{/if}

      {#each sections as section (section.key)}
        <div class="mt-4 mb-2 flex items-center gap-1.5 text-[0.68rem] font-medium text-muted-foreground">{#if section.icon}<MusicSoundscapeGroupIcon icon={section.icon} size={13} />{/if}<h4 class="min-w-0 truncate">{section.title}</h4></div>
        <div class="grid grid-cols-2 gap-2 @sm:grid-cols-3">
          {#each section.sounds as definition (definition.id)}
            {@const picked = soundIds.includes(definition.id)}
            <button type="button" aria-pressed={picked} disabled={!picked && atLimit} title={soundHint(definition)} class={`relative flex min-h-24 min-w-0 flex-col items-center justify-center gap-1.5 rounded-xl px-2 text-center ${picked ? "bg-primary/10 hover:bg-primary/15" : "bg-secondary/40 hover:bg-secondary/70 disabled:opacity-40"} ${definition.availability !== "available" ? "text-muted-foreground" : ""}`} onclick={() => soundIds = togglePresetDraftSound(soundIds, definition.id)}>
              {#if picked}<span class="absolute top-2 right-2 grid h-4 w-4 place-items-center rounded-full bg-primary text-primary-foreground"><Check size={11} strokeWidth={2.5} /></span>{/if}
              <MusicSoundscapeSoundIcon {definition} size={24} />
              <span class="max-w-full truncate text-xs font-medium">{soundName(definition)}</span>
            </button>
          {/each}
        </div>
      {/each}
    </section>
  </div>

  {#snippet footer()}
    <button type="button" onclick={() => onDone(null)} disabled={soundscape.saving} class="min-h-10 rounded-md border border-border bg-card px-3.5 py-2 text-sm font-medium hover:bg-accent disabled:opacity-50">{t("common.cancel")}</button>
    <button type="button" data-preset-save onclick={() => { void save(); }} disabled={!canSave} class="min-h-10 rounded-md border border-border bg-primary px-3.5 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50">{t("music.soundscape.preset.save")}</button>
    {#if preset && onRemove}<button type="button" onclick={() => onRemove(preset)} disabled={soundscape.saving} class="ml-auto min-h-10 rounded-md px-3.5 py-2 text-sm font-medium text-destructive hover:bg-accent disabled:opacity-50">{t("music.soundscape.preset.remove")}</button>{/if}
  {/snippet}
</MusicBuilderDialog>
