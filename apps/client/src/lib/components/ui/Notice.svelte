<script lang="ts">
  import { onDestroy, type Component, type Snippet } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Copy from "@lucide/svelte/icons/copy";
  import CopyX from "@lucide/svelte/icons/copy-x";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import X from "@lucide/svelte/icons/x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { writeTextToClipboard } from "$lib/utils/clipboard";

  /**
   * `inline` flows where the failed content would sit, `fill` centers the notice in the area the failed content
   * would occupy, and `floating` stands in for a dialog or panel that could not open.
   */
  type NoticeLayout = "inline" | "fill" | "floating";
  /** `warning` tints the icon for conditions that limit a feature without being a failure. */
  type NoticeTone = "neutral" | "warning";
  type FeedbackState = "idle" | "pending" | "done" | "failed";

  /** The retry icon turns at least once so a fast retry still reads as a response to the press. */
  const RETRY_MIN_SPIN_MS = 480;
  const FEEDBACK_MS = 1_600;

  let {
    message,
    title,
    icon: Icon = CircleAlert,
    tone = "neutral",
    layout = "inline",
    role = "alert",
    onRetry,
    retryLabel,
    copy,
    onDismiss,
    actions,
    class: className,
  }: {
    message: string;
    /** Short lead-in shown before the message, such as the name of the step that failed. */
    title?: string;
    icon?: Component<{ class?: string; strokeWidth?: number; "aria-hidden"?: boolean | "true" | "false" }>;
    tone?: NoticeTone;
    layout?: NoticeLayout;
    role?: "alert" | "status";
    /** Retries the failed work. A returned promise keeps the icon turning until it settles. */
    onRetry?: () => unknown;
    retryLabel?: string;
    /** Text the copy button writes; defaults to the title and message, and `false` hides the button. */
    copy?: string | false;
    /** Closes the notice; floating notices for a dialog that failed to open need it so the user can back out. */
    onDismiss?: () => void;
    /** Extra text actions shown before the icon buttons; style them with `notice-action-text`. */
    actions?: Snippet;
    class?: string;
  } = $props();

  const { t } = getLocalization();
  const copyText = $derived(copy === false ? null : copy ?? (title ? `${title} ${message}` : message));

  let retryState = $state<FeedbackState>("idle");
  let copyState = $state<FeedbackState>("idle");
  let destroyed = false;
  const timers = new Set<ReturnType<typeof setTimeout>>();

  function wait(ms: number): Promise<void> {
    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        timers.delete(timer);
        resolve();
      }, ms);
      timers.add(timer);
    });
  }

  async function settleFeedback(set: (state: FeedbackState) => void, state: FeedbackState): Promise<void> {
    set(state);
    await wait(FEEDBACK_MS);
    if (!destroyed) set("idle");
  }

  async function retry(): Promise<void> {
    if (!onRetry || retryState === "pending") return;
    retryState = "pending";
    const startedAt = performance.now();
    let succeeded = true;
    try {
      await onRetry();
    } catch (error: unknown) {
      succeeded = false;
      console.warn("Notice retry failed", error);
    }
    const remaining = RETRY_MIN_SPIN_MS - (performance.now() - startedAt);
    if (remaining > 0) await wait(remaining);
    if (destroyed) return;
    if (succeeded) await settleFeedback((state) => { retryState = state; }, "done");
    else retryState = "idle";
  }

  async function copyMessage(): Promise<void> {
    if (copyText === null) return;
    try {
      await writeTextToClipboard(copyText);
      if (!destroyed) await settleFeedback((state) => { copyState = state; }, "done");
    } catch (error: unknown) {
      console.warn("Notice copy failed", error);
      if (!destroyed) await settleFeedback((state) => { copyState = state; }, "failed");
    }
  }

  const copyAnnouncement = $derived(copyState === "done" ? t("common.notice.copied") : copyState === "failed" ? t("common.notice.copyFailed") : "");

  onDestroy(() => {
    destroyed = true;
    for (const timer of timers) clearTimeout(timer);
    timers.clear();
  });
</script>

