<script lang="ts">
  import { onMount } from "svelte";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";

  let { value = $bindable(""), label, placeholder = label, clearLabel, onClose, inline = false, focusOnMount = true }:
    { value?: string; label: string; placeholder?: string; clearLabel: string; onClose?: () => void; inline?: boolean; focusOnMount?: boolean } = $props();
  let input: HTMLInputElement;

  onMount(() => { if (focusOnMount) input.focus(); });
</script>

<div class={inline ? "min-w-0 flex-1" : "shrink-0 px-2 pb-2"}>
  <div class="explorer-search field flex items-center gap-1.5 py-0 text-muted-foreground">
    <Search size={14} aria-hidden="true" />
    <input
      bind:this={input}
      class="field-bare self-stretch text-foreground"
      bind:value
      type="search"
      aria-label={label}
      {placeholder}
      onkeydown={(event) => {
        if (event.key !== "Escape") return;
        event.stopPropagation();
        if (value) value = "";
        else onClose?.();
      }}
    />
    {#if value}
      <button type="button" class="explorer-icon" aria-label={clearLabel} data-app-tooltip={clearLabel} onclick={() => { value = ""; input.focus(); }}><X size={14} /></button>
    {/if}
  </div>
</div>
