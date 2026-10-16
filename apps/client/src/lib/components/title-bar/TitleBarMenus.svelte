<script lang="ts" module>
  import type { View } from "$lib/navigation";
  import type { WorkspaceSplitOrientation } from "$lib/workspace-tabs/model";

  /**
   * What the tab context menu offers for split view: showing that tab beside the active one, opening a new tab beside
   * the active tab, or arranging the split the tab belongs to as it shows now.
   */
  export type TabSplitMenu =
    | { kind: "open" }
    | { kind: "open-new"; kinds: readonly View[]; blockReason: string | null }
    | { kind: "arrange"; orientation: WorkspaceSplitOrientation; turnBlockReason: string | null };
</script>

<script lang="ts">
  import ArrowLeftRight from "@lucide/svelte/icons/arrow-left-right";
  import ArrowUpDown from "@lucide/svelte/icons/arrow-up-down";
  import Check from "@lucide/svelte/icons/check";
  import Columns2 from "@lucide/svelte/icons/columns-2";
  import Lock from "@lucide/svelte/icons/lock";
  import LockOpen from "@lucide/svelte/icons/lock-open";
  import PanelBottomClose from "@lucide/svelte/icons/panel-bottom-close";
  import PanelLeftClose from "@lucide/svelte/icons/panel-left-close";
  import PanelRightClose from "@lucide/svelte/icons/panel-right-close";
  import PanelTopClose from "@lucide/svelte/icons/panel-top-close";
  import RectangleHorizontal from "@lucide/svelte/icons/rectangle-horizontal";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Rows2 from "@lucide/svelte/icons/rows-2";
  import X from "@lucide/svelte/icons/x";
  import SquareArrowUpRight from "@lucide/svelte/icons/square-arrow-up-right";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatShortcut } from "$lib/keyboard-shortcuts";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import type { TitleBarControlId } from "$lib/stores/preference-options";
  import { workspaceTabKindIcon, workspaceTabKindLabel } from "$lib/workspace-tabs/presentation";
  import TitleBarSubmenu from "./TitleBarSubmenu.svelte";

  let {
    showTabContextMenu = $bindable(),
    tabContextId = $bindable(),
    tabContextMenuStyle,
    moveToNewWindowBlockReason,
    canReopenTab,
    lockAction,
    splitMenu,
    showTitleBarMenu = $bindable(),
    titleBarMenuStyle,
    controls,
    onMoveToNewWindow,
    onCloseTab,
    onReopenTab,
    onToggleLock,
    onOpenInSplit,
    onOpenKindInSplit,
    onUnsplit,
    onCloseSplitSide,
    onTurnSplit,
    onSwapSplit,
    onToggleControl,
  }: {
    showTabContextMenu: boolean;
    tabContextId: string | null;
    tabContextMenuStyle: string;
    /** Why the tab cannot move to a new window, or null when it can. */
    moveToNewWindowBlockReason: string | null;
    canReopenTab: boolean;
    /** Whether the tab can be locked or unlocked, or null when it shows nothing to lock to. */
    lockAction: "lock" | "unlock" | null;
    /** Split view actions for the tab, or null when this window offers none. */
    splitMenu: TabSplitMenu | null;
    showTitleBarMenu: boolean;
    titleBarMenuStyle: string;
    controls: Array<{ id: TitleBarControlId; label: string }>;
    onMoveToNewWindow: () => void;
    onCloseTab: () => void;
    onReopenTab: () => void;
    onToggleLock: () => void;
    onOpenInSplit: () => void;
    onOpenKindInSplit: (kind: View) => void;
    onUnsplit: () => void;
    /** Close the first (left or top) or second side of the tab's split. */
    onCloseSplitSide: (side: 0 | 1) => void;
    /** Show the tab's split the other way, stacked or side by side. */
    onTurnSplit: () => void;
    onSwapSplit: () => void;
    onToggleControl: (id: TitleBarControlId) => void;
  } = $props();

  const preferences = getPreferences();
  const { t } = getLocalization();
</script>