{#snippet retryButton()}
  <button
    type="button"
    class="notice-action-icon"
    data-notice-retry={retryState}
    aria-label={retryLabel ?? t("common.retry")}
    aria-busy={retryState === "pending" || undefined}
    onclick={() => void retry()}
  >{#if retryState === "done"}<Check aria-hidden="true" />{:else}<span class={cn("notice-spin", retryState === "pending" && "notice-spin-active")}><RotateCw aria-hidden="true" /></span>{/if}</button>
{/snippet}

{#snippet copyButton()}
  <button
    type="button"
    class="notice-action-icon"
    data-notice-copy={copyState}
    aria-label={copyState === "failed" ? t("common.notice.copyFailed") : t("common.notice.copy")}
    onclick={() => void copyMessage()}
  >{#if copyState === "done"}<Check aria-hidden="true" />{:else if copyState === "failed"}<CopyX aria-hidden="true" />{:else}<Copy aria-hidden="true" />{/if}</button>
{/snippet}

<!-- Children stay on one line so the notice's text content is exactly the title and message. -->
{#snippet body()}
  <div class={cn("notice", layout === "floating" && "surface-floating notice-floating", className)} {role} data-notice-tone={tone}><span class="notice-icon" aria-hidden="true"><Icon strokeWidth={1.8} aria-hidden="true" /></span><p class="notice-message">{#if title}<strong class="font-semibold">{title}</strong>{" "}{/if}{message}</p>{#if actions || onRetry || copyText !== null || onDismiss}<span class="notice-actions">{@render actions?.()}{#if onRetry}{@render retryButton()}{/if}{#if copyText !== null}{@render copyButton()}{/if}{#if onDismiss}<button type="button" class="notice-action-icon" data-notice-dismiss aria-label={t("common.close")} onclick={onDismiss}><X aria-hidden="true" /></button>{/if}</span>{/if}</div>
{/snippet}

{#if layout === "fill"}
  <div class="notice-fill">{@render body()}</div>
{:else}
  {@render body()}
{/if}
<!-- The copy announcement sits outside the notice so it never joins the alert text. -->
{#if copyText !== null}<span class="sr-only" aria-live="polite">{copyAnnouncement}</span>{/if}

<style>
  .notice { display: flex; min-width: 0; align-items: flex-start; gap: 0.5em; color: var(--foreground); line-height: 1.45; text-align: start; }
  .notice-fill { display: grid; width: 100%; height: 100%; min-height: inherit; place-items: center; padding: 1rem; }
  .notice-fill > .notice { max-width: 26rem; }
  /* Stands in for a dialog, so it shares the top dialog layer instead of the panel layer. */
  .notice-floating { position: fixed; inset: 0; z-index: 100; width: fit-content; max-width: min(26rem, calc(100vw - 2rem)); height: fit-content; margin: auto; padding: 0.5rem 0.625rem; color: var(--popover-foreground); }
  .notice-icon { display: flex; height: 1lh; flex: 0 0 auto; align-items: center; color: var(--muted-foreground); }
  .notice-icon :global(svg) { width: 1.1em; height: 1.1em; }
  .notice[data-notice-tone="warning"] .notice-icon { color: var(--status-tentative); }
  .notice-message { min-width: 0; flex: 1 1 auto; overflow-wrap: anywhere; }
  .notice-actions { display: flex; min-height: 1lh; flex: 0 0 auto; flex-wrap: wrap; align-items: center; justify-content: flex-end; gap: 0.125em; margin-block: -0.125em; }
  .notice-action-icon { display: grid; width: 1.625em; height: 1.625em; flex: 0 0 auto; place-items: center; border-radius: var(--floating-item-radius); color: var(--muted-foreground); transition: background-color 120ms ease, color 120ms ease; }
  .notice-action-icon:hover { background: var(--accent); color: var(--foreground); }
  .notice-action-icon[aria-busy="true"] { cursor: progress; }
  .notice-action-icon :global(svg) { width: 1em; height: 1em; }
  .notice-action-icon[data-notice-copy="failed"] { color: var(--destructive); }
  .notice-spin { display: inline-flex; }
  .notice-spin-active { animation: notice-spin 480ms ease-in-out infinite; }
  .notice-actions :global(.notice-action-text) { display: inline-flex; height: 1.625em; align-items: center; border-radius: var(--floating-item-radius); padding-inline: 0.375em; color: var(--foreground); font-weight: 500; transition: background-color 120ms ease; }
  .notice-actions :global(.notice-action-text:hover) { background: var(--accent); }
  .notice-actions :global(.notice-action-text:disabled) { cursor: not-allowed; opacity: 0.5; }
  :global(html[data-shell="mobile"]) .notice-action-icon { width: 2.75rem; height: 2.75rem; }
  :global(html[data-shell="mobile"]) .notice-actions :global(.notice-action-text) { height: 2.75rem; padding-inline: 0.625rem; }
  @media (any-pointer: coarse) and (any-hover: none) {
    .notice-action-icon { width: 2.75rem; height: 2.75rem; }
    .notice-actions :global(.notice-action-text) { height: 2.75rem; padding-inline: 0.625rem; }
  }
  @keyframes notice-spin {
    to { transform: rotate(360deg); }
  }
  @media (prefers-reduced-motion: reduce) {
    .notice-spin-active { animation: none; opacity: 0.6; }
  }
</style>
