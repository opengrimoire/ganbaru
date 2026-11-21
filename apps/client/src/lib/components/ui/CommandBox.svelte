<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import { onDestroy } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { writeTextToClipboard } from "$lib/utils/clipboard";
  import { cn } from "$lib/utils";
  import { hasHorizontalOverflow, observeScrollContent } from "$lib/utils/scroll-overflow";

  /**
   * A terminal command shown in monospace with a button that copies it. The prompt marker is
   * decoration only: it is not selectable and the button copies the command alone. A command
   * wider than the box scrolls sideways under a visible scrollbar, and the text keeps equal space
   * above and below while the scrollbar shows. The font size follows the `class` text size.
   */
  let {
    command,
    prompt = "$",
    class: className,
  }: {
    command: string;
    /** The marker before the command, such as `$` for a POSIX shell or `PS>` for PowerShell. */
    prompt?: string;
    class?: string;
  } = $props();

  /** How long the copy button shows that the command was copied. */
  const COPY_FEEDBACK_MS = 2_000;

  const { t } = getLocalization();
  let copied = $state(false);
  let overflowing = $state(false);
  let scrollElement = $state<HTMLElement | null>(null);
  let feedbackTimer: number | undefined;

  $effect(() => {
    const element = scrollElement;
    if (!element) return;
    // Reading `command` reruns the measurement when the text changes without resizing the box.
    void command;
    return observeScrollContent(element, () => { overflowing = hasHorizontalOverflow(element); });
  });

  async function copy(): Promise<void> {
    try {
      await writeTextToClipboard(command);
    } catch (cause: unknown) {
      console.warn("the command could not be copied", cause);
      return;
    }
    copied = true;
    window.clearTimeout(feedbackTimer);
    feedbackTimer = window.setTimeout(() => { copied = false; }, COPY_FEEDBACK_MS);
  }

  onDestroy(() => window.clearTimeout(feedbackTimer));
</script>

<div class={cn("flex min-w-0 items-center gap-2 rounded-md bg-muted pe-1 text-[0.8rem]", className)} dir="ltr">
  <code
    bind:this={scrollElement}
    class="command-scroll w-0 min-w-0 flex-1 overflow-x-auto py-1.5 ps-3 font-mono leading-5 whitespace-nowrap text-foreground"
    class:overflowing
  >
    <span class="text-muted-foreground select-none" aria-hidden="true">{prompt}</span> {command}
  </code>
  <button
    type="button"
    data-copy-command
    class="flex size-6 shrink-0 items-center justify-center rounded-sm text-muted-foreground hover:bg-background hover:text-foreground pointer-coarse:size-9"
    title={copied ? t("common.commandCopied") : t("common.copyCommand")}
    aria-label={copied ? t("common.commandCopied") : t("common.copyCommand")}
    onclick={() => void copy()}
  >
    {#if copied}<Check size={14} aria-hidden="true" />{:else}<Copy size={14} aria-hidden="true" />{/if}
  </button>
</div>

<style>
  /* The shared thumb matches the muted box, so it is drawn darker to stay visible without hovering. */
  .command-scroll {
    --app-scrollbar-thumb: color-mix(in oklab, var(--muted-foreground) 45%, transparent);
  }

  /* The scrollbar takes space under the text, so the same space above keeps the text centered. */
  .command-scroll.overflowing {
    padding-top: calc(0.375rem + var(--app-scrollbar-size));
  }
</style>
