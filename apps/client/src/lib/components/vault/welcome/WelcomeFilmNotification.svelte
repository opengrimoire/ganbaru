<script lang="ts" module>
  /** Which operating system's notification the welcome scenes draw. */
  export type FilmNotificationKind = "android" | "windows" | "gnome";

  /** One of a notification's actions, with the press on it from 0 to 1. */
  export interface FilmNotificationAction {
    label: string;
    touch: number;
  }
</script>

<script lang="ts">
  import appIconUrl from "../../../../../src-tauri/icons/64x64.png";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import WelcomeAndroidNotification, { type AndroidNotificationIcon } from "./WelcomeAndroidNotification.svelte";

  /**
   * A Ganbaru notification as the operating system draws it, outside the app: a GNOME banner, a Windows toast, or an
   * Android notification. It follows the system's light or dark style rather than the app theme, because the system
   * paints it. Every style is a function of its props, with no CSS transitions.
   */
  let {
    kind,
    shown,
    title,
    body,
    actions = [],
    progress = null,
    androidIcon = "focus",
  }: {
    kind: FilmNotificationKind;
    /** How far it has slid in, from 0 (past its edge of the canvas) to 1 (at rest). */
    shown: number;
    title: string;
    body: string;
    /** Its actions, in order. */
    actions?: readonly FilmNotificationAction[];
    /** A phase's progress, from 0 to 1, as Android's ongoing notification draws it; null draws no progress bar. */
    progress?: number | null;
    /** The small icon Android draws, after the feature that posts the notification. */
    androidIcon?: AndroidNotificationIcon;
  } = $props();

  const { t } = getLocalization();
  /** The gap between the notification and the canvas edges, in pixels. */
  const MARGIN_PX = 8;
  /** Android's body text size on the scene's canvas, in pixels. */
  const ANDROID_FONT_PX = 13;

  const hidden = $derived(`calc(${1 - shown} * (100% + ${MARGIN_PX}px))`);
  const transform = $derived(kind === "windows" ? `translateX(${hidden})` : `translate(-50%, calc(-1 * ${hidden}))`);
  const hasExtras = $derived(progress !== null || actions.length > 0);
</script>

