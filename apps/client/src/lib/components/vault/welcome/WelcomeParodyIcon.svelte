<script lang="ts" module>
  import type { BlockerApp } from "$lib/vault/welcome-blocker";

  /** The parody apps' accent colors, the same in every theme. */
  export const PARODY_COLORS: Readonly<Record<BlockerApp, string>> = { y: "#000000", bluedit: "#2f6bff", egogram: "#e8317f" };

  /**
   * Egogram's sunset gradient, warm from the bottom left corner to violet at the top right, which marks it as a photo
   * app's parody without copying any brand's exact colors.
   */
  export const EGOGRAM_GRADIENT = "radial-gradient(circle at 28% 108%, #ffd76a 0%, #ffb340 10%, #ff5f45 40%, #e8317f 62%, #8b3dff 95%)";

  /** The parody apps the icon draws: the blocked apps, and the mail site the scenes show as work. */
  export type ParodyIconApp = BlockerApp | "mail";

  /** The mail site's four colors, shifted from a well-known mail logo's so the parody reads without copying it. */
  export const MAIL_COLORS = { red: "#e5483b", blue: "#3d7cf0", green: "#2fa75a", yellow: "#f6b70f" } as const;

  /** The parody apps' icon backgrounds. */
  const PARODY_BACKGROUNDS: Readonly<Record<ParodyIconApp, string>> = {
    y: PARODY_COLORS.y,
    bluedit: PARODY_COLORS.bluedit,
    egogram: EGOGRAM_GRADIENT,
    mail: "#ffffff",
  };
</script>

<script lang="ts">
  /**
   * The icon of a parody app in the distraction blocker's welcome scenes: Y's hollow letter, Bluedit's alien,
   * Egogram's camera on its sunset gradient, and Gnail's four-color envelope, each on its brand background.
   */
  let { app, size }: { app: ParodyIconApp; size: number } = $props();

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
    <!-- A Y in the hollow and hairline stroke style of a well-known one-letter logo: a hollow band from the top left
         corner that bends into the stem, met by a thin solid arm from the top right. -->
    <svg viewBox="0 0 24 24" style:width={glyph} style:height={glyph}>
      <path d="M4.2 4h3.6l5.8 8.05V20h-3.2v-7.4z" fill="none" stroke="white" stroke-width="1.1" stroke-linejoin="miter" />
      <path d="M19.7 4l-6.3 7.4" stroke="white" stroke-width="1.6" />
    </svg>
  {:else if app === "bluedit"}
    <!-- A round-eared alien whose antenna leans to the left, its head outlined in the brand color to part it from the
         ears. The view centers the whole alien, antenna included, as the other glyphs are centered. -->
    <svg viewBox="1 -0.4 22 22" style:width={glyph} style:height={glyph}>
      <path d="M12.2 9.4L11 3.9L8.4 3.4" fill="none" stroke="white" stroke-width="0.9" stroke-linecap="round" stroke-linejoin="round" />
      <circle cx="7.3" cy="3.3" r="1.5" fill="white" />
      <circle cx="4.6" cy="11.6" r="1.9" fill="white" />
      <circle cx="19.4" cy="11.6" r="1.9" fill="white" />
      <ellipse cx="12" cy="14.2" rx="7.6" ry="5.2" fill="white" stroke={PARODY_COLORS.bluedit} stroke-width="0.9" />
      <ellipse cx="9.3" cy="13.4" rx="1.3" ry="1.5" fill={PARODY_COLORS.bluedit} />
      <ellipse cx="14.7" cy="13.4" rx="1.3" ry="1.5" fill={PARODY_COLORS.bluedit} />
      <path d="M9 16.4q3 2 6 0" fill="none" stroke={PARODY_COLORS.bluedit} stroke-width="0.9" stroke-linecap="round" />
    </svg>
  {:else if app === "mail"}
    <span class="absolute inset-0 rounded-[inherit] ring-1 ring-black/10 ring-inset"></span>
    <!-- An envelope drawn as an M in four colors, its sides swapped and its yellow corner moved to the left. -->
    <svg viewBox="0 0.3 24 24" style:width={glyph} style:height={glyph} fill="none" stroke-linecap="round" stroke-linejoin="round">
      <path d="M3 8.2v8.8a1.5 1.5 0 0 0 1.5 1.5h2.2v-8.3z" fill={MAIL_COLORS.green} />
      <path d="M21 8.2v8.8a1.5 1.5 0 0 1-1.5 1.5h-2.2v-8.3z" fill={MAIL_COLORS.blue} />
      <path d="M4.3 7.4L12 13l7.7-5.6" stroke={MAIL_COLORS.red} stroke-width="2.6" />
      <path d="M4.3 7.4L6.4 8.9" stroke={MAIL_COLORS.yellow} stroke-width="2.6" />
    </svg>
  {:else}
    <svg viewBox="0 0.4 24 24" style:width={glyph} style:height={glyph} fill="none" stroke="white" stroke-width="2" stroke-linejoin="round">
      <path d="M3.5 9.5a2.5 2.5 0 0 1 2.5-2.5h2.2l1.4-2.2h4.8l1.4 2.2H18a2.5 2.5 0 0 1 2.5 2.5v8a2.5 2.5 0 0 1-2.5 2.5H6a2.5 2.5 0 0 1-2.5-2.5z" />
      <circle cx="12" cy="13.2" r="3.4" />
      <circle cx="17.4" cy="10" r="0.6" fill="white" stroke-width="0.8" />
    </svg>
  {/if}
</span>
