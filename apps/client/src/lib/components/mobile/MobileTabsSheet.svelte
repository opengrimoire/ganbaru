<script lang="ts">
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import Lock from "@lucide/svelte/icons/lock";
  import LockOpen from "@lucide/svelte/icons/lock-open";
  import Plus from "@lucide/svelte/icons/plus";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import X from "@lucide/svelte/icons/x";
  import ProjectPickerMobileDialog from "$lib/components/projects/pickers/ProjectPickerMobileDialog.svelte";
  import MoveStepMenuItems from "$lib/components/ui/MoveStepMenuItems.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { availableAppViews, type View } from "$lib/navigation";
  import type { WorkspaceTabs } from "$lib/stores/workspace-tabs.svelte";
  import { cn } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { workspaceTabFullLabel, workspaceTabKindIcon, workspaceTabKindLabel } from "$lib/workspace-tabs/presentation";

  /** The workspace tab operations the sheet uses. */
  export type MobileTabsSheetTabs = Pick<
    WorkspaceTabs,
    "tabs" | "activeTabId" | "closed" | "activate" | "openKind" | "close" | "closeOthers" | "reopen" | "setLocked" | "move"
  >;

  /**
   * Every open tab on the phone shell, opened from the tab bar: switch, reorder, lock, or close tabs, open a new
   * one of any kind, or reopen a recently closed one. The only tab has no close, since closing it would leave the
   * app with nothing to show.
   */
  let {
    tabs,
    onClose,
  }: {
    tabs: MobileTabsSheetTabs;
    onClose: () => void;
  } = $props();

  const { t } = getLocalization();
  const kinds = availableAppViews();
  const iconStrokeWidth = 1.6;
  let menuTabId = $state<string | null>(null);

  const canClose = $derived(tabs.tabs.length > 1);

  function select(id: string): void {
    tabs.activate(id);
    onClose();
  }

  function openKind(kind: View): void {
    tabs.openKind(kind);
    onClose();
  }

  function reopen(index: number): void {
    tabs.reopen(index);
    onClose();
  }

  function moveTab(id: string, step: -1 | 1): void {
    const index = tabs.tabs.findIndex((tab) => tab.id === id);
    const target = tabs.tabs[index + step];
    if (index < 0 || !target) return;
    tabs.move(id, target.id, step < 0 ? "before" : "after");
  }

  function toggleMenu(id: string): void {
    menuTabId = menuTabId === id ? null : id;
  }

  function closeMenu(): void {
    menuTabId = null;
  }
</script>