{#snippet touch(amount: number)}
  {#if amount > 0}
    <span
      class="os-touch pointer-events-none absolute top-1/2 left-1/2 z-20 size-6 -translate-1/2 rounded-full"
      style:opacity={amount}
    ></span>
  {/if}
{/snippet}

{#snippet close()}
  <svg class="size-2.5" viewBox="0 0 10 10" aria-hidden="true">
    <path d="M2 2l6 6M8 2l-6 6" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" fill="none" />
  </svg>
{/snippet}

{#snippet androidExtras()}
  {#if progress !== null}
    <div class="os-progress"><div class="os-progress-value" style:width={`${progress * 100}%`}></div></div>
  {/if}
  {#if actions.length > 0}
    <div class="flex">
      {#each actions as action (action.label)}
        <span class={cn("os-action relative", action.touch > 0 && "os-action-pressed")}>
          {action.label}
          {@render touch(action.touch)}
        </span>
      {/each}
    </div>
  {/if}
{/snippet}

{#if kind === "android"}
  <WelcomeAndroidNotification
    {shown}
    margin={MARGIN_PX}
    fontSize={ANDROID_FONT_PX}
    {title}
    {body}
    icon={androidIcon}
    class="z-50"
    children={hasExtras ? androidExtras : undefined}
  />
{:else}
<div
  class={cn("os-notification pointer-events-none absolute z-50", `os-${kind}`)}
  style:transform
  style:--os-margin={`${MARGIN_PX}px`}
  aria-hidden="true"
>
  <div class="os-header">
    <img src={appIconUrl} alt="" class="size-3.5 shrink-0 rounded-[22%]" draggable="false" />
    <span class="truncate">{t("common.appName")}</span>
    {#if kind === "gnome"}
      <span class="os-close ml-auto">{@render close()}</span>
    {:else}
      <span class="ml-auto flex items-center gap-3">
        <svg class="size-2.5" viewBox="0 0 10 10" aria-hidden="true">
          <circle cx="1.5" cy="5" r="0.9" fill="currentColor" />
          <circle cx="5" cy="5" r="0.9" fill="currentColor" />
          <circle cx="8.5" cy="5" r="0.9" fill="currentColor" />
        </svg>
        {@render close()}
      </span>
    {/if}
  </div>
  <div class="os-content">
    <p class="os-title">{title}</p>
    <p class="os-body">{body}</p>
  </div>
  {#if actions.length > 0}
    <div class="os-actions">
      {#each actions as action (action.label)}
        <span class={cn("os-button relative", action.touch > 0 && "os-button-pressed")}>
          <span class="block truncate">{action.label}</span>
          {@render touch(action.touch)}
        </span>
      {/each}
    </div>
  {/if}
</div>
{/if}

<style>
  /*
   * The operating systems' own notification styles, in their light and dark variants. These colors mirror each
   * system's palette (GNOME Shell's stylesheet, Windows 11's toast surface, Android's Material notification), not the
   * app theme, so the film reads as the system speaking rather than the app.
   */
  .os-notification {
    color: var(--os-fg);
    background: var(--os-bg);
    line-height: 1.3;
  }

  /* The film's press, drawn in the system's style so it shows on either variant. */
  .os-touch {
    background: color-mix(in srgb, var(--os-fg) 20%, transparent);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--os-fg) 35%, transparent);
  }

  .os-header {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--os-muted);
  }

  .os-title {
    font-weight: 700;
  }

  .os-body {
    color: var(--os-body);
  }

  .os-actions {
    display: grid;
    grid-auto-columns: 1fr;
    grid-auto-flow: column;
    text-align: center;
  }

  .os-button {
    background: var(--os-button);
  }

  .os-button-pressed {
    background: var(--os-button-pressed);
  }

  /* GNOME Shell: a banner at the top center with a 16px radius, a symbolic close button, and pill actions. */
  .os-gnome {
    top: var(--os-margin);
    left: 50%;
    width: min(300px, 100% - 2 * var(--os-margin));
    padding: 6px;
    border-radius: 16px;
    box-shadow: 0 2px 4px 2px rgb(0 0 0 / 0.18);
    font-family: "Adwaita Sans", Cantarell, Inter, "Noto Sans", sans-serif;
    font-size: 12px;
    --os-bg: #ffffff;
    --os-fg: #222226;
    --os-muted: #6d6d71;
    --os-body: #222226;
    --os-button: rgb(0 0 6 / 0.08);
    --os-button-pressed: rgb(0 0 6 / 0.16);
  }

  .os-gnome .os-header {
    padding: 0 6px;
    font-size: 11px;
  }

  .os-gnome .os-close {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 9999px;
    background: var(--os-button);
    color: var(--os-fg);
  }

  .os-gnome .os-content {
    padding: 4px 6px 6px;
  }

  .os-gnome .os-actions {
    gap: 6px;
    padding: 2px;
  }

  .os-gnome .os-button {
    padding: 5px 10px;
    border-radius: 8px;
    font-weight: 700;
  }

  /* Windows 11: a toast at the bottom right with its attribution row, an 8px radius, and outlined actions. */
  .os-windows {
    right: var(--os-margin);
    bottom: var(--os-margin);
    width: min(272px, 100% - 2 * var(--os-margin));
    padding: 12px;
    border: 1px solid var(--os-border);
    border-radius: 8px;
    box-shadow: 0 8px 16px rgb(0 0 0 / 0.26);
    font-family: "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
    font-size: 12px;
    --os-bg: #f3f3f3;
    --os-fg: #1a1a1a;
    --os-muted: #5c5c5c;
    --os-body: #5c5c5c;
    --os-border: rgb(0 0 0 / 0.08);
    --os-button: #fdfdfd;
    --os-button-pressed: #f0f0f0;
  }

  .os-windows .os-header {
    font-size: 11px;
  }

  .os-windows .os-content {
    padding: 10px 0 12px;
  }

  .os-windows .os-title {
    font-weight: 600;
  }

  .os-windows .os-actions {
    gap: 8px;
  }

  .os-windows .os-button {
    padding: 5px 8px;
    border: 1px solid var(--os-border);
    border-radius: 4px;
  }

  /* Android's progress bar and text action, in the palette the Android notification sets. */
  .os-progress {
    height: 4px;
    margin-top: 8px;
    overflow: hidden;
    border-radius: 9999px;
    background: var(--os-track);
  }

  .os-progress-value {
    height: 100%;
    border-radius: 9999px;
    background: var(--os-accent);
  }

  .os-action {
    margin: 4px 0 0 -8px;
    padding: 6px 8px;
    border-radius: 9999px;
    color: var(--os-accent);
    font-weight: 500;
  }

  .os-action-pressed {
    background: var(--os-button-pressed);
  }

  @media (prefers-color-scheme: dark) {
    .os-gnome {
      box-shadow: 0 2px 4px 2px rgb(0 0 0 / 0.35);
      --os-bg: #505055;
      --os-fg: #ffffff;
      --os-muted: #c0c0c3;
      --os-body: #ffffff;
      --os-button: rgb(255 255 255 / 0.1);
      --os-button-pressed: rgb(255 255 255 / 0.2);
    }

    .os-windows {
      box-shadow: 0 8px 16px rgb(0 0 0 / 0.4);
      --os-bg: #2c2c2c;
      --os-fg: #ffffff;
      --os-muted: #cfcfcf;
      --os-body: #cfcfcf;
      --os-border: rgb(255 255 255 / 0.08);
      --os-button: rgb(255 255 255 / 0.06);
      --os-button-pressed: rgb(255 255 255 / 0.1);
    }
  }
</style>
