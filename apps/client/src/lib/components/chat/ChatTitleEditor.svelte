<script lang="ts">
  import { onMount } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  let {
    title,
    onCommit,
    onCancel,
  }: {
    title: string;
    onCommit: (title: string) => Promise<void>;
    onCancel: () => void;
  } = $props();

  const { t } = getLocalization();
  let draft = $state("");
  let input: HTMLInputElement | undefined = $state();
  let error = $state<string | null>(null);
  let committing = $state(false);

  onMount(() => {
    draft = title;
    input?.focus();
    input?.select();
  });

  async function commit(): Promise<void> {
    if (committing) return;
    const nextTitle = draft.trim();
    if (!nextTitle) {
      error = t("chat.header.emptyTitle");
      return;
    }
    committing = true;
    error = null;
    try {
      await onCommit(nextTitle);
    } catch (cause: unknown) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      committing = false;
    }
  }
</script>

<div class="max-w-sm" data-chat-title-editor>
  <input
    bind:this={input}
    class="field h-7 w-full px-2 text-identity font-medium"
    bind:value={draft}
    disabled={committing}
    onkeydown={(event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        void commit();
      }
      if (event.key === "Escape") {
        event.preventDefault();
        onCancel();
      }
    }}
    onblur={() => void commit()}
  />
  {#if error}<p role="alert" class="text-[0.666667rem] text-destructive">{error}</p>{/if}
</div>
