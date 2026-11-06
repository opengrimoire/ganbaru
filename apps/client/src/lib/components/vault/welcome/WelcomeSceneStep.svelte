<script lang="ts">
  import type { Snippet } from "svelte";
  import WelcomeStepHeader from "./WelcomeStepHeader.svelte";

  /**
   * A welcome screen built around a scene: the title and text in the same place as on every welcome screen, and the
   * scene filling the room left above the navigation. The screen scrolls when even the text does not fit.
   */
  let {
    headline,
    body,
    details,
    children,
  }: { headline: string; body: string; details?: Snippet; children: Snippet } = $props();
</script>

<section class="flex h-full flex-col overflow-x-hidden overflow-y-auto">
  <div class="mx-auto w-full max-w-xl shrink-0 px-4 pt-12 @min-[560px]:px-8 @min-[560px]:pt-18 @min-[860px]:pt-26">
    <WelcomeStepHeader {headline} {body}>{@render details?.()}</WelcomeStepHeader>
  </div>
  <div class="mx-auto flex min-h-0 w-full max-w-xl flex-1 flex-col px-4 py-6 @min-[560px]:px-8">
    {@render children()}
  </div>
</section>
