<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import SquareArrowLeft from "@lucide/svelte/icons/square-arrow-left";
  import SquareArrowUpRight from "@lucide/svelte/icons/square-arrow-up-right";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { DetachableTabView } from "$lib/navigation";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import type { TitleBarControlId } from "$lib/stores/preference-options";

  let {
    showTabContextMenu = $bindable(),
    tabContextView = $bindable(),
    tabContextMenuStyle,
    detachedWindow,
    canDetachTab,
    showTitleBarMenu = $bindable(),
    titleBarMenuStyle,
    controls,
    onTabContextAction,
    onToggleControl,
  }: {
    showTabContextMenu: boolean;
    tabContextView: DetachableTabView | null;
    tabContextMenuStyle: string;
    detachedWindow: boolean;
    canDetachTab: boolean;
    showTitleBarMenu: boolean;
    titleBarMenuStyle: string;
    controls: Array<{ id: TitleBarControlId; label: string }>;
    onTabContextAction: () => void;
    onToggleControl: (id: TitleBarControlId) => void;
  } = $props();

  const preferences = getPreferences();
  const { t } = getLocalization();
</script>

{#if showTabContextMenu && tabContextView}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-40"
    onclick={() => { showTabContextMenu = false; tabContextView = null; }}
    oncontextmenu={(e) => { e.preventDefault(); showTabContextMenu = false; tabContextView = null; }}
  ></div>
  <div
    class="surface-floating fixed z-50 overflow-hidden"
    style={tabContextMenuStyle}
    role="menu"
  >
    <div class="surface-floating-body">
      <button
        role="menuitem"
        disabled={!detachedWindow && !canDetachTab}
        title={!detachedWindow && !canDetachTab ? t("titleBar.keepOneTabInMainWindow") : undefined}
        onclick={onTabContextAction}
        class="menu-item"
      >
        {#if detachedWindow}
          <SquareArrowLeft size={15} strokeWidth={2.2} />
          <span class="shrink-0 whitespace-nowrap">{t("titleBar.moveBackToMainWindow")}</span>
        {:else}
          <SquareArrowUpRight size={15} strokeWidth={2.2} />
          <span class="shrink-0 whitespace-nowrap">{t("titleBar.moveToNewWindow")}</span>
        {/if}
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
