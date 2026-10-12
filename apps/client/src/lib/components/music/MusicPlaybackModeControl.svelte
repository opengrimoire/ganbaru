<script lang="ts">
  import { onMount } from "svelte";
  import ListOrdered from "@lucide/svelte/icons/list-ordered";
  import Shuffle from "@lucide/svelte/icons/shuffle";
  import Dice5 from "@lucide/svelte/icons/dice-5";
  import Check from "@lucide/svelte/icons/check";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicPlaybackMode } from "$lib/music/library/contracts";
  import { getMusicPlayer } from "$lib/stores/music-player.svelte";
  import { cn } from "$lib/utils";

  let { className = "", align = "start" }: { className?: string; align?: "start" | "end" } = $props();
  const { t } = getLocalization();
  const player = getMusicPlayer();
  let root = $state<HTMLElement | null>(null);
  let open = $state(false);
  const modes: MusicPlaybackMode[] = ["in-order", "shuffle", "mix"];

  onMount(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && !root?.contains(event.target)) open = false;
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && open) {
        event.stopPropagation();
        open = false;
        root?.querySelector("button")?.focus();
      }
    };
    window.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown, true);
    };
  });

  function select(mode: MusicPlaybackMode): void {
    player.setPlaybackMode(mode);
    open = false;
    root?.querySelector("button")?.focus();
  }
</script>

<div bind:this={root} class={cn("relative", className)}>
  <button
    type="button"
    onclick={() => open = !open}
    disabled={player.queue.length === 0}
    class="inline-flex h-9 w-9 items-center justify-center rounded-md bg-secondary text-secondary-foreground transition-colors disabled:pointer-events-none disabled:opacity-50"
    aria-label={t("music.playbackMode.current", t(`music.playbackMode.${player.playbackMode}`))}
    aria-haspopup="menu"
    aria-expanded={open}
  >
    {#if player.playbackMode === "mix"}<Dice5 size={14} strokeWidth={1.4} />
    {:else if player.playbackMode === "shuffle"}<Shuffle size={14} strokeWidth={1.4} />
    {:else}<ListOrdered size={14} strokeWidth={1.4} />{/if}
  </button>
  {#if open}
    <div class={cn("surface-floating surface-floating-body absolute bottom-full z-50 mb-2 min-w-40", align === "end" ? "right-0" : "left-0")} role="menu" aria-label={t("music.playbackMode.label")}>
      {#each modes as mode}
        <div class="flex items-center gap-0.5">
          <button type="button" role="menuitemradio" aria-checked={player.playbackMode === mode} onclick={() => select(mode)} class="menu-item min-w-0 flex-1">
            {#if mode === "mix"}<Dice5 size={16} />{:else if mode === "shuffle"}<Shuffle size={16} />{:else}<ListOrdered size={16} />{/if}
            <span class="flex-1">{t(`music.playbackMode.${mode}`)}</span>
            {#if player.playbackMode === mode}<Check size={15} />{/if}
          </button>
          {#if mode !== "in-order"}
            <span class="group relative shrink-0">
              <button type="button" class="grid h-6 w-6 place-items-center rounded-full text-[0.7rem] font-semibold text-muted-foreground hover:bg-secondary hover:text-foreground focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring" aria-label={t(`music.playbackMode.${mode}Help`)}>?</button>
              <span aria-hidden="true" class="surface-floating pointer-events-none invisible absolute bottom-full right-0 z-60 mb-1 w-52 px-2.5 py-2 text-left text-panel-detail opacity-0 transition-opacity group-hover:visible group-hover:opacity-100 group-focus-within:visible group-focus-within:opacity-100">{t(`music.playbackMode.${mode}Help`)}</span>
            </span>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
