<script lang="ts">
  import type { Snippet } from "svelte";

  /** The title and short text every welcome screen opens with, plus anything that belongs right under the text. */
  let {
    headline,
    body,
    headingId,
    children,
  }: { headline: string; body: string; headingId?: string; children?: Snippet } = $props();
</script>

<div class="space-y-2 @min-[860px]:space-y-3">
  <h1
    id={headingId}
    class="text-2xl font-semibold leading-tight tracking-tight text-balance text-foreground @min-[560px]:text-3xl @min-[860px]:text-4xl"
  >
    {headline}
  </h1>
  <p class="text-sm leading-6 text-pretty text-muted-foreground @min-[560px]:text-base @min-[560px]:leading-7">{body}</p>
  {@render children?.()}
</div>
