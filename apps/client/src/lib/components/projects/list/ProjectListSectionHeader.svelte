<script lang="ts">
  import Archive from "@lucide/svelte/icons/archive";
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import MoreHorizontal from "@lucide/svelte/icons/more-horizontal";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn, type MaybePromise } from "$lib/utils";
  import type { ProjectSection } from "$lib/projects/types";
  import ProjectListSelectionButton from "./ProjectListSelectionButton.svelte";
  import { keepInViewport } from "$lib/utils/keep-in-viewport";


  let {
    section,
    gridTemplate,
    gridMinWidth,
    leadingGridTemplate,
    taskCount,
    allSelected,
    partiallySelected,
    canDrag,
    draft,
    draftDirty,
    draftSaveable,
    menuOpen,
    onToggleSelection,
    onToggleCollapsed,
    onDraftChange,
    onSave,
    onToggleOptionsMenu,
    onRestore,
    onHide,
    onArchive,
    onPointerDown,
    onPointerUp,
    onPointerCancel,
    onDragStart,
    onDragEnd,
  }: {
    section: ProjectSection;
    gridTemplate: string;
    gridMinWidth: string;
    leadingGridTemplate: string;
    taskCount: number;
    allSelected: boolean;
    partiallySelected: boolean;
    canDrag: boolean;
    draft: string;
    draftDirty: boolean;
    draftSaveable: boolean;
    menuOpen: boolean;
    onToggleSelection: () => void;
    onToggleCollapsed: () => MaybePromise;
    onDraftChange: (value: string) => void;
    onSave: () => MaybePromise;
    onToggleOptionsMenu: () => void;
    onRestore: () => MaybePromise;
    onHide: () => MaybePromise;
    onArchive: () => MaybePromise;
    onPointerDown: (event: PointerEvent) => void;
    onPointerUp: (event: PointerEvent) => void;
    onPointerCancel: (event: PointerEvent) => void;
    onDragStart: (event: DragEvent) => void;
    onDragEnd: (event: DragEvent) => void;
  } = $props();

  const { t } = getLocalization();

  const disabled = $derived(Boolean(section.hiddenAt || section.archivedAt));
  const collapseLabel = $derived(
    section.collapsed
      ? t("projects.actions.expandSection", section.name)
      : t("projects.actions.collapseSection", section.name),
  );
  const optionsLabel = $derived(t("projects.actions.sectionOptions", section.name));
</script>

<div
  class={cn(
    "project-list-divider project-list-sticky-row group/section-header grid min-h-(--collection-table-row-height) items-center px-1",
    canDrag && "cursor-grab active:cursor-grabbing",
  )}
  style={`grid-template-columns: ${gridTemplate}; min-width: ${gridMinWidth};`}
  role="group"
  aria-label={section.name}
  draggable={canDrag}
  onpointerdown={onPointerDown}
  onpointerup={onPointerUp}
  onpointercancel={onPointerCancel}
  ondragstart={onDragStart}
  ondragend={onDragEnd}
>
  <div
    class="project-list-leading-row grid min-h-(--collection-table-row-height) items-center"
    style={`grid-column: 1 / span 3; grid-template-columns: ${leadingGridTemplate};`}
  >
    <ProjectListSelectionButton
      mode="section"
      {allSelected}
      {partiallySelected}
      disabled={taskCount === 0}
      ariaLabel={allSelected
        ? t("projects.actions.unselectTaskGroup", section.name)
        : t("projects.actions.selectTaskGroup", section.name)}
      onToggle={onToggleSelection}
    />
    <button
      type="button"
      class="flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
      aria-label={collapseLabel}
      title={collapseLabel}
      onclick={() => { void onToggleCollapsed(); }}
    >
      {#if section.collapsed}
        <ChevronRight size={14} strokeWidth={1.75} />
      {:else}
        <ChevronDown size={14} strokeWidth={1.75} />
      {/if}
    </button>
    <div
      class={cn(
        "flex min-h-8 min-w-0 flex-1 items-center gap-2 rounded-md px-2 transition-colors",
        draftDirty
          ? "bg-card ring-1 ring-inset ring-border"
          : "bg-transparent ring-0 hover:bg-card/60",
      )}
    >
      <input
        value={draft}
        data-list-section-drag-source="true"
        class="min-h-7 min-w-0 flex-1 bg-transparent text-[0.866667rem] font-semibold disabled:text-muted-foreground"
        aria-label={t("projects.list.sectionName")}
        {disabled}
        oninput={(event) => onDraftChange(event.currentTarget.value)}
        onkeydown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            void onSave();
          }
        }}
      />
      {#if draftSaveable}
        <button
          type="button"
          class="flex h-7 shrink-0 cursor-pointer items-center justify-center rounded-md border border-border bg-card px-2 text-[0.733333rem] text-muted-foreground hover:bg-accent hover:text-foreground"
          onclick={() => { void onSave(); }}
        >
          {t("common.save")}
        </button>
      {/if}
      <div class="relative" data-section-options-root="true">
      <button
        type="button"
        class={cn(
          "flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center rounded-md text-muted-foreground opacity-0 transition-opacity hover:bg-accent hover:text-foreground group-hover/section-header:opacity-100 group-has-focus-visible/section-header:opacity-100",
          menuOpen && "bg-accent text-foreground opacity-100",
        )}
        aria-label={optionsLabel}
        title={optionsLabel}
        aria-expanded={menuOpen}
        onclick={onToggleOptionsMenu}
      >
        <MoreHorizontal size={14} strokeWidth={1.75} />
      </button>
      {#if menuOpen}
        <div
          use:keepInViewport
          class="surface-floating surface-floating-body absolute right-0 top-8 z-30 w-floating-sm"
          role="menu"
          aria-label={optionsLabel}
        >
          {#if disabled}
            <button
              type="button"
              class="menu-item"
              role="menuitem"
              onclick={() => { void onRestore(); }}
            >
              <ArchiveRestore size={14} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
              <span class="min-w-0 truncate">{t("projects.actions.restoreSection", section.name)}</span>
            </button>
          {:else}
            <button
              type="button"
              class="menu-item"
              role="menuitem"
              onclick={() => { void onHide(); }}
            >
              <EyeOff size={14} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
              <span class="min-w-0 truncate">{t("projects.actions.hideSection", section.name)}</span>
            </button>
            <button
              type="button"
              class="menu-item"
              role="menuitem"
              onclick={() => { void onArchive(); }}
            >
              <Archive size={14} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
              <span class="min-w-0 truncate">{t("projects.actions.archiveSection", section.name)}</span>
            </button>
          {/if}
        </div>
      {/if}
      </div>
    </div>
  </div>
</div>
