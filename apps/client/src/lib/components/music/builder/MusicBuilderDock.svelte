<script lang="ts">
  import CloudRain from "@lucide/svelte/icons/cloud-rain";
  import ListChecks from "@lucide/svelte/icons/list-checks";
  import ListMusic from "@lucide/svelte/icons/list-music";
  import RadioTower from "@lucide/svelte/icons/radio-tower";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { projectMusicBuilderDockItems } from "$lib/music/builder/navigation";
  import type { MusicBuilderDestination, MusicBuilderPrimaryDestinationKind } from "$lib/music/builder/routing";

  let {
    destination,
    reviewCount,
    compact = false,
    showAllLabels = false,
    includeSoundscapes = true,
    onNavigate,
  }: {
    destination: MusicBuilderDestination;
    reviewCount: number;
    compact?: boolean;
    showAllLabels?: boolean;
    includeSoundscapes?: boolean;
    onNavigate: (destination: MusicBuilderDestination) => void;
  } = $props();

  const { t } = getLocalization();
  const items = $derived(projectMusicBuilderDockItems(destination, reviewCount, includeSoundscapes));

  function label(kind: MusicBuilderPrimaryDestinationKind): string {
    if (kind === "review") return t("music.builder.review");
    if (kind === "playlists") return t("music.builder.playlists");
    if (kind === "sources") return t("music.builder.sources");
    return t("music.builder.soundscapes");
  }

  function navigate(kind: MusicBuilderPrimaryDestinationKind): void {
    onNavigate({ kind });
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    const root = event.currentTarget instanceof HTMLElement ? event.currentTarget.closest("nav") : null;
    if (!(root instanceof HTMLElement)) return;
    const buttons = [...root.querySelectorAll<HTMLButtonElement>("[data-builder-dock-item]")];
    if (buttons.length === 0) return;
    event.preventDefault();
    const current = Math.max(0, buttons.indexOf(document.activeElement as HTMLButtonElement));
    const index = event.key === "Home" ? 0
      : event.key === "End" ? buttons.length - 1
      : event.key === "ArrowRight" ? (current + 1) % buttons.length
      : (current - 1 + buttons.length) % buttons.length;
    buttons[index]?.focus();
  }
</script>

<nav class:compact class:show-all-labels={showAllLabels} class="builder-dock flex h-11 min-w-0 items-center justify-center gap-1 px-1.5" aria-label={t("music.builder.compactNavigation")}>
  {#each items as item (item.kind)}
    {@const itemLabel = label(item.kind)}
    <button
      type="button"
      data-builder-dock-item
      class:active={item.active}
      class="dock-button relative inline-flex h-8 min-w-8 shrink-0 items-center justify-center gap-1.5 rounded-lg px-2 text-foreground hover:bg-accent/55 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/60"
      aria-current={item.active ? "page" : undefined}
      aria-label={`${itemLabel}, ${t("calendar.toolbar.shortcutKey", item.shortcut)}`}
      title={`${itemLabel} (${t("calendar.toolbar.shortcutKey", item.shortcut)})`}
      onclick={() => navigate(item.kind)}
      onkeydown={handleKeydown}
    >
      {#if item.kind === "review"}<ListChecks size={15} />
      {:else if item.kind === "playlists"}<ListMusic size={15} />
      {:else if item.kind === "sources"}<RadioTower size={15} />
      {:else}<CloudRain size={15} />{/if}
      {#if item.active || showAllLabels}<span class="active-label truncate text-[0.65rem] font-medium text-foreground">{itemLabel}</span>{/if}
      {#if item.badge !== null}<span class="dock-badge absolute -right-0.5 -top-0.5 min-w-4 rounded-full bg-secondary px-1 text-center text-[0.52rem] font-semibold leading-4 tabular-nums">{item.badge > 999 ? "999+" : item.badge}</span>{/if}
    </button>
  {/each}
</nav>

<style>
  .builder-dock { container-type: inline-size; border-top: 1px solid color-mix(in srgb, var(--border) 46%, transparent); }
  .dock-button.active { background: var(--secondary); color: var(--foreground); }
  .compact { border-top: 0; }
  @container (width < 340px) { .builder-dock:not(.show-all-labels) .active-label { display: none; } }
</style>
