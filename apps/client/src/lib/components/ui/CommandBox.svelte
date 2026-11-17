<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import { onDestroy } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { writeTextToClipboard } from "$lib/utils/clipboard";
  import { cn } from "$lib/utils";

  /**
   * A terminal command shown in monospace with a button that copies it. The prompt marker is
   * decoration only: it is not selectable and the button copies the command alone.
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
  let feedbackTimer: number | undefined;

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

<div class={cn("flex h-8 items-center gap-2 rounded-md bg-muted ps-3 pe-1", className)} dir="ltr">
  <code class="min-w-0 flex-1 overflow-x-auto font-mono text-[0.8rem] whitespace-nowrap text-foreground">
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
