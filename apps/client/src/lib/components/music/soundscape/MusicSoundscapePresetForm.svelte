<script lang="ts">
  import { onMount } from "svelte";
  import IconPicker from "$lib/components/icon-picker/IconPicker.svelte";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicSoundscapePreset } from "$lib/music/soundscape/contracts";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import MusicSoundscapeGroupIcon from "./MusicSoundscapeGroupIcon.svelte";

  const DEFAULT_PRESET_ICON = "lucide:layers";
  /** Mirrors the native preset name limit so the field never accepts a name the store rejects. */
  const MAX_PRESET_NAME_CHARS = 80;

  /**
   * Names a background sound preset. A new preset stores the sounds playing now; an existing one keeps its sounds unless
   * the person chooses to replace them with the current selection.
   */
  let {
    preset = null,
    currentIds,
    nested = false,
    compact = false,
    onDone,
    onRemove,
  }: {
    preset?: MusicSoundscapePreset | null;
    currentIds: readonly string[];
    /** Keeps the icon picker inside the nearest floating root so an enclosing popover stays open. */
    nested?: boolean;
    compact?: boolean;
    onDone: (preset: MusicSoundscapePreset | null) => void;
    onRemove?: (preset: MusicSoundscapePreset) => void;
  } = $props();

  const { t } = getLocalization();
  const soundscape = getSoundscapeStore();
  let nameInput = $state<HTMLInputElement | null>(null);
  // Drafts start from the preset once; later store refreshes must not overwrite what the person is typing.
  let name = $state(initialName());
  let icon = $state(initialIcon());
  let useCurrent = $state(false);
  const canUseCurrent = $derived(preset !== null && currentIds.length > 0 && !sameSounds(preset.soundscapeIds, currentIds));
  const soundscapeIds = $derived(preset && !(useCurrent && canUseCurrent) ? preset.soundscapeIds : [...currentIds]);
  const canSave = $derived(name.trim().length > 0 && soundscapeIds.length > 0 && !soundscape.saving);

  onMount(() => {
    nameInput?.focus();
    nameInput?.select();
  });

  function initialName(): string {
    return preset?.name ?? "";
  }

  function initialIcon(): string {
    return preset?.icon ?? DEFAULT_PRESET_ICON;
  }

  function sameSounds(left: readonly string[], right: readonly string[]): boolean {
    return left.length === right.length && left.every((id) => right.includes(id));
  }

  function cancelWithEscape(event: KeyboardEvent): void {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    event.preventDefault();
    event.stopPropagation();
    onDone(null);
  }

  async function save(): Promise<void> {
    if (!canSave) return;
    try {
      const saved = await soundscape.savePreset({
        id: preset?.id ?? crypto.randomUUID(),
        name: name.trim(),
        icon,
        soundscapeIds: [...soundscapeIds],
        expectedVersion: preset?.version ?? null,
        updatedAtMs: Date.now(),
      });
      onDone(saved);
    } catch (error) {
      console.error("Could not save background sound preset", error);
    }
  }
</script>

<form
  class={compact ? "rounded-floating-item bg-secondary/40 p-2" : "rounded-xl bg-secondary/35 p-3"}
  onsubmit={(event) => { event.preventDefault(); void save(); }}
>
  <div class="flex items-center gap-2">
    <IconPicker value={icon} onChange={(value) => icon = value} ariaLabel={t("music.soundscape.preset.icon")} showUpload={false} showRemove={false} {nested}>
      {#snippet trigger({ open, toggle, panelId })}<button type="button" class="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-background text-foreground hover:bg-accent" aria-label={t("music.soundscape.preset.icon")} aria-haspopup="dialog" aria-expanded={open} aria-controls={panelId} onclick={toggle}><MusicSoundscapeGroupIcon {icon} /></button>{/snippet}
    </IconPicker>
    <input bind:this={nameInput} class="field h-9 min-w-0 flex-1 px-3 text-xs" bind:value={name} aria-label={t("music.soundscape.preset.name")} placeholder={t("music.soundscape.preset.name")} maxlength={MAX_PRESET_NAME_CHARS} onkeydown={cancelWithEscape} />
  </div>
  {#if canUseCurrent}
    <label class="mt-2 flex items-center gap-2 px-1 text-xs text-muted-foreground"><Checkbox bind:checked={useCurrent} />{t("music.soundscape.preset.useCurrent")}</label>
  {/if}
  <div class="mt-2 flex flex-wrap items-center justify-end gap-2">
    {#if preset && onRemove}
      <button type="button" class="mr-auto h-8 rounded-lg px-2 text-xs text-destructive hover:bg-accent" onclick={() => onRemove(preset)}>{t("music.soundscape.preset.remove")}</button>
    {/if}
    <button type="button" class="h-8 rounded-lg px-3 text-xs hover:bg-accent" onclick={() => onDone(null)}>{t("common.cancel")}</button>
    <button type="submit" disabled={!canSave} class="h-8 rounded-lg bg-primary px-3 text-xs text-primary-foreground disabled:opacity-40">{t("music.soundscape.preset.save")}</button>
  </div>
</form>
