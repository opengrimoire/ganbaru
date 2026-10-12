<script lang="ts">
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Plus from "@lucide/svelte/icons/plus";

  let {
    kind,
    context = null,
    class: className = undefined,
  }: {
    kind: "chevron" | "plus";
    context?: "chat" | "notes" | null;
    class?: string;
  } = $props();

  const size = 14;
  const strokeWidth = 2;
</script>

{#if kind === "chevron"}
  <ChevronDown
    {size}
    {strokeWidth}
    class={className}
    style={`stroke-width:${strokeWidth} !important`}
    data-workspace-breadcrumb-terminal-icon="chevron"
    data-chat-context-chevron={context === "chat" ? true : undefined}
    data-notes-context-chevron={context === "notes" ? true : undefined}
  />
{:else}
  <Plus
    {size}
    {strokeWidth}
    class={className}
    style={`stroke-width:${strokeWidth} !important`}
    data-workspace-breadcrumb-terminal-icon="plus"
  />
{/if}
