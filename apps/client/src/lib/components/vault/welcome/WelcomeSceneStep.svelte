<script lang="ts">
  import type { Snippet } from "svelte";
  import { cn } from "$lib/utils";
  import { WELCOME_COLUMN_CLASS, WELCOME_TOP_PADDING_CLASS } from "$lib/vault/welcome-layout";
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
  <div class={cn(WELCOME_COLUMN_CLASS, WELCOME_TOP_PADDING_CLASS, "shrink-0")}>
    <WelcomeStepHeader {headline} {body}>{@render details?.()}</WelcomeStepHeader>
  </div>
  <div class={cn(WELCOME_COLUMN_CLASS, "flex min-h-0 flex-1 flex-col py-6")}>
    {@render children()}
  </div>
</section>