{#if showTabContextMenu && tabContextId}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-40"
    onclick={() => { showTabContextMenu = false; tabContextId = null; }}
    oncontextmenu={(e) => { e.preventDefault(); showTabContextMenu = false; tabContextId = null; }}
  ></div>
  <div
    class="surface-floating fixed z-50 overflow-hidden"
    style={tabContextMenuStyle}
    role="menu"
  >
    <div class="surface-floating-body">
      <button role="menuitem" onclick={onCloseTab} class="menu-item">
        <X size={15} strokeWidth={2.2} />
        <span class="shrink-0 whitespace-nowrap">{t("titleBar.tab.closeTab")}</span>
      </button>
      <button role="menuitem" disabled={!canReopenTab} onclick={onReopenTab} class="menu-item">
        <RotateCcw size={15} strokeWidth={2.2} />
        <span class="shrink-0 whitespace-nowrap">{t("titleBar.tab.withShortcut", t("titleBar.tab.reopenClosedTab"), formatShortcut("Mod + Shift + T"))}</span>
      </button>
      {#if lockAction}
        <button role="menuitem" onclick={onToggleLock} class="menu-item">
          {#if lockAction === "lock"}
            <Lock size={15} strokeWidth={2.2} />
            <span class="shrink-0 whitespace-nowrap">{t("titleBar.tab.lockTab")}</span>
          {:else}
            <LockOpen size={15} strokeWidth={2.2} />
            <span class="shrink-0 whitespace-nowrap">{t("titleBar.tab.unlockTab")}</span>
          {/if}
        </button>
      {/if}
      <div role="separator" class="menu-separator"></div>
      {#if splitMenu?.kind === "open"}
        <button
          role="menuitem"
          onclick={onOpenInSplit}
          class="menu-item"
        >
          <Columns2 size={15} strokeWidth={2.2} />
          <span class="shrink-0 whitespace-nowrap">{t("titleBar.split.openInSplit")}</span>
        </button>
      {:else if splitMenu?.kind === "open-new"}
        <TitleBarSubmenu
          label={t("titleBar.split.openNewInSplit")}
          icon={Columns2}
          disabled={splitMenu.kinds.length === 0}
          title={splitMenu.blockReason ?? undefined}
        >
          {#each splitMenu.kinds as kind (kind)}
            {@const Icon = workspaceTabKindIcon(kind)}
            <button role="menuitem" onclick={() => onOpenKindInSplit(kind)} class="menu-item">
              <Icon size={15} strokeWidth={2.2} />
              <span class="shrink-0 whitespace-nowrap">{workspaceTabKindLabel(kind, t)}</span>
            </button>
          {/each}
        </TitleBarSubmenu>
      {:else if splitMenu?.kind === "arrange"}
        {@const sideBySide = splitMenu.orientation === "side-by-side"}
        <TitleBarSubmenu label={t("titleBar.split.arrange")} icon={Columns2}>
          <button role="menuitem" onclick={onUnsplit} class="menu-item">
            <RectangleHorizontal size={15} strokeWidth={2.2} />
            <span class="shrink-0 whitespace-nowrap">{t("titleBar.split.unsplit")}</span>
          </button>
          <button role="menuitem" onclick={() => onCloseSplitSide(0)} class="menu-item">
            {#if sideBySide}<PanelLeftClose size={15} strokeWidth={2.2} />{:else}<PanelTopClose size={15} strokeWidth={2.2} />{/if}
            <span class="shrink-0 whitespace-nowrap">{t(sideBySide ? "titleBar.split.closeLeft" : "titleBar.split.closeTop")}</span>
          </button>
          <button role="menuitem" onclick={() => onCloseSplitSide(1)} class="menu-item">
            {#if sideBySide}<PanelRightClose size={15} strokeWidth={2.2} />{:else}<PanelBottomClose size={15} strokeWidth={2.2} />{/if}
            <span class="shrink-0 whitespace-nowrap">{t(sideBySide ? "titleBar.split.closeRight" : "titleBar.split.closeBottom")}</span>
          </button>
          <div role="separator" class="menu-separator"></div>
          <button
            role="menuitem"
            disabled={splitMenu.turnBlockReason !== null}
            title={splitMenu.turnBlockReason ?? undefined}
            onclick={onTurnSplit}
            class="menu-item"
          >
            {#if sideBySide}<Rows2 size={15} strokeWidth={2.2} />{:else}<Columns2 size={15} strokeWidth={2.2} />{/if}
            <span class="shrink-0 whitespace-nowrap">{t(sideBySide ? "titleBar.split.showStacked" : "titleBar.split.showSideBySide")}</span>
          </button>
          <button role="menuitem" onclick={onSwapSplit} class="menu-item">
            {#if sideBySide}<ArrowLeftRight size={15} strokeWidth={2.2} />{:else}<ArrowUpDown size={15} strokeWidth={2.2} />{/if}
            <span class="shrink-0 whitespace-nowrap">{t("titleBar.split.swap")}</span>
          </button>
        </TitleBarSubmenu>
      {/if}
      <button
        role="menuitem"
        disabled={moveToNewWindowBlockReason !== null}
        title={moveToNewWindowBlockReason ?? undefined}
        onclick={onMoveToNewWindow}
        class="menu-item"
      >
        <SquareArrowUpRight size={15} strokeWidth={2.2} />
        <span class="shrink-0 whitespace-nowrap">{t("titleBar.moveToNewWindow")}</span>
      </button>
    </div>
  </div>
{/if}

{#if showTitleBarMenu}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-40"
    onclick={() => { showTitleBarMenu = false; }}
    oncontextmenu={(e) => { e.preventDefault(); showTitleBarMenu = false; }}
  ></div>
  <div
    class="surface-floating fixed z-50 overflow-hidden"
    style={titleBarMenuStyle}
    role="menu"
  >
    <div class="surface-floating-body">
      {#each controls as control}
        {@const checked = preferences.titleBarVisibility[control.id]}
        <button
          role="menuitemcheckbox"
          aria-checked={checked}
          onclick={() => onToggleControl(control.id)}
          class="menu-item"
        >
          <span class="min-w-0 flex-1 truncate">{control.label}</span>
          {#if checked}<Check class="size-3.5 shrink-0" aria-hidden="true" />{/if}
        </button>
      {/each}
      <div role="separator" class="menu-separator"></div>
      <button
        role="menuitemcheckbox"
        aria-checked={preferences.titleBarVisibility.compactTabs}
        onclick={() => onToggleControl("compactTabs")}
        class="menu-item"
      >
        <span class="min-w-0 flex-1 truncate">{t("titleBar.control.compactTabs")}</span>
        {#if preferences.titleBarVisibility.compactTabs}<Check class="size-3.5 shrink-0" aria-hidden="true" />{/if}
      </button>
    </div>
  </div>
{/if}
