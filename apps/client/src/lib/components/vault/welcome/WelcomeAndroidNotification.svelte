<script lang="ts" module>
  /** The small icons Ganbaru's Android notifications set, named after their drawables. */
  export type AndroidNotificationIcon = "focus" | "music";

  /** Each small icon's path, copied from its `ic_notification_*` vector drawable on a 24 by 24 viewport. */
  const SMALL_ICON_PATHS: Record<AndroidNotificationIcon, string> = {
    focus: "M12,2A10,10 0,1 0,22 12A10,10 0,0 0,12 2M12,5A7,7 0,1 1,5 12A7,7 0,0 1,12 5M11,7H13V12.4L16.4,14.4L15.4,16.1L11,13.5Z",
    music: "M12,3V13.55C11.41,13.21 10.73,13 10,13A4,4 0,0 0,6 17A4,4 0,0 0,10 21A4,4 0,0 0,14 17V7H18V3H12Z",
  };
</script>

<script lang="ts">
  import type { Snippet } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";

  /**
   * A Ganbaru notification as Android draws it in the welcome scenes, dropped from the top: the small icon of the
   * feature that posts it tinted into its circle, the app's name, a title, and a body, with room below for a progress
   * bar or actions. It follows the system's light or dark Material palette rather than the app theme, because the
   * system paints it. Its sizes scale with `fontSize`, so the same notification fits a full canvas or a small phone.
   */
  let {
    shown,
    margin,
    fontSize,
    title,
    body,
    icon = "focus",
    class: className,
    children,
  }: {
    /** How far it has slid in, from 0 (past the top edge) to 1 (at rest). */
    shown: number;
    /** The gap between the notification and the screen's edges, in pixels. */
    margin: number;
    /** The body text's size in pixels, which every other size follows. */
    fontSize: number;
    title: string;
    body: string;
    /** The small icon: Focus's clock for Focus and the blocker, the music note for Music's media notification. */
    icon?: AndroidNotificationIcon;
    class?: string;
    /** What follows the body, such as a progress bar or actions. */
    children?: Snippet;
  } = $props();

  const { t } = getLocalization();
</script>

<div
  class={cn("android-notification pointer-events-none absolute", className)}
  style:top={`${margin}px`}
  style:left={`${margin}px`}
  style:right={`${margin}px`}
  style:font-size={`${fontSize}px`}
  style:padding-bottom={children ? "0.46em" : "0.92em"}
  style:transform={`translateY(calc(${-(1 - shown)} * (100% + ${margin}px)))`}
  aria-hidden="true"
>
  <div class="flex gap-[0.77em]">
    <span class="small-icon">
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path fill="currentColor" d={SMALL_ICON_PATHS[icon]} />
      </svg>
    </span>
    <div class="min-w-0 flex-1">
      <div class="header">
        <span class="truncate">{t("common.appName")}</span>
        <svg class="chevron" viewBox="0 0 12 12" aria-hidden="true">
          <path d="M3 4.5l3 3 3-3" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" fill="none" />
        </svg>
      </div>
      <p class="title truncate">{title}</p>
      <p class="body">{body}</p>
      {@render children?.()}
    </div>
  </div>
</div>

<style>
  /*
   * Android's Material notification palette in its light and dark variants. The variables also style what a parent
   * renders inside, such as a progress bar or an action.
   */
  .android-notification {
    padding-top: 0.92em;
    padding-right: 1.08em;
    padding-left: 0.92em;
    border-radius: 1.85em;
    background: var(--os-bg);
    box-shadow: 0 4px 12px rgb(0 0 0 / 0.2);
    color: var(--os-fg);
    font-family: "Google Sans Text", "Google Sans", Roboto, "Noto Sans", sans-serif;
    line-height: 1.3;
    --os-bg: #f3f3f8;
    --os-fg: #1b1b1f;
    --os-muted: #45464f;
    --os-accent: #0b57d0;
    --os-on-accent: #ffffff;
    --os-track: rgb(11 87 208 / 0.18);
    --os-button-pressed: rgb(11 87 208 / 0.12);
  }

  .small-icon {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 1.85em;
    height: 1.85em;
    border-radius: 9999px;
    background: var(--os-accent);
    color: var(--os-on-accent);
  }

  .small-icon svg {
    width: 0.92em;
    height: 0.92em;
  }

  .header {
    display: flex;
    align-items: center;
    gap: 0.5em;
    color: var(--os-muted);
    font-size: 0.846em;
  }

  .chevron {
    flex-shrink: 0;
    width: 1.09em;
    height: 1.09em;
    margin-left: auto;
  }

  .title {
    margin-top: 0.15em;
    font-weight: 500;
  }

  .body {
    color: var(--os-muted);
  }

  @media (prefers-color-scheme: dark) {
    .android-notification {
      box-shadow: 0 4px 12px rgb(0 0 0 / 0.45);
      --os-bg: #1f2024;
      --os-fg: #e3e2e6;
      --os-muted: #c5c6d0;
      --os-accent: #a8c7fa;
      --os-on-accent: #062e6f;
      --os-track: rgb(168 199 250 / 0.22);
      --os-button-pressed: rgb(168 199 250 / 0.16);
    }
  }
</style>
