<script lang="ts">
  import CircleGauge from "@lucide/svelte/icons/circle-gauge";
  import MoreHorizontal from "@lucide/svelte/icons/more-horizontal";
  import Moon from "@lucide/svelte/icons/moon";
  import Settings from "@lucide/svelte/icons/settings";
  import Sun from "@lucide/svelte/icons/sun";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatShortcut } from "$lib/keyboard-shortcuts";
  import type { TitleBarControlId } from "$lib/stores/preference-options";
  import { cn } from "$lib/utils";

  let {
    showTheme,
    showPerformance,
    showSettings,
    themeIsDark,
    lockedByThemeEditor,
    showOverflow = $bindable(),
    overflowControls,
    onToggleTheme,
    onTogglePerformance,
    onOpenSettings,
    onActivateOverflow,
    onOverflowOpened,
  }: {
    showTheme: boolean;
    showPerformance: boolean;
    showSettings: boolean;
    themeIsDark: boolean;
    lockedByThemeEditor: boolean;
    showOverflow: boolean;
    overflowControls: Array<{ id: TitleBarControlId; label: string; disabled: boolean }>;
    onToggleTheme: () => void;
    onTogglePerformance: () => void;
    onOpenSettings: () => void;
    onActivateOverflow: (id: TitleBarControlId) => void;
    onOverflowOpened: () => void;
  } = $props();

  const { t } = getLocalization();
  const TITLE_BAR_ICON_COLOR_CLASS = "text-foreground/68 dark:text-white/76";
  const TITLE_BAR_ICON_SIZE = 14;
  const TITLE_BAR_ICON_STROKE_WIDTH = 1.5;

  function toggleOverflow(): void {
    showOverflow = !showOverflow;
    if (showOverflow) onOverflowOpened();
  }
</script>

    {#if showTheme}
      <button
        onclick={onToggleTheme}
        disabled={lockedByThemeEditor}
        class={cn(
          "titlebar-icon-button flex items-center justify-center rounded-lg transition-colors",
          TITLE_BAR_ICON_COLOR_CLASS,
          lockedByThemeEditor
            ? "cursor-not-allowed opacity-40"
            : "hover:bg-sidebar-accent",
        )}
        title={lockedByThemeEditor
          ? t("titleBar.theme.disabledWhileEditing")
          : themeIsDark
            ? t("titleBar.theme.switchToLight", formatShortcut("Mod + Shift + L"))
            : t("titleBar.theme.switchToDark", formatShortcut("Mod + Shift + L"))}
      >
        {#if themeIsDark}
          <Sun size={TITLE_BAR_ICON_SIZE} strokeWidth={TITLE_BAR_ICON_STROKE_WIDTH} />
        {:else}
          <Moon size={TITLE_BAR_ICON_SIZE} strokeWidth={TITLE_BAR_ICON_STROKE_WIDTH} />
        {/if}
      </button>
    {/if}

    {#if showPerformance}
      <div class="relative">
        <button
          onclick={onTogglePerformance}
          class={cn(
            "titlebar-icon-button flex items-center justify-center rounded-lg transition-colors hover:bg-sidebar-accent",
            TITLE_BAR_ICON_COLOR_CLASS,
          )}
          title={t("titleBar.diagnosticsWithShortcut", formatShortcut("Mod + Shift + D"))}
        >
          <CircleGauge size={TITLE_BAR_ICON_SIZE} strokeWidth={TITLE_BAR_ICON_STROKE_WIDTH} />
        </button>
      </div>
    {/if}

    {#if showSettings}
      <button
        onclick={onOpenSettings}
        disabled={lockedByThemeEditor}
        class={cn(
          "titlebar-icon-button flex items-center justify-center rounded-lg transition-colors",
          TITLE_BAR_ICON_COLOR_CLASS,
          lockedByThemeEditor
            ? "cursor-not-allowed opacity-40"
            : "hover:bg-sidebar-accent",
        )}
        title={lockedByThemeEditor
          ? t("titleBar.theme.disabledWhileEditing")
          : t("titleBar.settingsWithShortcut", formatShortcut("Mod + ,"))}
        aria-label={t("titleBar.control.settings")}
      >
        <Settings size={TITLE_BAR_ICON_SIZE} strokeWidth={TITLE_BAR_ICON_STROKE_WIDTH} />
      </button>
    {/if}

    {#if overflowControls.length > 0}
      <div class="relative">
        <button
          onclick={toggleOverflow}
          class={cn(
            "titlebar-icon-button flex items-center justify-center rounded-lg transition-colors hover:bg-sidebar-accent",
            TITLE_BAR_ICON_COLOR_CLASS,
          )}
          aria-label={t("titleBar.control.more")}
          data-app-tooltip-disabled="true"
        >
          <MoreHorizontal size={TITLE_BAR_ICON_SIZE} strokeWidth={TITLE_BAR_ICON_STROKE_WIDTH} />
        </button>
        {#if showOverflow}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="fixed inset-0 z-40"
            onclick={() => { showOverflow = false; }}
            onkeydown={(e) => { if (e.key === "Escape") showOverflow = false; }}
          ></div>
          <div class="surface-floating surface-floating-body absolute right-0 top-9 z-50 w-floating-sm">
            {#each overflowControls as control}
              <button
                onclick={() => onActivateOverflow(control.id)}
                disabled={control.disabled}
                class="menu-item"
              >
                {control.label}
              </button>
            {/each}
          </div>
        {/if}
      </div>
    {/if}

<style>
  .titlebar-icon-button {
    width: 32px;
    height: 32px;
  }
</style>
