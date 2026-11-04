<script lang="ts">
  /**
   * Soundscape level slider. A centered slider fills from its midpoint, for levels relative to the Music volume;
   * otherwise it fills from the start, for a sound's share of its preset.
   */
  let {
    id,
    label,
    value,
    min,
    max,
    step = 5,
    centered = false,
    disabled = false,
    valueText,
    describedBy,
    onchange,
  }: {
    id: string;
    label: string;
    value: number;
    min: number;
    max: number;
    step?: number;
    centered?: boolean;
    disabled?: boolean;
    valueText: string;
    describedBy?: string;
    onchange: (value: number) => void;
  } = $props();
  const position = $derived(max > min ? ((Math.min(max, Math.max(min, value)) - min) / (max - min)) * 100 : 0);
  const start = $derived(centered ? Math.min(50, position) : 0);
  const end = $derived(centered ? Math.max(50, position) : position);
</script>

<input
  {id}
  type="range"
  {min}
  {max}
  {step}
  {value}
  {disabled}
  aria-label={label}
  aria-valuetext={valueText}
  aria-describedby={describedBy}
  class="soundscape-level-slider min-w-8 flex-1 disabled:opacity-40"
  style={`--soundscape-level-start: ${start}%; --soundscape-level-end: ${end}%;`}
  oninput={(event) => onchange(Number(event.currentTarget.value))}
/>

<style>
  .soundscape-level-slider {
    --soundscape-level-thumb-size: 0.875rem;
    --soundscape-level-track-height: 0.25rem;
    --soundscape-level-track-color: color-mix(in srgb, var(--foreground) 22%, transparent);
    --soundscape-level-thumb-border: color-mix(in srgb, var(--foreground) 18%, transparent);

    height: var(--soundscape-level-thumb-size);
    appearance: none;
    cursor: pointer;
    background: linear-gradient(
      to right,
      var(--soundscape-level-track-color) 0%,
      var(--soundscape-level-track-color) var(--soundscape-level-start),
      var(--primary) var(--soundscape-level-start),
      var(--primary) var(--soundscape-level-end),
      var(--soundscape-level-track-color) var(--soundscape-level-end),
      var(--soundscape-level-track-color) 100%
    ) center / calc(100% - var(--soundscape-level-thumb-size)) var(--soundscape-level-track-height) no-repeat;
  }

  .soundscape-level-slider:disabled { cursor: default; }

  .soundscape-level-slider::-webkit-slider-runnable-track {
    height: var(--soundscape-level-track-height);
    border-radius: 999px;
    background: transparent;
  }

  .soundscape-level-slider::-webkit-slider-thumb {
    width: var(--soundscape-level-thumb-size);
    height: var(--soundscape-level-thumb-size);
    margin-top: calc((var(--soundscape-level-track-height) - var(--soundscape-level-thumb-size)) / 2);
    appearance: none;
    border: 1px solid var(--soundscape-level-thumb-border);
    border-radius: 999px;
    background: var(--card);
  }

  .soundscape-level-slider::-moz-range-track,
  .soundscape-level-slider::-moz-range-progress {
    height: var(--soundscape-level-track-height);
    border-radius: 999px;
    background: transparent;
  }

  .soundscape-level-slider::-moz-range-thumb {
    width: var(--soundscape-level-thumb-size);
    height: var(--soundscape-level-thumb-size);
    border: 1px solid var(--soundscape-level-thumb-border);
    border-radius: 999px;
    background: var(--card);
  }
</style>
