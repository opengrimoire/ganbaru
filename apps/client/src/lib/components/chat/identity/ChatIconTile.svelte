<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * Square tile behind provider logos and other identity glyphs: a light surface with a black glyph
   * so logos and icons read the same in both themes. `background` and `color` override the tile for
   * brands with their own colors.
   */
  let {
    size = 36,
    label,
    class: className,
    background,
    color,
    children,
  }: {
    size?: number;
    label?: string;
    class?: string;
    background?: string;
    color?: string;
    children: Snippet;
  } = $props();
</script>

<span
  class={["icon-tile", className]}
  style:width={`${size}px`}
  style:height={`${size}px`}
  style:--icon-tile-background={background}
  style:--icon-tile-color={color}
  role={label ? "img" : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : "true"}
>
  {@render children()}
</span>

<style>
  .icon-tile {
    display: inline-grid;
    flex: 0 0 auto;
    place-items: center;
    overflow: hidden;
    border-radius: var(--chat-participant-avatar-radius, 22%);
    background: var(--icon-tile-background, #f2f2f2);
    color: var(--icon-tile-color, #000000);
  }
</style>
