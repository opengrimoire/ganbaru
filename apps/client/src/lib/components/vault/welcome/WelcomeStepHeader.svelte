<script lang="ts">
  import type { Snippet } from "svelte";
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";

  /**
   * The title and the optional short text a welcome screen opens with, plus anything that belongs right under the text. A line
   * break in the text holds on the phone shell, where a short closing sentence can read better on its own line, and
   * becomes a space elsewhere.
   */
  let {
    headline,
    body,
    headingId,
    children,
  }: { headline: string; body?: string; headingId?: string; children?: Snippet } = $props();

  const phoneShell = BUILD_PLATFORM_PROFILE.shell === "mobile";
  const text = $derived(body === undefined ? null : phoneShell ? body : body.replaceAll("\n", " "));
</script>

<div class="space-y-2 @min-[860px]:space-y-3">
  <h1
    id={headingId}
    class="text-2xl font-semibold leading-tight tracking-tight text-balance text-foreground @min-[560px]:text-3xl @min-[860px]:text-4xl"
  >
    {headline}
  </h1>
  {#if text !== null}
    <p class="text-sm leading-6 text-pretty whitespace-pre-line text-muted-foreground @min-[560px]:text-base @min-[560px]:leading-7">{text}</p>
  {/if}
  {@render children?.()}
</div>
