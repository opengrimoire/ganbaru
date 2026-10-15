<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Lock from "@lucide/svelte/icons/lock";
  import LockOpen from "@lucide/svelte/icons/lock-open";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import X from "@lucide/svelte/icons/x";
  import SquareArrowUpRight from "@lucide/svelte/icons/square-arrow-up-right";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatShortcut } from "$lib/keyboard-shortcuts";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import type { TitleBarControlId } from "$lib/stores/preference-options";

  let {
    showTabContextMenu = $bindable(),
    tabContextId = $bindable(),
    tabContextMenuStyle,
    moveToNewWindowBlockReason,
    canReopenTab,
    lockAction,
    showTitleBarMenu = $bindable(),
    titleBarMenuStyle,
    controls,
    onMoveToNewWindow,
    onCloseTab,
    onReopenTab,
    onToggleLock,
    onToggleControl,
  }: {
    showTabContextMenu: boolean;
    tabContextId: string | null;
    tabContextMenuStyle: string;
    /** Whether this window is not the main one, so its tabs can move there. */
    /** Why the tab cannot move to a new window, or null when it can. */
    moveToNewWindowBlockReason: string | null;
    canReopenTab: boolean;
    /** Whether the tab can be locked or unlocked, or null when it shows nothing to lock to. */
    lockAction: "lock" | "unlock" | null;
    showTitleBarMenu: boolean;
    titleBarMenuStyle: string;
    controls: Array<{ id: TitleBarControlId; label: string }>;
    onMoveToNewWindow: () => void;
    onCloseTab: () => void;
    onReopenTab: () => void;
    onToggleLock: () => void;
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
