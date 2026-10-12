<script lang="ts">
  import IconPickerColorControl from "./IconPickerColorControl.svelte";
  import CircleEllipsis from "@lucide/svelte/icons/circle-ellipsis";
  import LayoutGrid from "@lucide/svelte/icons/layout-grid";
  import Search from "@lucide/svelte/icons/search";
  import Shapes from "@lucide/svelte/icons/shapes";
  import Shuffle from "@lucide/svelte/icons/shuffle";
  import type { EventColor } from "$lib/calendar/types";
  import type {
    IconPickerGroupVirtualWindow,
    IconPickerLucideCategoryOption,
    IconPickerColor,
  } from "$lib/projects/icons/picker";
  import type {
    ProjectLucideCategory,
    ProjectLucideIconEntry,
    ProjectLucideIconNode,
  } from "$lib/projects/icons/lucide-catalog.generated";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import LucideNodeIcon from "./LucideNodeIcon.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";

  let {
    scrollElement = $bindable<HTMLElement | undefined>(),
    iconCategoryMenuTriggerElement = $bindable<HTMLButtonElement | undefined>(),
    iconQuery = $bindable(""),
    iconCategory,
    iconCategoryMenuOpen,
    iconCategoryOverflowActive,
    iconColor = $bindable<IconPickerColor>(),
    iconColorPanelOpen = $bindable(false),
    skinTonePanelOpen = $bindable(false),
    askIconColorEveryTime,
    colorSelectionBorder,
    gridScrollable,
    gridCanScrollUp,
    gridCanScrollDown,
    gridColumnCount,
    lucideRecentValues,
    lucideGroupVirtual,
    lucideLoading,
    primaryLucideCategoryOptions,
    automaticIconColor,
    iconColorLabel,
    iconColorSwatch,
    iconColorStyle,
    lucideRecentPreviewValue,
    onScroll,
    onChooseRandom,
    onChooseRecent,
    onChooseLucideIcon,
    onSelectIconColor,
    onDefaultIconColorPanelOpen,
    onAskIconColorEveryTimeChange,
    onSelectIconCategory,
    onToggleIconCategoryMenu,
  }: {
    scrollElement?: HTMLElement;
    iconCategoryMenuTriggerElement?: HTMLButtonElement;
    iconQuery: string;
    iconCategory: ProjectLucideCategory | "all";
    iconCategoryMenuOpen: boolean;
    iconCategoryOverflowActive: boolean;
    iconColor: IconPickerColor;
    iconColorPanelOpen: boolean;
    skinTonePanelOpen: boolean;
    askIconColorEveryTime: boolean;
    colorSelectionBorder: string;
    gridScrollable: boolean;
    gridCanScrollUp: boolean;
    gridCanScrollDown: boolean;
    gridColumnCount: number;
    lucideRecentValues: readonly string[];
    lucideGroupVirtual: IconPickerGroupVirtualWindow<ProjectLucideCategory, ProjectLucideIconEntry>;
    lucideLoading: boolean;
    primaryLucideCategoryOptions: readonly IconPickerLucideCategoryOption[];
    automaticIconColor: string;
    iconColorLabel: (color: IconPickerColor) => string;
    iconColorSwatch: (color: EventColor) => string;
    iconColorStyle: (color: EventColor) => string;
    lucideRecentPreviewValue: (rawValue: string) => string;
    onScroll: () => void;
    onChooseRandom: () => void;
    onChooseRecent: (rawValue: string, target: EventTarget | null) => void;
    onChooseLucideIcon: (
      slug: string,
      label: string,
      iconNode: readonly ProjectLucideIconNode[] | null,
      target: EventTarget | null,
    ) => void;
    onSelectIconColor: (color: IconPickerColor) => void;
    onDefaultIconColorPanelOpen: () => void;
    onAskIconColorEveryTimeChange: (enabled: boolean) => void;
    onSelectIconCategory: (category: ProjectLucideCategory | "all") => void;
    onToggleIconCategoryMenu: () => void;
  } = $props();

  const { t } = getLocalization();
</script>

<div class="flex shrink-0 items-center gap-2 px-3 pt-3">
  <div class="field flex h-8 min-w-0 flex-1 items-center gap-2 py-0">
    <Search size={14} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
    <input
      bind:value={iconQuery}
      class="field-bare h-full"
      placeholder={t("projects.iconPicker.filter")}
    />
  </div>
  <button
    type="button"
    class="flex size-8 shrink-0 items-center justify-center rounded-floating-item border border-border text-muted-foreground hover:bg-accent hover:text-foreground"
    aria-label={t("projects.iconPicker.random")}
    onclick={onChooseRandom}
  >
    <Shuffle size={14} strokeWidth={1.75} />
  </button>
  <IconPickerColorControl
    bind:open={iconColorPanelOpen}
    color={iconColor}
    label={t("projects.iconPicker.iconColor")}
    askEveryTime={askIconColorEveryTime}
    {colorSelectionBorder}
    automaticColor={automaticIconColor}
    colorLabel={iconColorLabel}
    colorSwatch={iconColorSwatch}
    onSelect={onSelectIconColor}
    onOpen={() => { onDefaultIconColorPanelOpen(); skinTonePanelOpen = false; }}
    onAskEveryTimeChange={onAskIconColorEveryTimeChange}
  />
</div>

<div
  bind:this={scrollElement}
  class={cn(
    "project-icon-picker-scroll-area min-h-0 flex-1 overflow-y-auto px-3 py-3",
    gridScrollable
      && gridCanScrollUp
      && gridCanScrollDown
      && "project-icon-picker-scroll-both",
    gridScrollable
      && gridCanScrollUp
      && !gridCanScrollDown
      && "project-icon-picker-scroll-top",
    gridScrollable
      && !gridCanScrollUp
      && gridCanScrollDown
      && "project-icon-picker-scroll-bottom",
  )}
  onscroll={onScroll}
