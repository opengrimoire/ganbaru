<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatPresetLevelAdjustment, PRESET_LEVEL_ADJUSTMENT_RANGE } from "$lib/music/soundscape/presets";
  import MusicSoundscapeLevelSlider from "./MusicSoundscapeLevelSlider.svelte";

  /** Preset level relative to the Music volume: a title with the current value, then the slider between its labeled ends. */
  let {
    id,
    title,
    label,
    adjustment,
    disabled = false,
    onchange,
  }: {
    id: string;
    title: string;
    /** Accessible name of the slider. */
    label: string;
    adjustment: number;
    disabled?: boolean;
    onchange: (adjustment: number) => void;
  } = $props();
  const localization = getLocalization();
  const { t } = localization;
  const format = (value: number) => formatPresetLevelAdjustment(localization.locale, value);
  const adjustmentText = $derived(format(adjustment));
</script>

<div class="flex items-baseline gap-2 text-panel-detail font-medium text-muted-foreground">
  <p class="min-w-0 flex-1 truncate">{title}</p>
  {#if !disabled}<span class="shrink-0 tabular-nums">{adjustmentText}</span>{/if}
</div>
<div class="flex min-h-(--panel-row-height) items-center gap-2">
  <span class="shrink-0 text-panel-detail tabular-nums text-muted-foreground" aria-hidden="true">{format(PRESET_LEVEL_ADJUSTMENT_RANGE.min)}</span>
  <MusicSoundscapeLevelSlider {id} {label} value={adjustment} min={PRESET_LEVEL_ADJUSTMENT_RANGE.min} max={PRESET_LEVEL_ADJUSTMENT_RANGE.max} centered {disabled} valueText={t("music.soundscape.levelValue", adjustmentText)} {onchange} />
  <span class="shrink-0 text-panel-detail tabular-nums text-muted-foreground" aria-hidden="true">{format(PRESET_LEVEL_ADJUSTMENT_RANGE.max)}</span>
</div>
