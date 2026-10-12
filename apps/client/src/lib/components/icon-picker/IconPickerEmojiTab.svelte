<script lang="ts">
  import LayoutGrid from "@lucide/svelte/icons/layout-grid";
  import Leaf from "@lucide/svelte/icons/leaf";
  import Package from "@lucide/svelte/icons/package";
  import Plane from "@lucide/svelte/icons/plane";
  import Plus from "@lucide/svelte/icons/plus";
  import Search from "@lucide/svelte/icons/search";
  import Shapes from "@lucide/svelte/icons/shapes";
  import Shuffle from "@lucide/svelte/icons/shuffle";
  import Smile from "@lucide/svelte/icons/smile";
  import Trophy from "@lucide/svelte/icons/trophy";
  import Utensils from "@lucide/svelte/icons/utensils";
  import UserRound from "@lucide/svelte/icons/user-round";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type {
    ProjectEmojiCategoryId,
    ProjectEmojiEntry,
  } from "$lib/projects/icons/emoji-catalog.generated";
  import {
    ICON_PICKER_SKIN_TONE_OPTIONS,
    applyProjectEmojiSkinTone,
    type ProjectEmojiSkinTone,
    type IconPickerVisibleEmojiCategory,
  } from "$lib/projects/icons/picker";
  import type { ProjectIconValue } from "$lib/projects/icons/values";
  import type { ProjectCustomEmoji } from "$lib/projects/types";
  import { cn } from "$lib/utils";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";

  interface IconPickerEmojiGroup {
    category: IconPickerVisibleEmojiCategory;
    entries: readonly ProjectEmojiEntry[];
  }

  let {
    scrollElement = $bindable<HTMLElement | undefined>(),
    customEmojiTriggerElement = $bindable<HTMLButtonElement | undefined>(),
    emojiQuery = $bindable(""),
    emojiCategory = $bindable<ProjectEmojiCategoryId | "all">("all"),
    emojiSkinTone = $bindable<ProjectEmojiSkinTone>("default"),
    skinTonePanelOpen = $bindable(false),
    iconColorPanelOpen = $bindable(false),
    customEmojiPanelOpen = $bindable(false),
    allowCustomEmojiCreate = true,
    gridScrollable,
    gridCanScrollUp,
    gridCanScrollDown,
    gridColumnCount,
    emojiRecentValues,
    visibleCustomEmojis,
    emojiGroups,
    visibleEmojiCategories,
    onScroll,
    onChooseRandom,
    onChooseRecent,
    onChooseIcon,
    onResetGridScroll,
  }: {
    scrollElement?: HTMLElement;
    customEmojiTriggerElement?: HTMLButtonElement;
    emojiQuery: string;
    emojiCategory: ProjectEmojiCategoryId | "all";
    emojiSkinTone: ProjectEmojiSkinTone;
    skinTonePanelOpen: boolean;
    iconColorPanelOpen: boolean;
    customEmojiPanelOpen: boolean;
    allowCustomEmojiCreate?: boolean;
    gridScrollable: boolean;
    gridCanScrollUp: boolean;
    gridCanScrollDown: boolean;
    gridColumnCount: number;
    emojiRecentValues: readonly string[];
    visibleCustomEmojis: readonly ProjectCustomEmoji[];
    emojiGroups: readonly IconPickerEmojiGroup[];
    visibleEmojiCategories: readonly IconPickerVisibleEmojiCategory[];
    onScroll: () => void;
    onChooseRandom: () => void;
    onChooseRecent: (rawValue: string) => void;
    onChooseIcon: (icon: ProjectIconValue) => void;
    onResetGridScroll: () => void;
  } = $props();

  const { t } = getLocalization();

  function skinToneLabel(skinTone: ProjectEmojiSkinTone): string {
    if (skinTone === "light") return t("projects.iconPicker.skinToneLight");
    if (skinTone === "medium-light") return t("projects.iconPicker.skinToneMediumLight");
    if (skinTone === "medium") return t("projects.iconPicker.skinToneMedium");
    if (skinTone === "medium-dark") return t("projects.iconPicker.skinToneMediumDark");
    if (skinTone === "dark") return t("projects.iconPicker.skinToneDark");
    return t("projects.iconPicker.skinToneDefault");
  }

  function skinTonePreview(skinTone: ProjectEmojiSkinTone): string {
    return applyProjectEmojiSkinTone("✋", skinTone);
  }

  function displayEmoji(emoji: string): string {
    return applyProjectEmojiSkinTone(emoji, emojiSkinTone);
  }

  function selectEmojiCategory(category: ProjectEmojiCategoryId | "all"): void {
    emojiCategory = category;
    onResetGridScroll();
  }
</script>