<ProjectPickerMobileDialog label={t("titleBar.tab.openTabs")} closeLabel={t("common.close")} {onClose}>
  <div class="surface-floating flex h-full min-h-0 flex-col overflow-hidden rounded-2xl" data-mobile-tabs-sheet>
    <div class="flex min-h-14 shrink-0 items-center gap-1 border-b border-border/70 p-2">
      <h2 class="min-w-0 flex-1 truncate px-2 text-base font-semibold">{t("titleBar.tab.openTabs")}</h2>
      <button
        type="button"
        data-project-picker-initial-focus="true"
        class="flex min-h-12 min-w-12 items-center justify-center rounded-xl active:bg-accent"
        aria-label={t("common.close")}
        onclick={onClose}
      >
        <X size={22} strokeWidth={iconStrokeWidth} aria-hidden="true" />
      </button>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto overflow-x-hidden p-2" use:scrollEdgeFadeAction>
      <ul class="flex flex-col gap-0.5">
        {#each tabs.tabs as tab, index (tab.id)}
          {@const Icon = workspaceTabKindIcon(tab.location.kind)}
          {@const active = tab.id === tabs.activeTabId}
          {@const label = workspaceTabFullLabel(tab, t)}
          {@const menuOpen = menuTabId === tab.id}
          <li data-mobile-tab-row={tab.id}>
            <div class={cn("flex min-h-12 items-center rounded-xl", active && "bg-accent text-accent-foreground")}>
              <button
                type="button"
                class="flex min-h-12 min-w-0 flex-1 items-center gap-3 rounded-xl px-3 text-left text-base active:bg-accent/70"
                aria-current={active ? "page" : undefined}
                aria-label={tab.locked ? t("titleBar.tab.lockedTab", label) : label}
                onclick={() => select(tab.id)}
              >
                <Icon class="shrink-0" size={20} strokeWidth={active ? 2 : iconStrokeWidth} aria-hidden="true" />
                <span class="min-w-0 flex-1 truncate">{label}</span>
                {#if tab.locked}<Lock class="shrink-0 opacity-60" size={14} strokeWidth={iconStrokeWidth} aria-hidden="true" />{/if}
              </button>
              <button
                type="button"
                class="flex min-h-12 min-w-12 shrink-0 items-center justify-center rounded-xl active:bg-accent/70"
                aria-label={t("titleBar.tab.tabActions", label)}
                aria-haspopup="menu"
                aria-expanded={menuOpen}
                onclick={() => toggleMenu(tab.id)}
              >
                <Ellipsis size={20} strokeWidth={iconStrokeWidth} aria-hidden="true" />
              </button>
              {#if canClose}
                <button
                  type="button"
                  class="flex min-h-12 min-w-12 shrink-0 items-center justify-center rounded-xl active:bg-accent/70"
                  aria-label={t("titleBar.tab.closeTab")}
                  data-mobile-tab-close
                  onclick={() => tabs.close(tab.id)}
                >
                  <X size={20} strokeWidth={iconStrokeWidth} aria-hidden="true" />
                </button>
              {/if}
            </div>
            {#if menuOpen}
              <div class="ml-8 flex flex-col py-1" role="menu" aria-label={t("titleBar.tab.tabActions", label)}>
                <MoveStepMenuItems
                  canMoveUp={index > 0}
                  canMoveDown={index < tabs.tabs.length - 1}
                  onMove={(step) => moveTab(tab.id, step)}
                  onClose={() => undefined}
                />
                <button
                  type="button"
                  role="menuitem"
                  class="menu-item"
                  onclick={() => { tabs.setLocked(tab.id, !tab.locked); closeMenu(); }}
                >
                  {#if tab.locked}<LockOpen />{:else}<Lock />{/if}
                  <span>{tab.locked ? t("titleBar.tab.unlockTab") : t("titleBar.tab.lockTab")}</span>
                </button>
                {#if canClose}
                  <button
                    type="button"
                    role="menuitem"
                    class="menu-item"
                    onclick={() => { tabs.closeOthers(tab.id); closeMenu(); }}
                  >
                    <X />
                    <span>{t("titleBar.tab.closeOtherTabs")}</span>
                  </button>
                {/if}
              </div>
            {/if}
          </li>
        {/each}
      </ul>

      <p class="menu-label mt-2">{t("titleBar.tab.newTab")}</p>
      <ul class="flex flex-col gap-0.5">
        {#each kinds as kind (kind)}
          {@const Icon = workspaceTabKindIcon(kind)}
          <li>
            <button
              type="button"
              class="flex min-h-12 w-full items-center gap-3 rounded-xl px-3 text-left text-base active:bg-accent/70"
              data-mobile-new-tab={kind}
              onclick={() => openKind(kind)}
            >
              <Icon class="shrink-0 text-muted-foreground" size={20} strokeWidth={iconStrokeWidth} aria-hidden="true" />
              <span class="min-w-0 flex-1 truncate">{workspaceTabKindLabel(kind, t)}</span>
              <Plus class="shrink-0 text-muted-foreground" size={18} strokeWidth={iconStrokeWidth} aria-hidden="true" />
            </button>
          </li>
        {/each}
      </ul>

      {#if tabs.closed.length > 0}
        <p class="menu-label mt-2">{t("titleBar.tab.recentlyClosed")}</p>
        <ul class="flex flex-col gap-0.5">
          {#each tabs.closed as entry, index (index)}
            {@const Icon = workspaceTabKindIcon(entry.location.kind)}
            <li>
              <button
                type="button"
                class="flex min-h-12 w-full items-center gap-3 rounded-xl px-3 text-left text-base active:bg-accent/70"
                data-mobile-closed-tab
                onclick={() => reopen(index)}
              >
                <Icon class="shrink-0 text-muted-foreground" size={20} strokeWidth={iconStrokeWidth} aria-hidden="true" />
                <span class="min-w-0 flex-1 truncate">{workspaceTabFullLabel(entry, t)}</span>
                <RotateCcw class="shrink-0 text-muted-foreground" size={18} strokeWidth={iconStrokeWidth} aria-hidden="true" />
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>
</ProjectPickerMobileDialog>
