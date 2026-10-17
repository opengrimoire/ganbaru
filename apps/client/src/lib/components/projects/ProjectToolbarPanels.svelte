<script lang="ts">
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import CollectionMenuSeparator from "$lib/components/collections/CollectionMenuSeparator.svelte";
  import CollectionPanel from "$lib/components/collections/CollectionPanel.svelte";
  import CollectionSettings from "$lib/components/collections/CollectionSettings.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Archive from "@lucide/svelte/icons/archive";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ArrowUpDown from "@lucide/svelte/icons/arrow-up-down";
  import CalendarRange from "@lucide/svelte/icons/calendar-range";
  import Check from "@lucide/svelte/icons/check";
  import CircleDot from "@lucide/svelte/icons/circle-dot";
  import Columns3 from "@lucide/svelte/icons/columns-3";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import Flag from "@lucide/svelte/icons/flag";
  import Link2 from "@lucide/svelte/icons/link-2";
  import ListTree from "@lucide/svelte/icons/list-tree";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import Tags from "@lucide/svelte/icons/tags";
  import type { Component } from "svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    projectTagColorDotStyle,
    projectTagColorSwatchClass,
    projectPriorityDisplayLabel,
  } from "$lib/projects/display";
  import {
    PROJECT_SETTINGS_PANEL_MAX_HEIGHT,
    projectToolbarPanelGeometry,
    type ProjectListColumnControl,
    type ProjectToolbarPanel,
  } from "$lib/projects/toolbar";
  import {
    customFieldIdFromCustomFieldReference,
    customFieldReference,
  } from "$lib/projects/tasks/list-columns";
  import { projectCustomFieldUsesOptions } from "$lib/projects/custom-fields";
  import {
    PROJECT_TASK_GROUP_MODES,
    PROJECT_TASK_SORT_MODES,
    type ProjectCustomField,
    type ProjectCustomFieldFilter,
    type ProjectTag,
    type ProjectPriority,
    type ProjectPriorityConfig,
    type ProjectSection,
    type ProjectTaskDependencyFilter,
    type ProjectTaskDueFilter,
    type ProjectTaskGroupMode,
    type ProjectTaskTagFilter,
    type ProjectTaskListColumn,
    type ProjectTaskScheduleFilter,
    type ProjectTaskSortDirection,
    type ProjectTaskSortMode,
    type ProjectTaskStatusFilter,
  } from "$lib/projects/types";
  import { useProjects } from "$lib/stores/projects.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import {
    APP_FLOATING_SURFACE_SELECTOR,
    cn,
    isAppFloatingSurfaceTarget,
  } from "$lib/utils";
  import { portal } from "$lib/utils/portal";
  import PriorityFlagIcon from "./PriorityFlagIcon.svelte";
  import ProjectSettingsPanel from "$lib/components/projects/settings/ProjectSettingsPanel.svelte";
  import ProjectListPresentationControls from "$lib/components/projects/list/ProjectListPresentationControls.svelte";
  import type { ProjectTaskQueryController } from "./task-query-controller.svelte";

  const TASK_STATUS_FILTERS: ProjectTaskStatusFilter[] = ["all", "open", "blocked", "done"];
  const TASK_DUE_FILTERS: ProjectTaskDueFilter[] = ["all", "overdue", "today", "week", "none", "range"];
  const TASK_SCHEDULE_FILTERS: ProjectTaskScheduleFilter[] = ["all", "scheduled", "unscheduled"];
  const TASK_DEPENDENCY_FILTERS: ProjectTaskDependencyFilter[] = ["all", "linked", "blocked_by", "blocking", "none"];
  const TASK_SORT_MODES: ProjectTaskSortMode[] = [...PROJECT_TASK_SORT_MODES];
  const PROJECT_SETTINGS_PANEL_WIDTH = 430;

  let {
    panel,
    projectId,
    sections,
    priorities,
    projectTags,
    projectCustomFields,
    taskListColumnControls,
    archivedProjectTaskCount,
    inactiveSectionCount,
    taskFiltersActive,
    listColumnsSaving = false,
    listColumnsError = null,
    taskStatusFilter = $bindable<ProjectTaskStatusFilter>(),
    taskSectionFilter = $bindable<string | "all">(),
    taskPriorityFilter = $bindable<ProjectPriority | "all">(),
    taskDueFilter = $bindable<ProjectTaskDueFilter>(),
    taskDueRangeStart = $bindable<string>(),
    taskDueRangeEnd = $bindable<string>(),
    taskScheduleFilter = $bindable<ProjectTaskScheduleFilter>(),
    taskDependencyFilter = $bindable<ProjectTaskDependencyFilter>(),
    taskTagFilter = $bindable<ProjectTaskTagFilter>(),
    taskCustomFieldFilters = $bindable<ProjectCustomFieldFilter[]>(),
    taskGroupBy = $bindable<ProjectTaskGroupMode>(),
    taskSortMode = $bindable<ProjectTaskSortMode>(),
    taskSortDirection = $bindable<ProjectTaskSortDirection>(),
    showArchivedTasks = $bindable<boolean>(),
    showInactiveSections = $bindable<boolean>(),
    onClose,
    onRevealInactive,
    onProjectSettingsDirtyChange,
    onClearTaskFilters,
    onToggleTaskListColumn,
    mobileLayout = false,
    taskQuery,
  }: {
    panel: ProjectToolbarPanel | null;
    taskQuery?: ProjectTaskQueryController;
    projectId: string | null;
    sections: ProjectSection[];
    priorities: ProjectPriorityConfig[];
    projectTags: ProjectTag[];
    projectCustomFields: ProjectCustomField[];
    taskListColumnControls: ProjectListColumnControl[];
    archivedProjectTaskCount: number;
    inactiveSectionCount: number;
    taskFiltersActive: boolean;
    listColumnsSaving?: boolean;
    listColumnsError?: string | null;
    taskStatusFilter: ProjectTaskStatusFilter;
    taskSectionFilter: string | "all";
    taskPriorityFilter: ProjectPriority | "all";
    taskDueFilter: ProjectTaskDueFilter;
    taskDueRangeStart: string;
    taskDueRangeEnd: string;
    taskScheduleFilter: ProjectTaskScheduleFilter;
    taskDependencyFilter: ProjectTaskDependencyFilter;
    taskTagFilter: ProjectTaskTagFilter;
    taskCustomFieldFilters: ProjectCustomFieldFilter[];
    taskGroupBy: ProjectTaskGroupMode;
    taskSortMode: ProjectTaskSortMode;
    taskSortDirection: ProjectTaskSortDirection;
    showArchivedTasks: boolean;
    showInactiveSections: boolean;
    onClose: () => void;
    onRevealInactive: () => void;
    onProjectSettingsDirtyChange: (dirty: boolean) => void;
    onClearTaskFilters: () => void;
    onToggleTaskListColumn: (column: ProjectTaskListColumn) => void | Promise<void>;
    mobileLayout?: boolean;
  } = $props();

  const projects = useProjects();
  const theme = getTheme();
  const viewport = getViewport();
  const localization = getLocalization();
  const { t } = localization;

  let panelElement = $state<HTMLDivElement | null>(null);
  let panelStyle = $state("");
  let panelGeometryFrame: number | null = null;

  /** Return the localized heading for a toolbar panel. */
  function panelTitle(currentPanel: ProjectToolbarPanel): string {
    if (currentPanel === "settings") return t("projects.settings.title");
    if (currentPanel === "group") return t("projects.toolbar.group");
    if (currentPanel === "sort") return t("projects.toolbar.sort");
    if (currentPanel === "customize") return t("projects.toolbar.customize");
    return t("projects.filters.title");
  }

  /** Find the invoking toolbar button for the current collection configuration. */
  function panelTriggerElement(currentPanel: ProjectToolbarPanel | null): HTMLElement | null {
    if (!currentPanel) return null;
    return document.querySelector<HTMLElement>(`[data-project-toolbar-trigger="${currentPanel}"]`);
  }

  /** Position the project settings draft editor beside its toolbar trigger, or inset fullscreen on mobile. */
  function refreshPanelGeometry(): void {
    panelGeometryFrame = null;
    if (panel !== "settings") return;
    if (mobileLayout) {
      panelStyle = [
        "left: calc(var(--safe-area-left) + 0.5rem)",
        "right: calc(var(--safe-area-right) + 0.5rem)",
        "top: calc(var(--safe-area-top) + var(--mobile-topbar-h) + 0.5rem)",
        "bottom: calc(var(--safe-area-bottom) + 0.5rem)",
        "width: auto",
        "max-height: none",
      ].join("; ");
      return;
    }
    const trigger = panelTriggerElement(panel);
    if (!trigger) return;
    const rect = trigger.getBoundingClientRect();
    const geometry = projectToolbarPanelGeometry({
      anchorLeft: rect.left,
      anchorRight: rect.right,
      anchorTop: rect.top,
      anchorBottom: rect.bottom,
      viewportWidth: viewport.width,
      viewportHeight: viewport.height,
      preferredWidth: PROJECT_SETTINGS_PANEL_WIDTH,
      preferredHeight: PROJECT_SETTINGS_PANEL_MAX_HEIGHT,
    });
    panelStyle = [
      `left: ${Math.round(geometry.left)}px`,
      `top: ${Math.round(geometry.top)}px`,
      `width: ${Math.round(geometry.width)}px`,
      `height: ${Math.round(geometry.maxHeight)}px`,
      `max-height: ${Math.round(geometry.maxHeight)}px`,
    ].join("; ");
  }

  /** Coalesce position updates while the project draft editor is open. */
  function requestPanelGeometryRefresh(): void {
    if (panelGeometryFrame !== null) cancelAnimationFrame(panelGeometryFrame);
    panelGeometryFrame = requestAnimationFrame(refreshPanelGeometry);
  }

  /** Route outside closure through the project's unsaved-draft guard. */
  function handlePanelPointerDown(event: PointerEvent): void {
    if (panel !== "settings") return;
    const target = event.target;
    if (!(target instanceof Node)) return;
    if (target instanceof Element && target.closest("[role='dialog'][aria-modal='true']")) return;
    if (isAppFloatingSurfaceTarget(target)) return;
    if (panelTriggerElement(panel)?.contains(target) || panelElement?.contains(target)) return;
    onClose();
  }

  /** Let nested pickers dismiss before requesting closure of the project draft. */
  function handlePanelKeydown(event: KeyboardEvent): void {
    if (panel !== "settings" || event.defaultPrevented || event.key !== "Escape") return;
    const floating = event.target instanceof Element
      ? event.target.closest(APP_FLOATING_SURFACE_SELECTOR)
      : null;
    if (floating && floating !== panelElement) return;
    if (panelElement?.querySelector(APP_FLOATING_SURFACE_SELECTOR)) return;
    if (document.querySelector("[role='dialog'][aria-modal='true']")) return;
    event.preventDefault();
    event.stopPropagation();
    onClose();
  }

  $effect(() => {
    if (panel !== "settings") return;
    const viewportWidth = viewport.width;
    const viewportHeight = viewport.height;
    void viewportWidth;
    void viewportHeight;
    requestPanelGeometryRefresh();
  });

  $effect(() => {
    if (panel !== "settings") return;
    window.addEventListener("scroll", requestPanelGeometryRefresh, true);
    window.addEventListener("resize", requestPanelGeometryRefresh);
    return () => {
      window.removeEventListener("scroll", requestPanelGeometryRefresh, true);
      window.removeEventListener("resize", requestPanelGeometryRefresh);
      if (panelGeometryFrame !== null) {
        cancelAnimationFrame(panelGeometryFrame);
        panelGeometryFrame = null;
      }
    };
  });

  /** Shared option row; touch layouts raise its height through the panel row token. */
  function optionClass(): string {
    return "menu-item min-w-0 justify-between";
  }

  function taskStatusFilterLabel(filter: ProjectTaskStatusFilter): string {
    if (filter === "open") return t("projects.filters.open");
    if (filter === "blocked") return t("projects.filters.blocked");
    if (filter === "done") return t("projects.filters.done");
    return t("projects.filters.allStatuses");
  }

  function taskDueFilterLabel(filter: ProjectTaskDueFilter): string {
    if (filter === "overdue") return t("projects.filters.overdue");
    if (filter === "today") return t("projects.filters.today");
    if (filter === "week") return t("projects.filters.thisWeek");
    if (filter === "none") return t("projects.filters.noDueDate");
    if (filter === "range") return t("projects.filters.dueRange");
    return t("projects.filters.allDueDates");
  }

  function taskScheduleFilterLabel(filter: ProjectTaskScheduleFilter): string {
    if (filter === "scheduled") return t("projects.filters.scheduled");
    if (filter === "unscheduled") return t("projects.filters.unscheduled");
    return t("projects.filters.allSchedule");
  }

  function taskDependencyFilterLabel(filter: ProjectTaskDependencyFilter): string {
    if (filter === "linked") return t("projects.filters.hasDependencies");
    if (filter === "blocked_by") return t("projects.filters.blockedByDependencies");
    if (filter === "blocking") return t("projects.filters.blockingDependencies");
    if (filter === "none") return t("projects.filters.noDependencies");
    return t("projects.filters.allDependencies");
  }

  function taskTagFilterLabel(filter: ProjectTaskTagFilter): string {
    if (filter === "all") return t("projects.filters.allTags");
    if (filter === "none") return t("projects.filters.noTags");
    return projectTags.find((tag) => tag.id === filter)?.name ?? t("projects.filters.allTags");
  }

  function taskSectionFilterLabel(): string {
    if (taskSectionFilter === "all") return t("projects.filters.allSections");
    return sections.find((section) => section.id === taskSectionFilter)?.name ?? t("projects.filters.allSections");
  }

  function taskPriorityFilterLabel(): string {
    return taskPriorityFilter === "all"
      ? t("projects.filters.allPriorities")
      : projectPriorityDisplayLabel(taskPriorityFilter, priorities, t);
  }

  function customFieldFilterLabel(field: ProjectCustomField): string {
    const filter = customFieldFilterFor(field.id);
    if (!filter) return t("projects.filters.allValues");
    if (filter.mode === "empty") return t("projects.filters.empty");
    if (filter.mode === "filled") return t("projects.filters.filled");
    if (filter.mode === "checkbox") {
      return filter.checked ? t("projects.customFields.checked") : t("projects.customFields.unchecked");
    }
    if (filter.mode !== "option") return t("projects.filters.allValues");
    return projects.customFieldOptionsForField(field.id).find((option) => option.id === filter.optionId)?.name
      ?? t("projects.filters.allValues");
  }

  function taskGroupModeLabel(mode: ProjectTaskGroupMode): string {
    if (mode === "status") return t("projects.grouping.status");
    if (mode === "priority") return t("projects.grouping.priority");
    if (mode === "due") return t("projects.grouping.due");
    if (mode === "scheduled") return t("projects.grouping.scheduled");
    return t("projects.grouping.section");
  }

  function taskSortModeLabel(mode: ProjectTaskSortMode): string {
    const customFieldId = customFieldIdFromCustomFieldReference(mode);
    if (customFieldId) {
      return projectCustomFields.find((field) => field.id === customFieldId)?.name
        ?? t("projects.columns.customField");
    }
    if (mode === "status") return t("projects.sort.status");
    if (mode === "title") return t("projects.list.name");
    if (mode === "start") return t("projects.columns.start");
    if (mode === "section") return t("projects.sort.section");
    if (mode === "priority") return t("projects.sort.priority");
    if (mode === "due") return t("projects.sort.due");
    if (mode === "scheduled") return t("projects.sort.scheduled");
    if (mode === "created") return t("projects.sort.created");
    if (mode === "updated") return t("projects.sort.updated");
    if (mode === "estimate") return t("projects.sort.estimate");
    return t("projects.sort.manual");
  }

  function taskSortDirectionLabel(direction: ProjectTaskSortDirection): string {
    return direction === "asc" ? t("projects.sort.ascending") : t("projects.sort.descending");
  }

  function customFieldFilterFor(fieldId: string): ProjectCustomFieldFilter | undefined {
    return taskCustomFieldFilters.find((filter) => filter.fieldId === fieldId);
  }

  function setTaskCustomFieldFilter(filter: ProjectCustomFieldFilter): void {
    taskCustomFieldFilters = [
      ...taskCustomFieldFilters.filter((entry) => entry.fieldId !== filter.fieldId),
      filter,
    ];
  }

  function clearTaskCustomFieldFilter(fieldId: string): void {
    taskCustomFieldFilters = taskCustomFieldFilters.filter((filter) => filter.fieldId !== fieldId);
  }

  function customFieldSortMode(field: ProjectCustomField): ProjectTaskSortMode {
    return customFieldReference(field.id);
  }
