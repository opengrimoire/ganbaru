<script lang="ts">
  import Minimize2 from "@lucide/svelte/icons/minimize-2";
  import Minus from "@lucide/svelte/icons/minus";
  import Square from "@lucide/svelte/icons/square";
  import X from "@lucide/svelte/icons/x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatShortcut } from "$lib/keyboard-shortcuts";
  import { cn } from "$lib/utils";

  let {
    isMaximized,
    lockedByBenchmark,
    isMainWindow,
    onMinimize,
    onToggleMaximize,
    onClose,
  }: {
    isMaximized: boolean;
    lockedByBenchmark: boolean;
    isMainWindow: boolean;
    onMinimize: () => void;
    onToggleMaximize: () => void;
    onClose: () => void;
  } = $props();

  const { t } = getLocalization();
  const iconColorClass = "text-foreground/68 dark:text-white/76";
  const iconSize = 14;
  const strokeWidth = 1.5;
</script>

<div class="flex shrink-0 items-center gap-0.5 pr-1.5">
  <button
    onclick={onMinimize}
    class={cn(
      "titlebar-icon-button flex items-center justify-center rounded-lg transition-colors hover:bg-sidebar-accent",
      iconColorClass,
    )}
    aria-label={t("common.minimize")}
    data-app-tooltip-disabled="true"
  >
    <Minus size={iconSize} strokeWidth={strokeWidth} />
  </button>
  <button
    onclick={onToggleMaximize}
    class={cn(
      "titlebar-icon-button flex items-center justify-center rounded-lg transition-colors hover:bg-sidebar-accent",
      iconColorClass,
    )}
    aria-label={isMaximized ? t("common.restore") : t("common.maximize")}
    data-app-tooltip-disabled="true"
  >
    {#if isMaximized}
      <Minimize2 size={iconSize} strokeWidth={strokeWidth} />
    {:else}
      <Square size={iconSize} strokeWidth={strokeWidth} />
    {/if}
  </button>
  <button
    onclick={onClose}
    disabled={lockedByBenchmark}
    class={cn(
      "titlebar-icon-button flex items-center justify-center rounded-lg transition-colors",
      iconColorClass,
      lockedByBenchmark
        ? "cursor-not-allowed opacity-40"
        : "hover:bg-destructive hover:text-destructive-foreground",
    )}
    title={lockedByBenchmark
      ? t("titleBar.disabledBenchmark")
      : isMainWindow
        ? t("window.closeAppWithShortcut", formatShortcut("Mod + Shift + W"))
        : t("window.closeWindowWithShortcut", formatShortcut("Mod + Shift + W"))}
    aria-label={t("window.close")}
  >
    <X size={iconSize} strokeWidth={strokeWidth} />
  </button>
</div>

<style>
  .titlebar-icon-button {
    width: 28px;
    height: 28px;
  }
</style>
