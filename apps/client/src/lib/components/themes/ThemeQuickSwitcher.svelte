<script lang="ts">
  import { onMount, tick } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import Moon from "@lucide/svelte/icons/moon";
  import Sun from "@lucide/svelte/icons/sun";
  import X from "@lucide/svelte/icons/x";
  import { themeDisplayName } from "$lib/i18n/theme-labels";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import type { ThemeId } from "$lib/themes";
  import ThemeMiniPreview from "./ThemeMiniPreview.svelte";
  import { cn } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  let { onClose }: { onClose: () => void } = $props();

  const themeStore = getTheme();
  const { t } = getLocalization();
  const originalId = themeStore.id;

  let selectedId = $state<ThemeId>(themeStore.id);
  let committed = false;
  let optionEls: HTMLButtonElement[] = $state([]);

  const orderedThemes = $derived.by(() => {
    const all = Object.values(themeStore.registry);
    return [
      ...all.filter((t) => themeStore.isBuiltin(t.id)),
      ...all.filter((t) => !themeStore.isBuiltin(t.id)),
    ];
  });
  const selectedIndex = $derived(
    orderedThemes.findIndex((theme) => theme.id === selectedId),
  );

  async function focusIndex(index: number): Promise<void> {
    await tick();
    optionEls[index]?.focus();
    optionEls[index]?.scrollIntoView({ block: "nearest" });
  }

  function previewTheme(id: ThemeId): void {
    selectedId = id;
    themeStore.setTheme(id);
  }

  function moveSelection(delta: number): void {
    if (orderedThemes.length === 0) return;
    const current = selectedIndex >= 0 ? selectedIndex : 0;
    const next = (current + delta + orderedThemes.length) % orderedThemes.length;
    previewTheme(orderedThemes[next].id);
    void focusIndex(next);
  }

  function jumpSelection(index: number): void {
    const theme = orderedThemes[index];
    if (!theme) return;
    previewTheme(theme.id);
    void focusIndex(index);
  }

  function commitSelection(id: ThemeId = selectedId): void {
    committed = true;
    themeStore.setTheme(id);
    onClose();
  }

  function cancelSelection(): void {
    if (!committed) themeStore.setTheme(originalId);
    onClose();
  }

  function handleKeydown(e: KeyboardEvent): void {
    e.stopPropagation();
    if (e.key === "ArrowDown") {
      e.preventDefault();
      moveSelection(1);
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      moveSelection(-1);
      return;
    }
    if (e.key === "Home") {
      e.preventDefault();
      jumpSelection(0);
      return;
    }
    if (e.key === "End") {
      e.preventDefault();
      jumpSelection(orderedThemes.length - 1);
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      commitSelection();
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      cancelSelection();
    }
  }

  onMount(() => {
    void focusIndex(Math.max(0, selectedIndex));
    window.addEventListener("keydown", handleKeydown, true);
    return () => window.removeEventListener("keydown", handleKeydown, true);
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="fixed inset-0 z-70 flex items-start justify-center px-2 pt-[calc(var(--titlebar-h)+1.25rem)]"
  onclick={(e) => {
    e.stopPropagation();
    cancelSelection();
  }}
>
  <div
    role="dialog"
    aria-modal="true"
    aria-label={t("settings.theme.pickerLabel")}
    tabindex="-1"
    class="flex max-h-[min(26rem,calc(100dvh-var(--titlebar-h)-2.5rem))] w-[min(26rem,calc(100vw-1rem))] flex-col overflow-hidden surface-floating"
    onclick={(e) => e.stopPropagation()}
  >
    <header class="flex shrink-0 items-center justify-between gap-3 border-b border-border/70 px-3 py-2">
      <h2 class="truncate font-medium text-foreground">{t("settings.theme.pickerTitle")}</h2>
      <button
        type="button"
        onclick={cancelSelection}
        aria-label={t("settings.theme.closePicker")}
        data-app-tooltip-disabled="true"
        class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
      >
        <X size={14} strokeWidth={2} />
      </button>
    </header>

    <div role="listbox" aria-label={t("settings.theme.themesHeading")} class="surface-floating-body min-h-0 flex-1 overflow-y-auto" use:scrollEdgeFadeAction>
      {#each orderedThemes as item, index (item.id)}
        {@const BaseIcon = item.iconLabel === "dark" ? Moon : Sun}
        {@const selected = item.id === selectedId}
        <button
          bind:this={optionEls[index]}
          type="button"
          role="option"
          aria-selected={selected}
          onclick={() => commitSelection(item.id)}
          data-highlighted={selected ? "" : undefined}
          onpointerenter={() => previewTheme(item.id)}
          class="menu-item text-foreground"
        >
          <BaseIcon
            size={14}
            strokeWidth={1.75}
            class="shrink-0 text-muted-foreground"
          />
          <span class="min-w-0 flex-1 truncate">
            {themeDisplayName(item, t)}
          </span>
          <ThemeMiniPreview theme={item} />
          <Check
            size={14}
            strokeWidth={2}
            aria-hidden="true"
            class={cn("shrink-0", item.id !== originalId && "invisible")}
          />
        </button>
      {/each}
    </div>
  </div>
</div>
