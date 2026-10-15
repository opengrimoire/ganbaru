<script lang="ts">
  import { onMount } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { availableAppViews, type View } from "$lib/navigation";
  import { getDetachedWindows } from "$lib/stores/detached-windows.svelte";
  import { getWorkspaceTabs } from "$lib/stores/workspace-tabs.svelte";
  import { anchoredPopover } from "$lib/utils/anchored-popover";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import {
    workspaceTabFullLabel,
    workspaceTabKindIcon,
    workspaceTabKindLabel,
  } from "$lib/workspace-tabs/presentation";

  let {
    anchor,
    showOpenTabs,
    onClose,
  }: {
    anchor: HTMLElement | undefined;
    /** List the open tabs too, for when the strip cannot show them all. */
    showOpenTabs: boolean;
    onClose: () => void;
  } = $props();

  const tabs = getWorkspaceTabs();
  const detachedWindows = getDetachedWindows();
  const { t } = getLocalization();
  const kinds = availableAppViews();

  let menu: HTMLDivElement | undefined = $state();

  function menuItems(): HTMLButtonElement[] {
    return Array.from(menu?.querySelectorAll<HTMLButtonElement>("[data-launcher-item]:not(:disabled)") ?? []);
  }

  function moveFocus(step: number | "first" | "last"): void {
    const items = menuItems();
    if (items.length === 0) return;
    const current = items.findIndex((item) => item === document.activeElement);
    const next = step === "first"
      ? 0
      : step === "last"
        ? items.length - 1
        : (Math.max(current, step > 0 ? -1 : 0) + step + items.length) % items.length;
    items[next]?.focus();
  }

  function finish(): void {
    onClose();
    anchor?.focus();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      finish();
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      moveFocus(1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      moveFocus(-1);
    } else if (event.key === "Home") {
      event.preventDefault();
      moveFocus("first");
    } else if (event.key === "End") {
      event.preventDefault();
      moveFocus("last");
    } else {
      const kind = kindForDigit(event);
      if (!kind) return;
      event.preventDefault();
      openKind(kind);
    }
  }

  /**
   * The kind a digit key opens, numbered from 1 in launcher order. The physical key counts too, so layouts that
   * type digits with Shift still work.
   */
  function kindForDigit(event: KeyboardEvent): View | null {
    if (event.ctrlKey || event.altKey || event.metaKey) return null;
    const digit = /^[1-9]$/.test(event.key) ? event.key : /^(?:Digit|Numpad)([1-9])$/.exec(event.code)?.[1];
    return digit ? kinds[Number(digit) - 1] ?? null : null;
  }

  function openKind(kind: View): void {
    onClose();
    tabs.openKind(kind);
  }

  function activate(id: string): void {
    onClose();
    tabs.activate(id);
  }

  onMount(() => {
    moveFocus("first");
  });
</script>

<div
  bind:this={menu}
  use:portal
  use:anchoredPopover={{ anchor, onOutside: onClose, horizontalAlign: "start" }}
  class="surface-floating fixed z-90 flex w-floating-sm flex-col overflow-hidden"
  role="menu"
  tabindex="-1"
  aria-label={t("titleBar.tab.launcher")}
  data-app-floating-surface
  onkeydown={handleKeydown}
>
  <div class="surface-floating-body flex min-h-0 flex-1 flex-col overflow-y-auto overflow-x-hidden" use:scrollEdgeFadeAction>
    <p class="menu-label">{t("titleBar.tab.newTab")}</p>
    {#each kinds as kind, index (kind)}
      {@const Icon = workspaceTabKindIcon(kind)}
      {@const detached = detachedWindows.views.has(kind)}
      {@const key = index < 9 ? String(index + 1) : null}
      <button
        type="button"
        role="menuitem"
        class="menu-item"
        data-launcher-item
        aria-keyshortcuts={key ?? undefined}
        onclick={() => openKind(kind)}
      >
        <Icon class="text-muted-foreground" />
        <span class="min-w-0 truncate">{workspaceTabKindLabel(kind, t)}</span>
        {#if key}<span class="shrink-0 text-panel-detail text-muted-foreground">{t("titleBar.tab.pressKey", key)}</span>{/if}
        <span class="flex-1"></span>
        {#if detached}
          <ExternalLink class="size-3.5 shrink-0 text-muted-foreground" aria-label={t("titleBar.tab.inAnotherWindow")} />
        {/if}
      </button>
    {/each}

    {#if showOpenTabs && tabs.tabs.length > 0}
      <div role="separator" class="menu-separator"></div>
      <p class="menu-label">{t("titleBar.tab.openTabs")}</p>
      {#each tabs.tabs as tab (tab.id)}
        {@const Icon = workspaceTabKindIcon(tab.location.kind)}
        {@const active = tab.id === tabs.activeTabId}
        <button
          type="button"
          role="menuitemradio"
          aria-checked={active}
          class="menu-item"
          data-launcher-item
          onclick={() => activate(tab.id)}
        >
          <Icon class="text-muted-foreground" />
          <span class="min-w-0 flex-1 truncate">{workspaceTabFullLabel(tab, t)}</span>
          {#if active}<Check class="size-3.5 shrink-0" aria-hidden="true" />{/if}
        </button>
      {/each}
    {/if}
  </div>
</div>
