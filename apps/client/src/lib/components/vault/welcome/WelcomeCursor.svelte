<script lang="ts">
  import { untrack } from "svelte";
  import { WELCOME_CURSOR_SIZE_PX, welcomeCursorFrame, type WelcomeCursorScript, type WelcomeCursorStop } from "$lib/vault/welcome-cursor";

  /**
   * The desktop welcome scenes' mouse pointer. It covers the scene's drawing, finds each place of `script` by the
   * `data-cursor` names inside the drawing, or inside the nearest enclosing `data-cursor-root` so it can head for a
   * place past its own drawing's edge, which hides it as it crosses, and glides between them, so it lands on a control wherever the drawing lays
   * it out. Once it lands it stays put while the drawing scrolls or changes under it, unless its place follows an element
   * it drags. Elements laid out but hidden are not places it can go. A held button shows as a ring under a slightly
   * smaller pointer, and the pointer fades away while it rests idle for a while. It keeps the system's black arrow with a
   * white edge in every theme, as the real one does.
   */
  let {
    script,
    elapsed,
    width,
    size = WELCOME_CURSOR_SIZE_PX,
  }: {
    script: WelcomeCursorScript;
    elapsed: number;
    /** The drawing's width in logical pixels, which maps measured positions back into the zoomed drawing. */
    width: number;
    /** The pointer's drawn size in logical pixels. */
    size?: number;
  } = $props();

  interface Point {
    x: number;
    y: number;
  }

  /** The point of each shape that clicks, in its 24 unit view box. */
  const HOTSPOTS = { arrow: [5, 2], pointer: [9.5, 1.5], text: [12, 12] } as const;

  let layer: HTMLDivElement | undefined = $state();
  let point: Point | null = $state(null);
  const frame = $derived(welcomeCursorFrame(script, elapsed));

  /** Where the pointer left from, and the place it is heading to, so a path keeps its start once that element goes. */
  let origin: Point | null = null;
  let heading: WelcomeCursorStop | null = null;
  /** The drawing's width when the pointer landed on its place, after which it stays put until the drawing resizes. */
  let landedWidth: number | null = null;

  /** Where `stop` lands in the drawing, or null while its element is not drawn or is laid out but hidden. */
  function locate(stop: WelcomeCursorStop, root: Element, layerRect: DOMRect, scale: number): Point | null {
    for (const element of root.querySelectorAll(`[data-cursor~="${CSS.escape(stop.target)}"]`)) {
      const rect = element.getBoundingClientRect();
      if (rect.width === 0 && rect.height === 0) continue;
      if (getComputedStyle(element).visibility === "hidden") continue;
      const [x, y] = stop.anchor ?? [0.5, 0.5];
      return { x: (rect.left - layerRect.left + rect.width * x) * scale, y: (rect.top - layerRect.top + rect.height * y) * scale };
    }
    return null;
  }

  $effect(() => {
    const { to, progress } = frame;
    const root = layer?.closest("[data-cursor-root]") ?? layer?.parentElement;
    if (!layer || !root) return;
    if (heading !== to) {
      heading = to;
      origin = untrack(() => point);
      landedWidth = null;
    }
    // A landed pointer stays put, as a mouse does while the drawing changes or scrolls under it.
    if (landedWidth === width && !to.follow) return;
    const layerRect = layer.getBoundingClientRect();
    if (layerRect.width === 0) return;
    const target = locate(to, root, layerRect, width / layerRect.width);
    if (target === null) return;
    const moving = origin !== null && !to.follow && progress < 1;
    point = moving && origin ? { x: origin.x + (target.x - origin.x) * progress, y: origin.y + (target.y - origin.y) * progress } : target;
    if (!moving) landedWidth = width;
  });

  const hotspot = $derived(HOTSPOTS[frame.glyph]);
  const scale = $derived(1 - 0.12 * frame.pressed);
</script>

<div bind:this={layer} class="pointer-events-none absolute inset-0 z-100 overflow-hidden" aria-hidden="true">
  {#if point && frame.shown > 0}
    {#if frame.pressed > 0}
      <span
        class="absolute -translate-1/2 rounded-full bg-foreground/15 ring-1 ring-foreground/30"
        style:left={`${point.x}px`}
        style:top={`${point.y}px`}
        style:width={`${size}px`}
        style:height={`${size}px`}
        style:opacity={frame.pressed * frame.shown}
      ></span>
    {/if}
    <svg
      class="absolute overflow-visible"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      style:left={`${point.x - (hotspot[0] * size) / 24}px`}
      style:top={`${point.y - (hotspot[1] * size) / 24}px`}
      style:opacity={frame.shown < 1 ? frame.shown : undefined}
      style:transform={`scale(${scale})`}
      style:transform-origin={`${(hotspot[0] * 100) / 24}% ${(hotspot[1] * 100) / 24}%`}
    >
      {#if frame.glyph === "text"}
        <g fill="none" stroke-linecap="round" stroke-linejoin="round">
          <path class="stroke-white" stroke-width="3.6" d="M8.5 3h2c.9 0 1.5.6 1.5 1.5 0-.9.6-1.5 1.5-1.5h2M12 4.5v15M8.5 21h2c.9 0 1.5-.6 1.5-1.5 0 .9.6 1.5 1.5 1.5h2" />
          <path class="stroke-black" stroke-width="1.5" d="M8.5 3h2c.9 0 1.5.6 1.5 1.5 0-.9.6-1.5 1.5-1.5h2M12 4.5v15M8.5 21h2c.9 0 1.5-.6 1.5-1.5 0 .9.6 1.5 1.5 1.5h2" />
        </g>
      {:else if frame.glyph === "pointer"}
        <g stroke-linecap="round" stroke-linejoin="round">
          <path
            class="fill-white stroke-black"
            stroke-width="1.4"
            d="M7.5 3.5a2 2 0 0 1 4 0v6.7a2 2 0 0 1 3.6.6 2 2 0 0 1 3.6.8 2 2 0 0 1 3.3 1.4V17c0 3.6-2.5 6.5-6.1 6.5h-3c-2.1 0-3.4-.8-4.7-2.3L3.3 16.9a1.8 1.8 0 0 1 2.6-2.6l1.6 1.7Z"
          />
          <path class="fill-none stroke-black" stroke-width="1.1" d="M11.5 10.2v3.6M15.1 10.8v3.2M18.7 11.6v2.6" />
        </g>
      {:else}
        <path class="fill-black stroke-white" stroke-width="1.5" stroke-linejoin="round" d="M5 2v17.5l4.4-4 3 6.5 3-1.3-2.9-6.4H18.5Z" />
      {/if}
    </svg>
  {/if}
</div>
