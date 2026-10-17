<script lang="ts">
  let {
    orientation,
    value,
    minimum,
    maximum,
    label,
    active = false,
    onPointerDown,
    onKeyDown,
  }: {
    orientation: "vertical" | "horizontal";
    value: number;
    minimum: number;
    maximum: number;
    label: string;
    active?: boolean;
    onPointerDown: (event: PointerEvent) => void;
    onKeyDown: (event: KeyboardEvent) => void;
  } = $props();
</script>

<div class="pane-resize-handle {orientation}" class:active>
  <input
    type="range"
    min={minimum}
    max={maximum}
    step="any"
    {value}
    aria-label={label}
    aria-orientation={orientation === "vertical" ? "horizontal" : "vertical"}
    onpointerdown={onPointerDown}
    onkeydown={onKeyDown}
  />
  <span aria-hidden="true"></span>
</div>

<style>
  .pane-resize-handle { position: relative; z-index: 2; background: transparent; }
  .pane-resize-handle input { position: absolute; inset: 0; width: 100%; height: 100%; touch-action: none; appearance: none; margin: 0; cursor: inherit; opacity: 0; }
  .pane-resize-handle span { --chat-pane-divider-highlight: color-mix(in srgb, var(--ring) 55%, var(--border)); position: absolute; pointer-events: none; background: var(--border); }
  .pane-resize-handle.vertical { width: 8px; min-width: 8px; flex: 0 0 8px; margin-inline: -4px; cursor: col-resize; }
  .pane-resize-handle.vertical span { inset-block: 0; left: 50%; width: 1px; }
  .pane-resize-handle.vertical:is(:hover, .active) span, .pane-resize-handle.vertical input:focus-visible + span { background: linear-gradient(to bottom, var(--border), var(--chat-pane-divider-highlight) 50%, var(--border)); }
  .pane-resize-handle.horizontal { width: 100%; height: 8px; min-height: 8px; flex: 0 0 8px; margin-block: -4px; cursor: row-resize; }
  .pane-resize-handle.horizontal span { inset-inline: 0; top: 50%; height: 1px; }
  .pane-resize-handle.horizontal:is(:hover, .active) span, .pane-resize-handle.horizontal input:focus-visible + span { background: linear-gradient(to right, var(--border), var(--chat-pane-divider-highlight) 50%, var(--border)); }
</style>
