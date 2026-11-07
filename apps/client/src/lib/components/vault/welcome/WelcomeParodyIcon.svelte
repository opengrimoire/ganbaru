<script lang="ts" module>
  import type { BlockerApp } from "$lib/vault/welcome-blocker";

  /** The parody apps' accent colors, the same in every theme. */
  export const PARODY_COLORS: Readonly<Record<BlockerApp, string>> = { y: "#000000", bluedit: "#2f6bff", egogram: "#e8317f" };

  /**
   * Egogram's sunset gradient, warm from the bottom left corner to violet at the top right, which marks it as a photo
   * app's parody without copying any brand's exact colors.
   */
  export const EGOGRAM_GRADIENT = "radial-gradient(circle at 28% 108%, #ffd76a 0%, #ffb340 10%, #ff5f45 40%, #e8317f 62%, #8b3dff 95%)";

  /** The parody apps' icon backgrounds. */
  const PARODY_BACKGROUNDS: Readonly<Record<BlockerApp, string>> = {
    y: PARODY_COLORS.y,
    bluedit: PARODY_COLORS.bluedit,
    egogram: EGOGRAM_GRADIENT,
  };
</script>

<script lang="ts">
  /**
   * The icon of a parody app in the distraction blocker's welcome scenes: Y's letter, Bluedit's speech bubble with an
   * up arrow, and Egogram's camera on its sunset gradient, each on its brand background.
   */
  let { app, size }: { app: BlockerApp; size: number } = $props();

  const glyph = $derived(`${size * 0.74}px`);
</script>

<span
  class="relative grid shrink-0 place-items-center overflow-hidden"
  style:width={`${size}px`}
  style:height={`${size}px`}
  style:border-radius={`${size * 0.26}px`}
  style:background={PARODY_BACKGROUNDS[app]}
  aria-hidden="true"
>
  {#if app === "y"}
    <span class="absolute inset-0 rounded-[inherit] ring-1 ring-white/15 ring-inset"></span>
    <span class="leading-none font-black text-white" style:font-size={`${size * 0.62}px`}>Y</span>
  {:else if app === "bluedit"}
    <svg viewBox="0 0 24 24" style:width={glyph} style:height={glyph}>
      <path d="M12 3.5c-4.7 0-8.5 3.1-8.5 7 0 2.1 1.1 4 2.9 5.3L5.7 20l3.9-2c.8.2 1.6.3 2.4.3 4.7 0 8.5-3.1 8.5-7s-3.8-7.8-8.5-7.8z" fill="white" />
      <path d="M12 6.8l-3.4 3.6h2.2v3.4h2.4v-3.4h2.2z" fill={PARODY_COLORS.bluedit} />
    </svg>
  {:else}
    <svg viewBox="0 0 24 24" style:width={glyph} style:height={glyph} fill="none" stroke="white" stroke-width="2" stroke-linejoin="round">
      <path d="M3.5 9.5a2.5 2.5 0 0 1 2.5-2.5h2.2l1.4-2.2h4.8l1.4 2.2H18a2.5 2.5 0 0 1 2.5 2.5v8a2.5 2.5 0 0 1-2.5 2.5H6a2.5 2.5 0 0 1-2.5-2.5z" />
      <circle cx="12" cy="13.2" r="3.4" />
      <circle cx="17.4" cy="10" r="0.6" fill="white" stroke-width="0.8" />
    </svg>
  {/if}
</span>
