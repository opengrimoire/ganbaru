<script lang="ts">
  import { cn } from "$lib/utils";

  /** A placeholder text line for the parts of a system screen that the guide does not need to spell out. */
  let { class: className = "w-16" }: { class?: string } = $props();
</script>

<span class={cn("block h-1.5 rounded-full bg-foreground/12", className)}></span>
