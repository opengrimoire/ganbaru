<script lang="ts">
  import type { Snippet } from "svelte";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getMobileIdentitySlot, mobileIdentityPortal } from "$lib/mobile/identity-slot";
  import { cn } from "$lib/utils";

  let {
    title,
    expanded,
    onclick,
    icon,
    triggerElement = $bindable(null),
    class: className = "",
  }: {
    /** Visible name of the current project, channel, or page. */
    title: string;
    expanded: boolean;
    onclick: (event: MouseEvent) => void;
    icon?: Snippet;
    triggerElement?: HTMLButtonElement | null;
    class?: string;
  } = $props();

  const { t } = getLocalization();
  const identitySlot = getMobileIdentitySlot();
</script>

<!-- The phone top bar shows one context switcher for the active destination; it opens that destination's navigator. -->
<div use:mobileIdentityPortal={identitySlot?.target ?? null} class={cn("flex h-full min-w-0 max-w-full items-center", className)}>
  <button
    bind:this={triggerElement}
    type="button"
    data-mobile-context-switcher
    aria-label={t("mobile.contextSwitcher", title)}
    aria-haspopup="dialog"
    aria-expanded={expanded}
    {onclick}
    class={cn(
      "flex h-9 min-w-0 max-w-full items-center gap-2 rounded-full pr-2.5 pl-2 text-left text-identity font-medium text-foreground transition-colors active:bg-accent",
      expanded ? "bg-accent" : "bg-accent/55",
    )}
  >
    {#if icon}<span class="grid size-5 shrink-0 place-items-center" aria-hidden="true">{@render icon()}</span>{/if}
    <span class="min-w-0 truncate">{title}</span>
    <ChevronDown size={16} class="shrink-0 text-muted-foreground" aria-hidden="true" />
  </button>
</div>
