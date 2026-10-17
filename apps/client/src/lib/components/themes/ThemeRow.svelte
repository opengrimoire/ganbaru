<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Eye from "@lucide/svelte/icons/eye";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Download from "@lucide/svelte/icons/download";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Sun from "@lucide/svelte/icons/sun";
  import Moon from "@lucide/svelte/icons/moon";
  import { themeDisplayName } from "$lib/i18n/theme-labels";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import type { Theme } from "$lib/themes";
  import ThemeMiniPreview from "./ThemeMiniPreview.svelte";

  let {
    theme,
    isActive,
    isBuiltin,
    onApply,
    onOpen,
    onDuplicate,
    onExport,
    onDelete,
    showEditorActions = true,
    showFileActions = true,
    exporting = false,
    exportDisabled = false,
    mobileLayout = false,
  }: {
    theme: Theme;
    isActive: boolean;
    isBuiltin: boolean;
    onApply: () => void;
    onOpen: () => void;
    onDuplicate: () => void;
    onExport: () => void;
    onDelete: () => void;
    showEditorActions?: boolean;
    showFileActions?: boolean;
    exporting?: boolean;
    exportDisabled?: boolean;
    mobileLayout?: boolean;
  } = $props();

  const { t } = getLocalization();
  const BaseIcon = $derived(theme.iconLabel === "dark" ? Moon : Sun);
  const displayName = $derived(themeDisplayName(theme, t));
  let hovering = $state(false);
  let suppressHover = $state(false);

  function handlePointerEnter() {
    hovering = true;
    suppressHover = false;
  }

  function handlePointerLeave() {
    hovering = false;
    suppressHover = false;
  }

  function handlePointerDown() {
    suppressHover = true;
  }
</script>

<div
  role="group"
  aria-label={displayName}
  onpointerenter={handlePointerEnter}
  onpointerleave={handlePointerLeave}
  onpointerdown={handlePointerDown}
  class={cn(
    "relative flex items-center justify-between gap-1 rounded-md px-1 py-1 transition-colors max-[520px]:flex-col max-[520px]:items-stretch",
    mobileLayout && "min-h-14 gap-2 rounded-xl py-2",
    !isActive && "hover:text-foreground",
    hovering && !suppressHover && "bg-accent/25",
  )}
>
  <button
    type="button"
    onclick={onApply}
    data-app-tooltip-disabled="true"
    class="absolute inset-0 rounded-md focus:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-default"
    disabled={isActive}
    aria-label={isActive
      ? t("settings.theme.activeTheme", displayName)
      : t("settings.theme.applyTheme", displayName)}
  ></button>

  <div class="pointer-events-none relative z-10 flex min-w-0 flex-1 items-center gap-3 text-left">
    <div class="min-w-0 flex-1">
      <div class="flex items-center gap-2 text-[0.866667rem] text-foreground">
        <BaseIcon
          size={13}
          strokeWidth={1.75}
          class="shrink-0 text-muted-foreground"
        />
        <span class="truncate">{displayName}</span>
        {#if isActive}
          <Check size={13} strokeWidth={2.5} class="shrink-0 text-foreground" />
        {/if}
      </div>
    </div>
    <ThemeMiniPreview {theme} />
  </div>

  <div class="relative z-20 flex shrink-0 items-center justify-end gap-1">
    {#if showEditorActions}
      <button
        type="button"
        onclick={onDuplicate}
        aria-label={t("settings.theme.duplicateAndEditTheme")}
        data-app-tooltip-disabled="true"
        class={cn(
          "flex items-center gap-1.5 text-[0.8rem] text-foreground transition-colors",
          mobileLayout
            ? "h-8 rounded-lg border border-border bg-card px-2.5 active:bg-accent dark:bg-transparent"
            : "h-7 rounded-md border border-border bg-card px-2.5 hover:bg-accent dark:bg-transparent",
        )}
      >
        <Copy size={13} strokeWidth={2} />
        <span class="max-[380px]:hidden">{t("settings.theme.duplicateAndEdit")}</span>
      </button>
      <button
        type="button"
        onclick={onOpen}
        aria-label={isBuiltin ? t("settings.theme.viewTheme") : t("settings.theme.editTheme")}
        data-app-tooltip-disabled="true"
        class={cn(
          "flex items-center justify-center text-foreground transition-colors",
          mobileLayout
            ? "size-8 rounded-lg border border-border bg-card active:bg-accent dark:bg-transparent"
            : "h-7 w-7 rounded-md border border-border bg-card hover:bg-accent dark:bg-transparent",
        )}
      >
        {#if isBuiltin}
          <Eye size={13} strokeWidth={2} />
        {:else}
          <Pencil size={13} strokeWidth={2} />
        {/if}
      </button>
    {/if}
    {#if showFileActions}
      <button
        type="button"
        onclick={onExport}
        disabled={exporting || exportDisabled}
        aria-busy={exporting}
        aria-label={exporting
          ? t("settings.theme.exportingJson")
          : t("settings.theme.exportJson")}
        data-app-tooltip-disabled="true"
        class={cn(
          "flex items-center justify-center text-foreground transition-colors disabled:cursor-wait disabled:opacity-55",
          mobileLayout
            ? "size-8 rounded-lg border border-border bg-card active:bg-accent dark:bg-transparent"
            : "h-7 w-7 rounded-md border border-border bg-card hover:bg-accent dark:bg-transparent",
        )}
      >
        {#if exporting}
          <LoaderCircle
            size={13}
            strokeWidth={2}
            class="animate-spin motion-reduce:animate-none"
          />
        {:else}
          <Download size={13} strokeWidth={2} />
        {/if}
      </button>
    {/if}
    <button
      type="button"
      onclick={onDelete}
      aria-label={t("settings.theme.deleteTheme")}
      data-app-tooltip-disabled="true"
      disabled={isBuiltin}
      class={cn(
        "flex items-center justify-center text-foreground transition-colors disabled:cursor-not-allowed disabled:opacity-40",
        mobileLayout
          ? "size-8 rounded-lg border border-border bg-card active:bg-accent disabled:active:bg-transparent dark:bg-transparent"
          : "h-7 w-7 rounded-md border border-border bg-card hover:bg-accent disabled:hover:bg-card dark:bg-transparent dark:disabled:hover:bg-transparent",
      )}
    >
      <Trash2 size={13} strokeWidth={2} />
    </button>
  </div>
</div>