>
  {#if lucideRecentValues.length > 0}
    <section class="mb-3">
      <div class="mb-1 flex h-5 items-center gap-2 text-[0.733333rem] text-muted-foreground">
        <span>{t("projects.iconPicker.recent")}</span>
        <span class="h-px flex-1 bg-border/70"></span>
      </div>
      <div class="grid gap-0" style={`grid-template-columns: repeat(${gridColumnCount}, minmax(0, 1fr));`}>
        {#each lucideRecentValues as recentValue}
          <button
            type="button"
            class={cn(
              "flex h-9 items-center justify-center rounded-md hover:bg-accent hover:text-foreground",
              iconColor !== "default" ? "text-muted-foreground" : "text-foreground",
            )}
            aria-label={t("projects.iconPicker.selectRecent")}
            onclick={(event) => onChooseRecent(recentValue, event.currentTarget)}
          >
            <ProjectIcon name={lucideRecentPreviewValue(recentValue)} size={18} />
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if lucideLoading}
    <Loading layout="fill" class="min-h-24" />
  {:else}
    <div style={`height: ${lucideGroupVirtual.beforeHeight}px;`} aria-hidden="true"></div>
    {#each lucideGroupVirtual.groups as group (group.category)}
      <section class="mb-3">
        <div class="mb-1 flex h-5 items-center gap-2 text-[0.733333rem] text-muted-foreground">
          <span>{group.category}</span>
          <span class="h-px flex-1 bg-border/70"></span>
        </div>
        <div style={`height: ${group.beforeRowsHeight}px;`} aria-hidden="true"></div>
        <div class="grid gap-0" style={`grid-template-columns: repeat(${gridColumnCount}, minmax(0, 1fr));`}>
          {#each group.entries as entry (entry.slug)}
            <button
              type="button"
              class={cn(
                "flex h-9 items-center justify-center rounded-md hover:bg-accent hover:text-foreground",
                iconColor !== "default" ? "text-muted-foreground" : "text-foreground",
              )}
              title={entry.label}
              onclick={(event) => onChooseLucideIcon(entry.slug, entry.label, entry.iconNode, event.currentTarget)}
            >
              <LucideNodeIcon
                iconNode={entry.iconNode}
                size={18}
                strokeWidth={1.75}
                style={iconColor !== "default" ? iconColorStyle(iconColor) : undefined}
              />
            </button>
          {/each}
        </div>
        <div style={`height: ${group.afterRowsHeight}px;`} aria-hidden="true"></div>
      </section>
    {/each}
    <div style={`height: ${lucideGroupVirtual.afterHeight}px;`} aria-hidden="true"></div>
  {/if}
</div>

<div class="flex shrink-0 items-center gap-1 border-t border-border/70 px-3 py-2">
  <button
    type="button"
    class={cn("flex h-8 w-8 shrink-0 items-center justify-center rounded-md", iconCategory === "all" ? "bg-accent text-foreground" : "text-muted-foreground hover:bg-accent hover:text-foreground")}
    aria-label={t("projects.iconPicker.all")}
    title={t("projects.iconPicker.all")}
    onclick={() => onSelectIconCategory("all")}
  >
    <LayoutGrid size={16} strokeWidth={1.75} />
  </button>
  {#each primaryLucideCategoryOptions as option (option.category)}
    <button
      type="button"
      class={cn("flex h-8 w-8 shrink-0 items-center justify-center rounded-md", iconCategory === option.category ? "bg-accent text-foreground" : "text-muted-foreground hover:bg-accent hover:text-foreground")}
      aria-label={option.category}
      title={option.category}
      onclick={() => onSelectIconCategory(option.category)}
    >
      {#if option.icon}
        <LucideNodeIcon iconNode={option.icon.iconNode} size={16} strokeWidth={1.75} />
      {:else}
        <Shapes size={16} strokeWidth={1.75} />
      {/if}
    </button>
  {/each}
  <div class="h-5 w-px shrink-0 bg-border/80"></div>
  <button
    bind:this={iconCategoryMenuTriggerElement}
    type="button"
    class={cn(
      "flex h-8 w-8 shrink-0 items-center justify-center rounded-md",
      iconCategoryMenuOpen || iconCategoryOverflowActive
        ? "bg-accent text-foreground"
        : "text-muted-foreground hover:bg-accent hover:text-foreground",
    )}
    aria-label={t("projects.iconPicker.moreCategories")}
    title={t("projects.iconPicker.moreCategories")}
    onclick={onToggleIconCategoryMenu}
  >
    <CircleEllipsis size={16} strokeWidth={1.75} />
  </button>
</div>

<style>
  .project-icon-picker-scroll-area {
    transition: -webkit-mask-image 120ms ease, mask-image 120ms ease;
  }

  .project-icon-picker-scroll-top {
    -webkit-mask-image: linear-gradient(to bottom, transparent, black 20px, black);
    mask-image: linear-gradient(to bottom, transparent, black 20px, black);
  }

  .project-icon-picker-scroll-bottom {
    -webkit-mask-image: linear-gradient(to bottom, black, black calc(100% - 20px), transparent);
    mask-image: linear-gradient(to bottom, black, black calc(100% - 20px), transparent);
  }

  .project-icon-picker-scroll-both {
    -webkit-mask-image: linear-gradient(to bottom, transparent, black 20px, black calc(100% - 20px), transparent);
    mask-image: linear-gradient(to bottom, transparent, black 20px, black calc(100% - 20px), transparent);
  }

</style>
