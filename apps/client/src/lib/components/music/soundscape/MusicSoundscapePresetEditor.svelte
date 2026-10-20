<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import { onMount } from "svelte";
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
   * Creates or edits a background sound preset by picking its sounds explicitly. While it is open, Music and background
   * sounds pause and the draft plays in an isolated preview; leaving brings back what was playing, unchanged.
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
  let root = $state<HTMLFormElement | null>(null);
  let nameInput = $state<HTMLInputElement | null>(null);
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

  onMount(() => {
    root?.scrollIntoView({ block: "start" });
    nameInput?.focus();
    nameInput?.select();
    // Capture runs before the builder's Escape-to-go-back handler, so Escape cancels the editor instead of leaving the page.
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      const target = event.target instanceof Element ? event.target : null;
      if (target?.closest('[role="dialog"]') || root?.querySelector('[aria-haspopup="dialog"][aria-expanded="true"]')) return;
      event.preventDefault();
      event.stopPropagation();
      onDone(null);
    };
    window.addEventListener("keydown", escape, true);
    return () => window.removeEventListener("keydown", escape, true);
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

  function soundDetail(definition: MusicSoundscapeDefinition): string {
    if (definition.availability !== "available") return t("music.soundscape.needsRepair");
    return definition.generatedKind ? t(`music.soundscape.generatedName.${definition.generatedKind}`) : "";
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
      console.error("Could not save background sound preset", error);
    }
  }
</script>

<form bind:this={root} class="flex flex-col gap-5" aria-labelledby="soundscape-preset-editor" onsubmit={(event) => { event.preventDefault(); void save(); }}>
  <div class="flex h-8 items-center gap-2">
    <h2 id="soundscape-preset-editor" class="min-w-0 flex-1 truncate text-xs font-semibold">{preset ? t("music.soundscape.preset.editTitle") : t("music.soundscape.preset.new")}</h2>
    <button type="button" class="h-8 shrink-0 rounded-lg px-3 text-xs hover:bg-accent" onclick={() => onDone(null)}>{t("common.cancel")}</button>
    <button type="submit" disabled={!canSave} class="h-8 shrink-0 rounded-lg bg-primary px-3 text-xs text-primary-foreground disabled:opacity-40">{t("music.soundscape.preset.save")}</button>
  </div>

  <div class="flex items-center gap-2">
    <IconPicker value={icon} onChange={(value) => icon = value} ariaLabel={t("music.soundscape.preset.icon")} showUpload={false} showRemove={false}>
      {#snippet trigger({ open, toggle, panelId })}<button type="button" class="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-secondary/40 text-foreground hover:bg-accent" aria-label={t("music.soundscape.preset.icon")} aria-haspopup="dialog" aria-expanded={open} aria-controls={panelId} onclick={toggle}><MusicSoundscapeGroupIcon {icon} /></button>{/snippet}
    </IconPicker>
    <input bind:this={nameInput} class="field h-9 min-w-0 flex-1 px-3 text-xs" bind:value={name} aria-label={t("music.soundscape.preset.name")} placeholder={t("music.soundscape.preset.name")} maxlength={MAX_PRESET_NAME_CHARS} />
  </div>

  <section aria-labelledby="soundscape-preset-sounds">
    <div class="flex h-8 items-center gap-2">
      <h3 id="soundscape-preset-sounds" class="min-w-0 flex-1 truncate text-xs font-semibold">{t("music.soundscape.preset.sounds")}</h3>
      <span class="min-w-0 truncate text-xs text-muted-foreground" aria-hidden="true">{t("music.soundscape.preset.listen")}</span>
      <Switch size="compact" checked={listening} ariaLabel={t("music.soundscape.preset.listen")} onChange={(value) => listening = value} />
    </div>
    <p class="text-xs text-muted-foreground">{t("music.soundscape.preset.soundsHint")}</p>
    {#if atLimit}<Notice class="mt-2 text-xs" tone="warning" role="status" copy={false} message={t("music.soundscape.preset.limitReached", MAX_SOUNDSCAPE_LAYERS)} />{/if}

    {#each sections as section (section.key)}
      <div class="mt-4 mb-2 flex items-center gap-1.5 text-[0.68rem] font-medium text-muted-foreground">{#if section.icon}<MusicSoundscapeGroupIcon icon={section.icon} size={13} />{/if}<h4 class="min-w-0 truncate">{section.title}</h4></div>
      <div class="grid grid-cols-2 gap-2 @sm:grid-cols-3">
        {#each section.sounds as definition (definition.id)}
          {@const picked = soundIds.includes(definition.id)}
          {@const detail = soundDetail(definition)}
          <button type="button" aria-pressed={picked} disabled={!picked && atLimit} class={picked ? "relative flex min-h-24 min-w-0 flex-col items-center justify-center gap-1.5 rounded-xl bg-primary/10 px-2 text-center hover:bg-primary/15" : "relative flex min-h-24 min-w-0 flex-col items-center justify-center gap-1.5 rounded-xl bg-secondary/40 px-2 text-center hover:bg-secondary/70 disabled:opacity-40"} onclick={() => soundIds = togglePresetDraftSound(soundIds, definition.id)}>
            {#if picked}<span class="absolute top-2 right-2 grid h-4 w-4 place-items-center rounded-full bg-primary text-primary-foreground"><Check size={11} strokeWidth={2.5} /></span>{/if}
            <MusicSoundscapeSoundIcon {definition} size={24} />
            <span class="max-w-full truncate text-xs font-medium">{soundName(definition)}</span>
            {#if detail}<span class="max-w-full truncate text-[0.68rem] text-muted-foreground">{detail}</span>{/if}
          </button>
        {/each}
      </div>
    {/each}
  </section>

  {#if preset && onRemove}
    <div><button type="button" class="h-8 rounded-lg px-2 text-xs text-destructive hover:bg-accent" onclick={() => onRemove(preset)}>{t("music.soundscape.preset.remove")}</button></div>
  {/if}
</form>
