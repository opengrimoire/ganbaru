<script lang="ts">
  import GripVertical from "@lucide/svelte/icons/grip-vertical";

  let {
    showIcon,
  }: {
    showIcon: boolean;
  } = $props();
</script>

{#if showIcon}
  <div
    class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-muted-foreground opacity-40"
    aria-hidden="true"
  >
    <GripVertical size={13} strokeWidth={1.75} />
  </div>
{:else}
  <div class="h-7 w-7 shrink-0" aria-hidden="true"></div>
{/if}