</script>

<svelte:window onpointerdown={handlePanelPointerDown} onkeydown={handlePanelKeydown} />

{#snippet optionRow(label: string, active: boolean, onSelect: () => void, disabled = false)}
  <button
    type="button"
    data-collection-settings-row
    class={optionClass()}
    aria-pressed={active}
    {disabled}
    onclick={() => {
      onSelect();
    }}
  >
    <span class="truncate">{label}</span>
    {#if active}
      <Check size={13} strokeWidth={1.75} class="shrink-0" />
    {/if}
  </button>
{/snippet}

{#snippet priorityOptionRow(priority: ProjectPriorityConfig)}
  <button
    type="button"
    data-collection-settings-row
    class={optionClass()}
    aria-pressed={taskPriorityFilter === priority.id}
    onclick={() => {
      taskPriorityFilter = priority.id;
    }}
  >
    <span class="flex min-w-0 items-center gap-1.5">
      <PriorityFlagIcon color={priority.color} theme={theme.current} size={13} class="shrink-0" />
      <span class="min-w-0 truncate">{priority.name}</span>
    </span>
    {#if taskPriorityFilter === priority.id}
      <Check size={13} strokeWidth={1.75} class="shrink-0" />
    {/if}
  </button>
{/snippet}

{#snippet toggleRow(label: string, active: boolean, Icon: Component, onSelect: () => void)}
  <button
    type="button"
    data-collection-settings-row
    class={optionClass()}
    aria-pressed={active}
    onclick={() => {
      onSelect();
    }}
  >
    <Icon size={14} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
    <span class="min-w-0 flex-1 truncate">{label}</span>
    {#if active}
      <Check size={13} strokeWidth={1.75} class="shrink-0" />
    {/if}
  </button>
{/snippet}

{#snippet sortOptions()}
  {#each TASK_SORT_MODES as mode}
    {@render optionRow(taskSortModeLabel(mode), taskSortMode === mode, () => { taskSortMode = mode; })}
  {/each}
  {#each projectCustomFields as field (field.id)}
    {@const mode = customFieldSortMode(field)}
    {@render optionRow(field.name, taskSortMode === mode, () => { taskSortMode = mode; })}
  {/each}
  <CollectionMenuSeparator />
  {@render toggleRow(taskSortDirectionLabel(taskSortDirection), true, taskSortDirection === "asc" ? ArrowUp : ArrowDown, () => { taskSortDirection = taskSortDirection === "asc" ? "desc" : "asc"; })}
{/snippet}

{#snippet visibilityOptions()}
  {@render toggleRow(showArchivedTasks ? t("projects.filters.hideArchived") : t("projects.filters.showArchived", archivedProjectTaskCount), showArchivedTasks, Archive, () => { showArchivedTasks = !showArchivedTasks; })}
  {#if inactiveSectionCount > 0}
    {@render toggleRow(showInactiveSections ? t("projects.filters.hideInactiveSectionsShort") : t("projects.filters.showInactiveSectionsShort", inactiveSectionCount), showInactiveSections, showInactiveSections ? EyeOff : Eye, () => { showInactiveSections = !showInactiveSections; })}
  {/if}
{/snippet}

{#if panel === "settings" && projectId}
  <div use:portal class="fixed z-80" style={panelStyle}>
    <CollectionPanel bind:element={panelElement} class="h-full" label={panelTitle(panel)}>
      <ProjectSettingsPanel
        {projectId}
        presentation="popover"
        onClose={onClose}
        onRevealInactive={onRevealInactive}
        onDirtyChange={onProjectSettingsDirtyChange}
      />
    </CollectionPanel>
  </div>
{:else if panel && panel !== "settings"}
  {#key panel}
    <CollectionSettings label={panelTitle(panel)} anchor={panelTriggerElement(panel)} onClose={onClose}>
      {#if panel === "group"}
        {#each PROJECT_TASK_GROUP_MODES as mode}
          {@render optionRow(taskGroupModeLabel(mode), taskGroupBy === mode, () => { taskGroupBy = mode; })}
        {/each}
      {:else if panel === "sort"}
        {@render sortOptions()}
      {:else if panel === "filters"}
        <CollectionMenu fullWidth kind="filter" icon={CircleDot} label={t("projects.columns.status")} summary={taskStatusFilterLabel(taskStatusFilter)}>
          {#each TASK_STATUS_FILTERS as filter}
            {@render optionRow(taskStatusFilterLabel(filter), taskStatusFilter === filter, () => { taskStatusFilter = filter; })}
          {/each}
        </CollectionMenu>
        <CollectionMenu fullWidth kind="filter" icon={ListTree} label={t("projects.sort.section")} summary={taskSectionFilterLabel()}>
          {@render optionRow(t("projects.filters.allSections"), taskSectionFilter === "all", () => { taskSectionFilter = "all"; })}
          {#each sections as section (section.id)}
            {@render optionRow(section.name, taskSectionFilter === section.id, () => { taskSectionFilter = section.id; })}
          {/each}
        </CollectionMenu>
        <CollectionMenu fullWidth kind="filter" icon={Flag} label={t("projects.columns.priority")} summary={taskPriorityFilterLabel()}>
          {@render optionRow(t("projects.filters.allPriorities"), taskPriorityFilter === "all", () => { taskPriorityFilter = "all"; })}
          {#each priorities as priority (priority.id)}
            {@render priorityOptionRow(priority)}
          {/each}
        </CollectionMenu>
        <CollectionMenu fullWidth kind="filter" icon={CalendarRange} label={t("projects.columns.due")} summary={taskDueFilterLabel(taskDueFilter)}>
          {#each TASK_DUE_FILTERS as filter}
            {@render optionRow(taskDueFilterLabel(filter), taskDueFilter === filter, () => { taskDueFilter = filter; })}
          {/each}
          {#if taskDueFilter === "range"}
            <div class="mt-1 grid grid-cols-[1fr_auto_1fr] items-center gap-1">
              <input
                bind:value={taskDueRangeStart}
                placeholder={t("projects.filters.dueRangeStart")}
                aria-label={t("projects.filters.dueRangeStart")}
                class="field min-w-0 text-foreground"
              />
              <span class="text-panel-detail text-muted-foreground">{t("projects.filters.dueRangeTo")}</span>
              <input
                bind:value={taskDueRangeEnd}
                placeholder={t("projects.filters.dueRangeEnd")}
                aria-label={t("projects.filters.dueRangeEnd")}
                class="field min-w-0 text-foreground"
              />
            </div>
          {/if}
        </CollectionMenu>
        <CollectionMenu fullWidth kind="filter" icon={CalendarRange} label={t("projects.columns.scheduled")} summary={taskScheduleFilterLabel(taskScheduleFilter)}>
          {#each TASK_SCHEDULE_FILTERS as filter}
            {@render optionRow(taskScheduleFilterLabel(filter), taskScheduleFilter === filter, () => { taskScheduleFilter = filter; })}
          {/each}
        </CollectionMenu>
        <CollectionMenu fullWidth kind="filter" icon={Link2} label={t("projects.columns.dependencies")} summary={taskDependencyFilterLabel(taskDependencyFilter)}>
          {#each TASK_DEPENDENCY_FILTERS as filter}
            {@render optionRow(taskDependencyFilterLabel(filter), taskDependencyFilter === filter, () => { taskDependencyFilter = filter; })}
          {/each}
        </CollectionMenu>
        <CollectionMenu fullWidth kind="filter" icon={Tags} label={t("projects.settings.tags")} summary={taskTagFilterLabel(taskTagFilter)}>
          {@render optionRow(taskTagFilterLabel("all"), taskTagFilter === "all", () => { taskTagFilter = "all"; })}
          {@render optionRow(taskTagFilterLabel("none"), taskTagFilter === "none", () => { taskTagFilter = "none"; })}
          {#each projectTags as tag (tag.id)}
            <button
              type="button"
              data-collection-settings-row
              class={optionClass()}
              aria-pressed={taskTagFilter === tag.id}
              onclick={() => {
                taskTagFilter = tag.id;
              }}
            >
              <span class="flex min-w-0 items-center gap-2">
                <span
                  class={cn("h-2 w-2 shrink-0 rounded-full border", projectTagColorSwatchClass(tag.color))}
                  style={projectTagColorDotStyle(tag.color, theme.current)}
                ></span>
                <span class="truncate">{tag.name}</span>
              </span>
              {#if taskTagFilter === tag.id}
                <Check size={13} strokeWidth={1.75} class="shrink-0" />
              {/if}
            </button>
          {/each}
        </CollectionMenu>
        {#each projectCustomFields as field (field.id)}
          <CollectionMenu fullWidth kind="filter" icon={SlidersHorizontal} label={field.name} summary={customFieldFilterLabel(field)}>
            {@const currentCustomFieldFilter = customFieldFilterFor(field.id)}
            {@render optionRow(t("projects.filters.allValues"), currentCustomFieldFilter === undefined, () => clearTaskCustomFieldFilter(field.id))}
            {#if projectCustomFieldUsesOptions(field.fieldType)}
              {@render optionRow(t("projects.filters.empty"), currentCustomFieldFilter?.mode === "empty", () => setTaskCustomFieldFilter({ fieldId: field.id, mode: "empty" }))}
              {#each projects.customFieldOptionsForField(field.id) as option (option.id)}
                {@render optionRow(option.name, currentCustomFieldFilter?.mode === "option" && currentCustomFieldFilter.optionId === option.id, () => setTaskCustomFieldFilter({ fieldId: field.id, mode: "option", optionId: option.id }))}
              {/each}
            {:else if field.fieldType === "checkbox"}
              {@render optionRow(t("projects.customFields.checked"), currentCustomFieldFilter?.mode === "checkbox" && currentCustomFieldFilter.checked, () => setTaskCustomFieldFilter({ fieldId: field.id, mode: "checkbox", checked: true }))}
              {@render optionRow(t("projects.customFields.unchecked"), currentCustomFieldFilter?.mode === "checkbox" && !currentCustomFieldFilter.checked, () => setTaskCustomFieldFilter({ fieldId: field.id, mode: "checkbox", checked: false }))}
              {@render optionRow(t("projects.filters.empty"), currentCustomFieldFilter?.mode === "empty", () => setTaskCustomFieldFilter({ fieldId: field.id, mode: "empty" }))}
            {:else}
              {@render optionRow(t("projects.filters.filled"), currentCustomFieldFilter?.mode === "filled", () => setTaskCustomFieldFilter({ fieldId: field.id, mode: "filled" }))}
              {@render optionRow(t("projects.filters.empty"), currentCustomFieldFilter?.mode === "empty", () => setTaskCustomFieldFilter({ fieldId: field.id, mode: "empty" }))}
            {/if}
          </CollectionMenu>
        {/each}
        <CollectionMenuSeparator />
        <CollectionMenu fullWidth kind="sort" icon={ArrowUpDown} label={t("projects.toolbar.sort")} summary={taskSortModeLabel(taskSortMode)}>
          {@render sortOptions()}
        </CollectionMenu>
        <CollectionMenu fullWidth kind="properties" icon={Eye} label={t("projects.toolbar.visibility")} summary={showArchivedTasks || showInactiveSections ? formatNumber(localization.locale, Number(showArchivedTasks) + Number(showInactiveSections)) : t("projects.toolbar.default")}>
          {@render visibilityOptions()}
        </CollectionMenu>
        {#if taskFiltersActive}
          <CollectionMenuSeparator />
          <button type="button" data-collection-settings-row class={optionClass()} onclick={onClearTaskFilters}>
            <span class="flex min-w-0 items-center gap-2">
              <RotateCcw size={14} strokeWidth={1.75} class="shrink-0 text-muted-foreground" />
              <span class="truncate">{t("projects.filters.reset")}</span>
            </span>
          </button>
        {/if}
      {:else if panel === "customize"}
        {#if taskQuery}<ProjectListPresentationControls query={taskQuery} />{/if}
        <CollectionMenu fullWidth kind="properties" icon={Columns3} label={t("projects.columns.title")} summary={formatNumber(localization.locale, taskListColumnControls.filter((control) => control.visible).length)}>
          {#each taskListColumnControls as control (control.column)}
            {@render optionRow(control.label, control.visible, () => { void onToggleTaskListColumn(control.column); }, listColumnsSaving)}
          {/each}
          {#if listColumnsError}
            <Notice class="px-2 py-1" message={listColumnsError} />
          {/if}
        </CollectionMenu>
      {/if}
    </CollectionSettings>
  {/key}
{/if}
