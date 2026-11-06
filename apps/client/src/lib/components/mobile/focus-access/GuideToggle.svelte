<script lang="ts">
  import { cn } from "$lib/utils";
  import GuideTap from "./GuideTap.svelte";

  /** A drawn switch in a guide scene, styled like the app's own switch. */
  let { on, tapping = false }: { on: boolean; tapping?: boolean } = $props();
</script>

<span
  class={cn(
    "relative inline-flex h-5 w-8 shrink-0 items-center rounded-full border p-0.5 transition-colors duration-300 motion-reduce:transition-none",
    on ? "border-primary bg-primary" : "border-border bg-foreground/10",
  )}
>
  <span
    class={cn(
      "block size-3.5 rounded-full bg-background transition-transform duration-300 motion-reduce:transition-none",
      on ? "translate-x-3" : "translate-x-0",
    )}
  ></span>
  <GuideTap active={tapping} />
</span>
