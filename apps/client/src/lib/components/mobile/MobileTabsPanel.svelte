<script lang="ts">
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Lock from "@lucide/svelte/icons/lock";
  import Plus from "@lucide/svelte/icons/plus";
  import X from "@lucide/svelte/icons/x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { availableAppViews, type View } from "$lib/navigation";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import type { WorkspaceTabs } from "$lib/stores/workspace-tabs.svelte";
  import { cn } from "$lib/utils";
  import { dismissOnOutside, type DismissOnOutsideReason } from "$lib/utils/dismiss-on-outside";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { createSortableDrag } from "$lib/utils/sortable-drag.svelte";
  import { workspaceTabFullLabel, workspaceTabKindIcon, workspaceTabKindLabel } from "$lib/workspace-tabs/presentation";

  /** The workspace tab operations the panel uses. */
  export type MobileTabsPanelTabs = Pick<WorkspaceTabs, "tabs" | "activeTabId" | "activate" | "openKind" | "close" | "move">;

  /**
   * Every open tab on the phone shell, in a panel that rises from the tab bar over the content without blocking it:
   * switch or close tabs, or open the new tab level to add one of any kind. Holding a row lifts it for reordering while a
   * swipe scrolls. Stepwise moves, locking, and closing other tabs stay on desktop, so each row keeps a single action. A
   * press outside the panel, Back, or Escape closes it; the tabs button that toggles it is left to do so itself. The only
   * tab has no close, since closing it would leave the app with nothing to show.
   */
  let {
    tabs,
    onClose,
  }: {
    tabs: MobileTabsPanelTabs;
    onClose: () => void;
  } = $props();

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  const kinds = availableAppViews();
  const iconStrokeWidth = 1.6;
  let level = $state<"tabs" | "new">("tabs");

  const canClose = $derived(tabs.tabs.length > 1);
  const tabIds = (): string[] => tabs.tabs.map((tab) => tab.id);

  const drag = createSortableDrag({
    getIds: tabIds,
    commit: (id, targetIndex) => {
      const ids = tabIds();
      const source = ids.indexOf(id);
      const target = ids[targetIndex];
      if (source < 0 || !target || target === id) return;
      tabs.move(id, target, targetIndex > source ? "after" : "before");
    },
    touchHold: true,
  });

  $effect(() => mobileBackStack.activate({ handle: onClose }));
  $effect(() => {
    if (level !== "new") return;
    return mobileBackStack.activate({ handle: () => { level = "tabs"; } });
  });
  $effect(() => () => drag.reset());

  function dismiss(reason: DismissOnOutsideReason, event: Event): void {
    if (reason === "outside-pointer" && event.target instanceof Element && event.target.closest("[data-mobile-tabs-button]")) return;
    onClose();
  }

  function select(id: string): void {
    tabs.activate(id);
    onClose();
  }

  function openKind(kind: View): void {
    tabs.openKind(kind);
    onClose();
  }
</script>

<div class="pointer-events-none absolute inset-2 z-50 flex flex-col items-center justify-end">
  <div
    class="surface-floating pointer-events-auto flex max-h-full min-h-0 w-full max-w-xl flex-col overflow-hidden rounded-2xl"
    role="dialog"
    aria-label={level === "new" ? t("titleBar.tab.newTab") : t("titleBar.tab.openTabs")}
    use:dismissOnOutside={{ onDismiss: dismiss }}
    data-mobile-tabs-panel
  >
    {#if level === "tabs"}
      <div class="min-h-0 overflow-x-hidden overflow-y-auto p-2" use:scrollEdgeFadeAction>
        <ul class="flex flex-col gap-0.5">
          {#each tabs.tabs as tab (tab.id)}
            {@const Icon = workspaceTabKindIcon(tab.location.kind)}
            {@const active = tab.id === tabs.activeTabId}
            {@const label = workspaceTabFullLabel(tab, t)}
            <li {@attach drag.item(tab.id)} data-mobile-tab-row={tab.id}>
              <div class={cn("flex min-h-12 items-center rounded-xl", active && "bg-accent text-accent-foreground")}>
                <button
                  type="button"
                  class="flex min-h-12 min-w-0 flex-1 items-center gap-3 rounded-xl px-3 text-left text-base active:bg-accent/70"
                  aria-current={active ? "page" : undefined}
                  aria-label={tab.locked ? t("titleBar.tab.lockedTab", label) : label}
                  onpointerdown={(event) => drag.pointerDown(event, tab.id)}
                  onclick={() => select(tab.id)}
                >
                  <Icon class="shrink-0" size={20} strokeWidth={active ? 2 : iconStrokeWidth} aria-hidden="true" />
                  <span class="min-w-0 flex-1 truncate">{label}</span>
                  {#if tab.locked}<Lock class="shrink-0 opacity-60" size={14} strokeWidth={iconStrokeWidth} aria-hidden="true" />{/if}
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
            </li>
          {/each}
        </ul>
      </div>
      <div class="shrink-0 border-t border-border/70 p-2">
        <button
          type="button"
          class="flex min-h-12 w-full items-center gap-3 rounded-xl px-3 text-left text-base active:bg-accent/70"
          data-mobile-new-tab-level
          onclick={() => { level = "new"; }}
        >
          <Plus class="shrink-0" size={20} strokeWidth={iconStrokeWidth} aria-hidden="true" />
          <span class="min-w-0 flex-1 truncate">{t("titleBar.tab.newTab")}</span>
          <ChevronRight class="shrink-0 text-muted-foreground" size={18} strokeWidth={iconStrokeWidth} aria-hidden="true" />
        </button>
      </div>
    {:else}
      <div class="flex min-h-14 shrink-0 items-center gap-1 border-b border-border/70 p-2">
        <button
          type="button"
          class="flex min-h-12 min-w-12 items-center justify-center rounded-xl active:bg-accent"
          aria-label={t("common.back")}
          data-mobile-new-tab-back
          onclick={() => { level = "tabs"; }}
        >
          <ChevronLeft size={22} strokeWidth={iconStrokeWidth} aria-hidden="true" />
        </button>
        <h2 class="min-w-0 flex-1 truncate px-1 text-base font-semibold">{t("titleBar.tab.newTab")}</h2>
      </div>
      <ul class="flex min-h-0 flex-col gap-0.5 overflow-y-auto p-2" use:scrollEdgeFadeAction>
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
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>
