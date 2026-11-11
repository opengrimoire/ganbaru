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
    class="flex size-(--control-height) shrink-0 items-center justify-center rounded-md text-muted-foreground opacity-40"
    aria-hidden="true"
  >
    <GripVertical size={13} strokeWidth={1.75} />
  </div>
{:else}
  <div class="size-(--control-height) shrink-0" aria-hidden="true"></div>
{/if}
