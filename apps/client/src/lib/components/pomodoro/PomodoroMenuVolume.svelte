<script lang="ts" module>
  /** Volume change per slider step in the Focus menu. */
  export const POMODORO_MENU_VOLUME_STEP = 0.05;
</script>

<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /**
   * The Focus menu's volume row: a label and a compact slider whose fill shows the volume. Without `onchange` the row
   * only depicts a volume and stays out of the tab order.
   */
  let {
    value,
    max,
    onchange,
  }: {
    value: number;
    max: number;
    onchange?: (value: number) => void;
  } = $props();

  const { t } = getLocalization();
  const progress = $derived(max > 0 ? `${Math.min(100, Math.max(0, (value / max) * 100))}%` : "0%");
</script>

<div class="flex min-h-(--panel-row-height) items-center gap-3 px-2 text-foreground">
  <span class="shrink-0">{t("titleBar.music.volume")}</span>
  <input
    type="range"
    min="0"
    {max}
    step={POMODORO_MENU_VOLUME_STEP}
    {value}
    class="pomodoro-menu-volume-slider min-w-0 flex-1"
    style={`--pomodoro-menu-volume-progress: ${progress};`}
    aria-label={t("titleBar.music.volumeLabel")}
    tabindex={onchange ? undefined : -1}
    oninput={(event) => { onchange?.(Number(event.currentTarget.value)); }}
  />
</div>

<style>
  .pomodoro-menu-volume-slider {
    --pomodoro-menu-volume-thumb-size: 0.5rem;
    --pomodoro-menu-volume-track-height: 0.125rem;
    --pomodoro-menu-volume-track-color: color-mix(in srgb, var(--foreground) 18%, transparent);
    --pomodoro-menu-volume-fill-color: color-mix(in srgb, var(--foreground) 58%, transparent);
    height: var(--pomodoro-menu-volume-thumb-size);
    appearance: none;
    cursor: pointer;
    background: linear-gradient(
      to right,
      var(--pomodoro-menu-volume-fill-color) 0%,
      var(--pomodoro-menu-volume-fill-color) var(--pomodoro-menu-volume-progress),
      var(--pomodoro-menu-volume-track-color) var(--pomodoro-menu-volume-progress),
      var(--pomodoro-menu-volume-track-color) 100%
    ) center / calc(100% - var(--pomodoro-menu-volume-thumb-size)) var(--pomodoro-menu-volume-track-height) no-repeat;
  }

  .pomodoro-menu-volume-slider::-webkit-slider-runnable-track {
    height: var(--pomodoro-menu-volume-track-height);
    border-radius: 999px;
    background: transparent;
  }

  .pomodoro-menu-volume-slider::-webkit-slider-thumb {
    width: var(--pomodoro-menu-volume-thumb-size);
    height: var(--pomodoro-menu-volume-thumb-size);
    margin-top: calc((var(--pomodoro-menu-volume-track-height) - var(--pomodoro-menu-volume-thumb-size)) / 2);
    appearance: none;
    border: 0;
    border-radius: 999px;
    background: var(--foreground);
  }

  .pomodoro-menu-volume-slider::-moz-range-track,
  .pomodoro-menu-volume-slider::-moz-range-progress {
    height: var(--pomodoro-menu-volume-track-height);
    border-radius: 999px;
    background: transparent;
  }

  .pomodoro-menu-volume-slider::-moz-range-thumb {
    width: var(--pomodoro-menu-volume-thumb-size);
    height: var(--pomodoro-menu-volume-thumb-size);
    border: 0;
    border-radius: 999px;
    background: var(--foreground);
  }
</style>