<div class="flex shrink-0 items-center gap-2 px-3 pt-3">
  <div class="field flex h-8 min-w-0 flex-1 items-center gap-2 py-0">
    <Search size={14} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
    <input
      bind:value={emojiQuery}
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
  <div class="relative shrink-0" data-icon-picker-inline-panel>
    <button
      type="button"
      class={cn(
        "flex size-8 items-center justify-center rounded-floating-item border border-border text-base text-muted-foreground hover:bg-accent hover:text-foreground",
        skinTonePanelOpen && "bg-accent text-foreground",
      )}
      aria-label={t("projects.iconPicker.skinTone")}
      title={skinToneLabel(emojiSkinTone)}
      onclick={(event) => {
        event.stopPropagation();
        skinTonePanelOpen = !skinTonePanelOpen;
        iconColorPanelOpen = false;
      }}
    >
      {skinTonePreview(emojiSkinTone)}
    </button>
    {#if skinTonePanelOpen}
      <div
        class="surface-floating absolute right-0 top-9 z-10 grid gap-1 p-2"
        style="grid-template-columns: repeat(3, 2rem); width: 7rem; background-color: var(--icon-picker-bg); color: var(--icon-picker-text);"
      >
        {#each ICON_PICKER_SKIN_TONE_OPTIONS as tone}
          <button
            type="button"
            class={cn(
              "flex h-8 w-8 items-center justify-center rounded-md text-[1rem]",
              emojiSkinTone === tone.value ? "bg-accent text-foreground" : "text-muted-foreground hover:bg-accent hover:text-foreground",
            )}
            aria-label={skinToneLabel(tone.value)}
            title={skinToneLabel(tone.value)}
            onclick={(event) => {
              event.stopPropagation();
              emojiSkinTone = tone.value;
              skinTonePanelOpen = false;
            }}
          >
            {skinTonePreview(tone.value)}
          </button>
        {/each}
      </div>
    {/if}
  </div>
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
  {#if emojiRecentValues.length > 0}
    <section class="mb-3">
      <div class="mb-1 flex items-center gap-2 text-[0.733333rem] text-muted-foreground">
        <span>{t("projects.iconPicker.recent")}</span>
        <span class="h-px flex-1 bg-border/70"></span>
      </div>
      <div class="grid gap-0" style={`grid-template-columns: repeat(${gridColumnCount}, minmax(0, 1fr));`}>
        {#each emojiRecentValues as recentValue}
          <button
            type="button"
            class="flex h-9 items-center justify-center rounded-md hover:bg-accent"
            aria-label={t("projects.iconPicker.selectRecent")}
            onclick={() => onChooseRecent(recentValue)}
          >
            <ProjectIcon name={recentValue} size={18} />
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if visibleCustomEmojis.length > 0}
    <section class="mb-3">
      <div class="mb-1 flex items-center gap-2 text-[0.733333rem] text-muted-foreground">
        <span>{t("projects.iconPicker.custom")}</span>
        <span class="h-px flex-1 bg-border/70"></span>
      </div>
      <div class="grid gap-0" style={`grid-template-columns: repeat(${gridColumnCount}, minmax(0, 1fr));`}>
        {#each visibleCustomEmojis as emoji}
          <button
            type="button"
            class="flex h-9 items-center justify-center rounded-md hover:bg-accent"
            title={emoji.name}
            onclick={() => onChooseIcon({ kind: "custom-emoji", id: emoji.id })}
          >
            <ProjectIcon name={`custom-emoji:${emoji.id}`} size={20} />
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#each emojiGroups as group (group.category.id)}
    <section class="mb-3">
      <div class="mb-1 flex items-center gap-2 text-[0.733333rem] text-muted-foreground">
        <span>{group.category.label}</span>
        <span class="h-px flex-1 bg-border/70"></span>
      </div>
      <div class="grid gap-0" style={`grid-template-columns: repeat(${gridColumnCount}, minmax(0, 1fr));`}>
        {#each group.entries as entry (entry.emoji)}
          <button
            type="button"
            class="flex h-9 items-center justify-center rounded-md text-[1.2rem] hover:bg-accent"
            title={entry.name}
            onclick={() => onChooseIcon({ kind: "emoji", emoji: displayEmoji(entry.emoji) })}
          >
            {displayEmoji(entry.emoji)}
          </button>
        {/each}
      </div>
    </section>
  {/each}
</div>

<div class="flex shrink-0 items-center gap-1 overflow-x-auto border-t border-border/70 px-3 py-2">
  <button
    type="button"
    class={cn("flex h-8 w-8 shrink-0 items-center justify-center rounded-md", emojiCategory === "all" ? "bg-accent text-foreground" : "text-muted-foreground hover:bg-accent hover:text-foreground")}
    aria-label={t("projects.iconPicker.all")}
    title={t("projects.iconPicker.all")}
    onclick={() => selectEmojiCategory("all")}
  >
    <LayoutGrid size={16} strokeWidth={1.75} />
  </button>
  {#each visibleEmojiCategories as category}
    <button
      type="button"
      class={cn("flex h-8 w-8 shrink-0 items-center justify-center rounded-md", emojiCategory === category.id ? "bg-accent text-foreground" : "text-muted-foreground hover:bg-accent hover:text-foreground")}
      aria-label={category.label}
      title={category.label}
      onclick={() => selectEmojiCategory(category.id)}
    >
      {#if category.id === "smileys"}
        <Smile size={16} strokeWidth={1.75} />
      {:else if category.id === "people"}
        <UserRound size={16} strokeWidth={1.75} />
      {:else if category.id === "nature"}
        <Leaf size={16} strokeWidth={1.75} />
      {:else if category.id === "food"}
        <Utensils size={16} strokeWidth={1.75} />
      {:else if category.id === "activity"}
        <Trophy size={16} strokeWidth={1.75} />
      {:else if category.id === "travel"}
        <Plane size={16} strokeWidth={1.75} />
      {:else if category.id === "objects"}
        <Package size={16} strokeWidth={1.75} />
      {:else if category.id === "symbols"}
        <Shapes size={16} strokeWidth={1.75} />
      {/if}
    </button>
  {/each}
  {#if allowCustomEmojiCreate}
    <button
      bind:this={customEmojiTriggerElement}
      type="button"
      class={cn(
        "ml-auto flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground",
        customEmojiPanelOpen && "bg-accent text-foreground",
      )}
      aria-label={t("projects.iconPicker.addCustomEmoji")}
      title={t("projects.iconPicker.addCustomEmoji")}
      onclick={() => {
        customEmojiPanelOpen = !customEmojiPanelOpen;
        skinTonePanelOpen = false;
        iconColorPanelOpen = false;
      }}
    >
      <Plus size={16} strokeWidth={1.75} />
    </button>
  {/if}
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
