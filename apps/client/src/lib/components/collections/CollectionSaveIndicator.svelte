<script module lang="ts">
  export const COLLECTION_SAVE_FEEDBACK_DELAY_MS = 600;
</script>

<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";

  let { pending, label, delayMs = COLLECTION_SAVE_FEEDBACK_DELAY_MS }: {
    pending: boolean;
    label: string;
    delayMs?: number;
  } = $props();
  let visible = $state(false);

  $effect(() => {
    visible = false;
    if (!pending) return;
    const timer = setTimeout(() => { visible = true; }, delayMs);
    return () => clearTimeout(timer);
  });
</script>

<span class="inline-flex size-4 shrink-0 items-center justify-center" aria-live="polite" aria-atomic="true">
  {#if pending && visible}
    <span role="status" aria-label={label}>
      <LoaderCircle size={16} strokeWidth={2} class="animate-spin text-muted-foreground motion-reduce:animate-none" aria-hidden="true" />
      <span class="sr-only">{label}</span>
    </span>
  {/if}
</span>
