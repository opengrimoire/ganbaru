<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";

  /** The back arrow and title that open a drawn Android settings screen. */
  let { title }: { title: string } = $props();
</script>

<div class="flex flex-col gap-2 px-3 pb-2 pt-3">
  <ArrowLeft class="size-3.5 text-muted-foreground" aria-hidden="true" />
  <div class="text-sm font-semibold leading-tight text-foreground">{title}</div>
</div>
