<script lang="ts">
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import type { Snippet } from "svelte";
  import { anchoredPopover } from "$lib/utils/anchored-popover";
  import { portal } from "$lib/utils/portal";
  import MusicSoundscapeIcon from "./MusicSoundscapeIcon.svelte";

  /**
   * Builder option for a soundscape or layer: icon at the left, only as wide as its name, with an optional actions
   * menu that stays visible so touch layouts can reach it.
   */
  let {
    icon,
    label,
    active = false,
    muted = false,
    ariaLabel,
    title,
    disabled = false,
    onclick,
    menuLabel,
    menu,
  }: {
    icon: string;
    label: string;
    /** Whether the option plays now. */
    active?: boolean;
    /** Whether the option needs attention, such as a layer whose file is missing. */
    muted?: boolean;
    ariaLabel: string;
    title?: string;
    disabled?: boolean;
    onclick: () => void;
    menuLabel?: string;
    /** Menu items; each receives `close` to call before acting. */
    menu?: Snippet<[close: () => void]>;
  } = $props();

  let menuButton = $state<HTMLButtonElement | null>(null);
  let menuOpen = $state(false);

  $effect(() => {
    if (!menuOpen) return;
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      closeMenu();
      menuButton?.focus();
    };
    window.addEventListener("keydown", escape, true);
    return () => window.removeEventListener("keydown", escape, true);
  });

  function closeMenu(): void {
    menuOpen = false;
  }
</script>

<div class={["inline-flex h-8 min-w-0 max-w-full items-center rounded-floating-item transition-colors hover:bg-accent/70", active && "bg-primary/10", muted && "text-muted-foreground"]}>
  <button type="button" aria-pressed={active} aria-label={ariaLabel} {title} {disabled} class={["inline-flex h-full min-w-0 items-center gap-2 pl-2 disabled:opacity-50", menu ? "pr-1" : "pr-2"]} {onclick}>
    <span class="shrink-0"><MusicSoundscapeIcon {icon} size={14} /></span>
    <span class="truncate">{label}</span>
  </button>
  {#if menu}
    <button bind:this={menuButton} type="button" aria-label={menuLabel} aria-haspopup="menu" aria-expanded={menuOpen} class="grid h-full w-6 shrink-0 place-items-center text-muted-foreground hover:text-foreground" onclick={() => menuOpen = !menuOpen}><Ellipsis size={14} /></button>
  {/if}
</div>
{#if menu && menuOpen}
  <div use:portal use:anchoredPopover={{ anchor: menuButton ?? undefined, onOutside: closeMenu, horizontalAlign: "end" }} class="surface-floating fixed z-90 flex min-w-36 flex-col overflow-hidden" role="menu" aria-label={menuLabel} data-app-floating-surface>
    <div class="surface-floating-body flex flex-col">{@render menu(closeMenu)}</div>
  </div>
{/if}
