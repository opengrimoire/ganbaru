<script lang="ts">
  import { tick } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getNotesEditor } from "$lib/components/notes/editor-context";
  import {
    flatNotesDestinationPickerTargets,
    nextNotesDestinationPickerIndex,
    notesDestinationPickerSectionList,
    notesDestinationPickerSections,
    type NotesDestinationPickerTarget,
  } from "$lib/notes/pages/destination-picker";
  import Search from "@lucide/svelte/icons/search";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  let {
    targets,
    searchLabel,
    searchPlaceholder,
    recentLabel,
    pagesLabel,
    emptyLabel,
    optionLabel,
    onSelect,
    onClose,
  }: {
    targets: NotesDestinationPickerTarget[];
    searchLabel: string;
    searchPlaceholder: string;
    recentLabel: string;
    pagesLabel: string;
    emptyLabel: string;
    optionLabel: (target: NotesDestinationPickerTarget) => string;
    onSelect: (key: string) => void;
    onClose: () => void;
  } = $props();

  let query = $state("");
  let queryInitialized = false;
  const notes = getNotesEditor();
  const { t } = getLocalization();
  let activeIndex = $state(0);
  let searchInput = $state<HTMLInputElement | null>(null);
  const sections = $derived(notesDestinationPickerSections(targets, query));
  const sectionList = $derived(notesDestinationPickerSectionList(sections));
  const flatTargets = $derived(flatNotesDestinationPickerTargets(sections));
  const activeTarget = $derived(flatTargets[activeIndex] ?? null);
  const listboxId = $derived(`notes-destination-picker-${targets.map((target) => target.key).join("-")}`);
  const activeOptionId = $derived(activeTarget ? `${listboxId}-${activeTarget.key}` : undefined);

  $effect(() => {
    const _query = query;
    activeIndex = flatTargets.length > 0 ? 0 : -1;
  });

  $effect(() => {
    const search = query;
    if (!queryInitialized) {
      queryInitialized = true;
      return;
    }
    const timeout = window.setTimeout(() => void notes.reloadLinkResolutionPages(search), 150);
    return () => window.clearTimeout(timeout);
  });

  $effect(() => {
    void tick().then(() => {
      searchInput?.focus();
    });
  });

  function sectionLabel(key: "recent" | "pages"): string {
    return key === "recent" ? recentLabel : pagesLabel;
  }

  function targetSubtitle(target: NotesDestinationPickerTarget): string {
    return target.path.join(" / ");
  }

  function selectTarget(target: NotesDestinationPickerTarget | null): void {
    if (!target) return;
    onSelect(target.key);
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (
      event.key === "ArrowDown"
      || event.key === "ArrowUp"
      || event.key === "Home"
      || event.key === "End"
    ) {
      event.preventDefault();
      activeIndex = nextNotesDestinationPickerIndex(flatTargets.length, activeIndex, event.key);
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      selectTarget(activeTarget);
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }
</script>

<div
  class="notes-destination-picker min-w-0"
  role="group"
  aria-label={searchLabel}
>
  <label class="field mx-2 mb-1 flex items-center gap-1.5">
    <Search class="size-3.5 shrink-0 text-muted-foreground" />
    <input
      bind:this={searchInput}
      class="field-bare text-foreground"
      bind:value={query}
      aria-label={searchLabel}
      aria-controls={listboxId}
      aria-activedescendant={activeOptionId}
      placeholder={searchPlaceholder}
      onkeydown={handleKeydown}
    />
  </label>

  {#if flatTargets.length === 0}
    <div class="px-2.5 py-1.5 text-panel-detail text-muted-foreground">
      {emptyLabel}
    </div>
  {:else}
    <div id={listboxId} class="max-h-60 overflow-auto py-1" role="listbox" use:scrollEdgeFadeAction aria-label={searchLabel}>
      {#each sectionList as section (section.key)}
        <div class="menu-label">
          {sectionLabel(section.key)}
        </div>
        {#each section.targets as target (target.key)}
          {@const targetIndex = flatTargets.findIndex((candidate) => candidate.key === target.key)}
          <button
            id={`${listboxId}-${target.key}`}
            class="menu-item min-w-0"
            data-highlighted={targetIndex === activeIndex ? "" : undefined}
            style={`padding-left: ${0.625 + Math.min(target.depth, 6) * 0.75}rem`}
            type="button"
            role="option"
            aria-selected={targetIndex === activeIndex}
            aria-label={optionLabel(target)}
            onmouseenter={() => {
              activeIndex = targetIndex;
            }}
            onclick={() => {
              selectTarget(target);
            }}
          >
            <span class="min-w-0 flex-1">
              <span class="block truncate">{target.title}</span>
              {#if targetSubtitle(target)}
                <span class="block truncate text-panel-detail text-muted-foreground">
                  {targetSubtitle(target)}
                </span>
              {/if}
            </span>
          </button>
        {/each}
      {/each}
      {#if notes.destinationHasMore}
        <button
          type="button"
          class="mx-2 my-1 rounded-floating-item border border-border px-2 py-1.5 text-panel-detail text-foreground hover:bg-accent"
          onclick={() => void notes.loadMoreDestinationCandidates()}
        >
          {t("common.loadMore")}
        </button>
      {/if}
    </div>
  {/if}
</div>
