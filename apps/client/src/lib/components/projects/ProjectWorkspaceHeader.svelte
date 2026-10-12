<script lang="ts">
  import CollectionViewButton from "$lib/components/collections/CollectionViewButton.svelte";
  import ArrowUpDown from "@lucide/svelte/icons/arrow-up-down";
  import Check from "@lucide/svelte/icons/check";
  import Columns3 from "@lucide/svelte/icons/columns-3";
  import Layers from "@lucide/svelte/icons/layers";
  import ListFilter from "@lucide/svelte/icons/list-filter";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    COMPACT_IDENTITY_EMOJI_SCALE,
    COMPACT_IDENTITY_ICON_SIZE,
    COMPACT_IDENTITY_ICON_STROKE_WIDTH,
  } from "$lib/icon-sizing";
  import {
    projectLifecycleBadgeClass,
    projectLifecycleLabel,
  } from "$lib/projects/display";
  import {
    projectNavigatorPanelGeometry,
    type ProjectToolbarPanel,
    type ProjectNavigatorPanelMode,
  } from "$lib/projects/toolbar";
  import {
    PROJECT_VIEW_IDS,
    type Project,
    type ProjectChatIntegration,
    type ProjectGroup,
    type ProjectViewId,
  } from "$lib/projects/types";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import { cn } from "$lib/utils";
  import ProjectIcon from "./ProjectIcon.svelte";
  import WorkspaceBreadcrumbTerminalIcon from "$lib/components/ui/WorkspaceBreadcrumbTerminalIcon.svelte";
  import ProjectNavigator from "./ProjectNavigator.svelte";
  import ProjectPickerMobileDialog from "$lib/components/projects/pickers/ProjectPickerMobileDialog.svelte";
  import Select from "$lib/components/ui/Select.svelte";

  let {
    selectedProject,
    selectedGroup,
    selectedProjectId,
    showInactiveProjects,
    projectToolbarPanel,
    taskGroupingActive,
    taskFiltersActive,
    taskCustomizeActive,
    onShowInactiveProjectsChange,
    onProjectSelected,
    onToggleToolbarPanel,
    onCreateTask,
    mobileLayout = false,
    projectChat = null,
  }: {
    selectedProject: Project;
    selectedGroup: ProjectGroup;
    selectedProjectId: string | null;
    showInactiveProjects: boolean;
    projectToolbarPanel: ProjectToolbarPanel | null;
    taskGroupingActive: boolean;
    taskFiltersActive: boolean;
    taskCustomizeActive: boolean;
    onShowInactiveProjectsChange: (value: boolean) => void;
    onProjectSelected: () => void;
    onToggleToolbarPanel: (panel: ProjectToolbarPanel) => void;
    onCreateTask: () => void;
    mobileLayout?: boolean;
    projectChat?: ProjectChatIntegration | null;
  } = $props();

  const projects = getProjects();
  const mobileBackStack = getMobileBackStack();
  const viewport = getViewport();
  const { t } = getLocalization();
  const projectIdentityIconSize = COMPACT_IDENTITY_ICON_SIZE;
  const projectIdentityIconStrokeWidth = COMPACT_IDENTITY_ICON_STROKE_WIDTH;
  const projectIdentityEmojiScale = COMPACT_IDENTITY_EMOJI_SCALE;

  let projectNavigatorOpen = $state(false);
  let mobileViewMenuOpen = $state(false);
  let mobileCustomizationMenuOpen = $state(false);
  let projectNavigatorMode = $state<ProjectNavigatorPanelMode>("groups");
  let projectHeaderElement = $state<HTMLDivElement | null>(null);
  let projectNavigatorAnchorElement = $state<HTMLButtonElement | null>(null);
  let projectGroupTriggerElement = $state<HTMLButtonElement | null>(null);
  let projectTriggerElement = $state<HTMLButtonElement | null>(null);
  let projectNavigatorPanelElement = $state<HTMLDivElement | null>(null);
  let projectNavigatorPanelStyle = $state("");
  let projectNavigatorPanelMaxHeight = $state(0);
  const projectWorkingFolders = $derived(
    projectChat?.listWorkingFolders(selectedProject.id) ?? [],
  );

  $effect(() => {
    if (!projectChat) return;
    void projectChat.ensureLoaded().catch((error) => {
      console.error("load project working folders failed", error);
    });
  });

  async function openProjectChat(workingFolderId?: string): Promise<void> {
    if (!projectChat) return;
    await projectChat.openProject(selectedProject.id, workingFolderId);
  }
  interface ProjectNavigatorBounds {
    left: number;
    right: number;
    top: number;
    bottom: number;
  }

  function viewLabel(view: ProjectViewId): string {
    if (view === "dashboard") return t("projects.tabs.dashboard");
    if (view === "list") return t("projects.tabs.list");
    if (view === "kanban") return t("projects.tabs.kanban");
    if (view === "calendar") return t("projects.tabs.calendar");
    if (view === "gantt") return t("projects.tabs.gantt");
    return t("projects.tabs.list");
  }

  function viewShortcutTitle(view: ProjectViewId): string {
    const shortcut = String(PROJECT_VIEW_IDS.indexOf(view) + 1);
    return `${viewLabel(view)} (${t("calendar.toolbar.shortcutKey", shortcut)})`;
  }

  function toolbarIconButtonClass(active: boolean, open = false): string {
    return cn(
      "flex shrink-0 items-center justify-center rounded-md text-foreground transition-colors hover:bg-accent",
      mobileLayout ? "h-12 w-12" : "h-7 w-7",
      (active || open) && "bg-accent",
    );
  }

  const MOBILE_CUSTOMIZATION_PANELS = ["filters", "sort", "customize", "group"] as const;
  type MobileCustomizationPanel = (typeof MOBILE_CUSTOMIZATION_PANELS)[number];

  function customizationIcon(panel: MobileCustomizationPanel) {
    if (panel === "filters") return ListFilter;
    if (panel === "sort") return ArrowUpDown;
    if (panel === "customize") return Columns3;
    return Layers;
  }

  function customizationLabel(panel: MobileCustomizationPanel): string {
    if (panel === "filters") return t("projects.filters.title");
    if (panel === "sort") return t("projects.toolbar.sort");
    if (panel === "customize") return t("projects.toolbar.customize");
    return t("projects.toolbar.group");
  }

  function customizationActive(panel: MobileCustomizationPanel): boolean {
    if (panel === "filters") return taskFiltersActive;
    if (panel === "customize") return taskCustomizeActive;
    if (panel === "group") return taskGroupingActive;
    return false;
  }

  function closeMobileMenus(): void {
    mobileViewMenuOpen = false;
    mobileCustomizationMenuOpen = false;
  }

  function selectMobileView(view: ProjectViewId): void {
    closeMobileMenus();
    projects.activeView = view;
  }

  function selectMobileCustomization(panel: MobileCustomizationPanel): void {
    closeMobileMenus();
    onToggleToolbarPanel(panel);
  }

  function projectNavigatorBounds(): ProjectNavigatorBounds {
    const boundsElement = projectHeaderElement?.closest(".projects-view-root");
    const rect = boundsElement?.getBoundingClientRect();
    if (rect) {
      return {
        left: rect.left,
        right: rect.right,
        top: rect.top,
        bottom: rect.bottom,
      };
    }

    return {
      left: 0,
      right: viewport.width,
      top: 0,
      bottom: viewport.height,
    };
  }

  function refreshProjectNavigatorPanelGeometry(): void {
    if (!projectNavigatorOpen) return;
    if (mobileLayout) {
      projectNavigatorPanelStyle = "";
      projectNavigatorPanelMaxHeight = 0;
      return;
    }
    if (!projectNavigatorAnchorElement) return;
    const rect = projectNavigatorAnchorElement.getBoundingClientRect();
    const bounds = projectNavigatorBounds();
    const geometry = projectNavigatorPanelGeometry({
      anchorLeft: rect.left,
      anchorBottom: rect.bottom,
      viewportWidth: viewport.width,
      viewportHeight: viewport.height,
      boundsLeft: bounds.left,
      boundsRight: bounds.right,
      boundsTop: bounds.top,
      boundsBottom: bounds.bottom,
    });
    projectNavigatorPanelStyle = [
      `left: ${Math.round(geometry.left)}px`,
      `top: ${Math.round(geometry.top)}px`,
      `width: ${Math.round(geometry.width)}px`,
    ].join("; ");
    projectNavigatorPanelMaxHeight = geometry.height;
  }

  function openProjectNavigator(mode: ProjectNavigatorPanelMode): void {
    closeMobileMenus();
    projectNavigatorMode = mode;
    projectNavigatorAnchorElement = mode === "groups"
      ? projectGroupTriggerElement
      : projectTriggerElement;
    projectNavigatorOpen = true;
    refreshProjectNavigatorPanelGeometry();
    requestAnimationFrame(refreshProjectNavigatorPanelGeometry);
  }

  function toggleProjectNavigator(mode: ProjectNavigatorPanelMode): void {
    if (projectNavigatorOpen && projectNavigatorMode === mode) {
      projectNavigatorOpen = false;
      return;
    }
    openProjectNavigator(mode);
  }

  function closeProjectNavigator(): void {
    projectNavigatorOpen = false;
  }

  $effect(() => {
    if (!mobileLayout || (!mobileViewMenuOpen && !mobileCustomizationMenuOpen)) return;
    return mobileBackStack.activate({
      handle: closeMobileMenus,
    });
  });

  function handleProjectWindowPointerDown(event: PointerEvent): void {
    if (mobileLayout) return;
    const target = event.target;
    if (!(target instanceof Node)) return;
    if (
      projectNavigatorOpen
      && !projectGroupTriggerElement?.contains(target)
      && !projectTriggerElement?.contains(target)
      && !projectNavigatorPanelElement?.contains(target)
    ) {
      projectNavigatorOpen = false;
    }
  }

  $effect(() => {
    if (!projectNavigatorOpen) return;
    const viewportWidth = viewport.width;
    const viewportHeight = viewport.height;
    void viewportWidth;
    void viewportHeight;
    requestAnimationFrame(refreshProjectNavigatorPanelGeometry);
  });

</script>

<svelte:window onpointerdown={handleProjectWindowPointerDown} />

<div
  bind:this={projectHeaderElement}
  class={cn(
    "flex shrink-0 items-center gap-1",
    mobileLayout ? "overflow-visible px-3" : "hide-scrollbar overflow-x-auto px-3",
  )}
  style="height: var(--cal-header-row-h); background-color: var(--cal-header-bg); border-bottom: 1px solid var(--sidebar);"
  onscroll={refreshProjectNavigatorPanelGeometry}
>
  <div
    class={cn(
      "relative",
      mobileLayout
        ? "min-w-0 flex-1 overflow-hidden"
        : "min-w-36 shrink-0 min-[760px]:max-w-md",
    )}
  >
    <div class="flex min-w-0 max-w-full items-center gap-0.5 text-identity font-medium {mobileLayout ? 'h-12' : 'h-7'}">
      <button
        bind:this={projectGroupTriggerElement}
        type="button"
        class={cn(
          "flex min-w-0 items-center gap-1.5 rounded-md px-1.5 text-left hover:bg-accent",
          mobileLayout && "shrink-0",
          mobileLayout ? "h-12" : "h-7",
          projectNavigatorOpen && projectNavigatorMode === "groups" && "bg-accent",
        )}
        aria-label={t("projects.navigator.open")}
        aria-haspopup="dialog"
        aria-expanded={projectNavigatorOpen && projectNavigatorMode === "groups"}
        onpointerenter={() => { if (!mobileLayout) openProjectNavigator("groups"); }}
        onclick={() => toggleProjectNavigator("groups")}
      >
        {#if !mobileLayout}
          <ProjectIcon
            name={selectedGroup.icon}
            size={projectIdentityIconSize}
            strokeWidth={projectIdentityIconStrokeWidth}
            emojiScale={projectIdentityEmojiScale}
            class="shrink-0"
          />
        {/if}
        <span class="min-w-0 truncate text-foreground">{selectedGroup.name}</span>
      </button>
      <span class="shrink-0 px-0.5 font-semibold text-muted-foreground">/</span>
      <button
        bind:this={projectTriggerElement}
        type="button"
        class={cn(
          "flex min-w-0 items-center gap-1.5 rounded-md px-1.5 text-left hover:bg-accent",
          mobileLayout && "flex-1",
          mobileLayout ? "h-12" : "h-7",
          projectNavigatorOpen && projectNavigatorMode === "projects" && "bg-accent",
        )}
        aria-label={t("projects.navigator.open")}
        aria-haspopup="dialog"
        aria-expanded={projectNavigatorOpen && projectNavigatorMode === "projects"}
        onpointerenter={() => { if (!mobileLayout) openProjectNavigator("projects"); }}
        onclick={() => toggleProjectNavigator("projects")}
      >
        {#if !mobileLayout}
          <ProjectIcon
            name={selectedProject.icon}
            size={projectIdentityIconSize}
            strokeWidth={projectIdentityIconStrokeWidth}
            emojiScale={projectIdentityEmojiScale}
            class="shrink-0"
          />
        {/if}
        <span class="min-w-0 truncate text-foreground">{selectedProject.name}</span>
        {#if selectedProject.status !== "active"}
          <span class={cn("shrink-0 rounded border px-1.5 py-0.5 text-[0.666667rem]", projectLifecycleBadgeClass(selectedProject.status))}>
            {projectLifecycleLabel(selectedProject.status, t)}
          </span>
        {/if}
        <WorkspaceBreadcrumbTerminalIcon kind="chevron" class="shrink-0 text-muted-foreground" />
      </button>
      <button
        type="button"
        class={toolbarIconButtonClass(false)}
        aria-label={t("projects.header.addTask")}
        title={t("projects.header.addTask")}
        data-project-new-task-button
        onclick={onCreateTask}
      >
        <WorkspaceBreadcrumbTerminalIcon kind="plus" />
      </button>
    </div>
    {#if projectNavigatorOpen}
      {#if mobileLayout}
        <ProjectPickerMobileDialog
          label={t("projects.navigator.pickerLabel")}
          closeLabel={t("projects.navigator.closePicker")}
          onClose={closeProjectNavigator}
        >
          <ProjectNavigator
            {selectedProjectId}
            selectedGroupId={selectedGroup.id}
            {showInactiveProjects}
            panelMode="groups"
            initialMobileGroupId={projectNavigatorMode === "projects" ? selectedGroup.id : null}
            onShowInactiveProjectsChange={onShowInactiveProjectsChange}
            onProjectSelected={() => {
              closeProjectNavigator();
              onProjectSelected();
            }}
            mobileLayout
            onClose={closeProjectNavigator}
          />
        </ProjectPickerMobileDialog>
      {:else}
        <div
          bind:this={projectNavigatorPanelElement}
          class="fixed z-80"
          style={projectNavigatorPanelStyle}
          role="dialog"
          tabindex="-1"
          aria-label={t("projects.navigator.pickerLabel")}
        >
          <ProjectNavigator
            {selectedProjectId}
            selectedGroupId={selectedGroup.id}
            {showInactiveProjects}
            panelMode={projectNavigatorMode}
            panelMaxHeight={projectNavigatorPanelMaxHeight}
            onShowInactiveProjectsChange={onShowInactiveProjectsChange}
            onProjectSelected={() => {
              closeProjectNavigator();
              onProjectSelected();
            }}
          />
        </div>
      {/if}
    {/if}
  </div>
  {#if mobileLayout}
    <div class="flex shrink-0 items-center gap-0">
      <div class="relative shrink-0">
        <button
          type="button"
          class={cn("flex h-12 items-center rounded-md px-2 text-sm hover:bg-accent", mobileViewMenuOpen && "bg-accent")}
          aria-label={viewLabel(projects.activeView)}
          title={viewLabel(projects.activeView)}
          aria-haspopup="menu"
          aria-expanded={mobileViewMenuOpen}
          onclick={() => {
            projectNavigatorOpen = false;
            mobileCustomizationMenuOpen = false;
            mobileViewMenuOpen = !mobileViewMenuOpen;
          }}
        >
          {viewLabel(projects.activeView)}
        </button>
        {#if mobileViewMenuOpen}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="fixed inset-0 z-40" onclick={closeMobileMenus}></div>
          <div
            class="surface-floating surface-floating-body absolute right-0 top-full z-50 mt-1 w-floating-sm"
            role="menu"
            aria-label={t("projects.toolbar.views")}
          >
            {#each PROJECT_VIEW_IDS as view}
              {@const label = viewLabel(view)}
              <button
                type="button"
                role="menuitemradio"
                aria-checked={projects.activeView === view}
                class="menu-item"
                onclick={() => selectMobileView(view)}
              >
                <span class="min-w-0 flex-1 truncate">{label}</span>
                {#if projects.activeView === view}<Check class="size-3.5 shrink-0" aria-hidden="true" />{/if}
              </button>
            {/each}
          </div>
        {/if}
      </div>

      <div class="relative shrink-0">
        <button
          type="button"
          class={toolbarIconButtonClass(
            taskFiltersActive || taskCustomizeActive || taskGroupingActive,
            mobileCustomizationMenuOpen,
          )}
          aria-label={t("projects.toolbar.customization")}
          title={t("projects.toolbar.customization")}
          aria-haspopup="menu"
          aria-expanded={mobileCustomizationMenuOpen}
          onclick={() => {
            projectNavigatorOpen = false;
            mobileViewMenuOpen = false;
            mobileCustomizationMenuOpen = !mobileCustomizationMenuOpen;
          }}
        >
          <SlidersHorizontal size={16} strokeWidth={1.75} />
        </button>
        {#if mobileCustomizationMenuOpen}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="fixed inset-0 z-40" onclick={closeMobileMenus}></div>
          <div
            class="surface-floating surface-floating-body absolute right-0 top-full z-50 mt-1 w-floating-sm"
            role="menu"
            aria-label={t("projects.toolbar.customization")}
          >
            {#each MOBILE_CUSTOMIZATION_PANELS as panel}
              {@const Icon = customizationIcon(panel)}
              {@const label = customizationLabel(panel)}
              {@const active = customizationActive(panel) || projectToolbarPanel === panel}
              <button
                type="button"
                role="menuitem"
                class="menu-item"
                onclick={() => selectMobileCustomization(panel)}
              >
                <Icon class="text-muted-foreground" size={16} strokeWidth={1.75} />
                <span class="min-w-0 flex-1 truncate">{label}</span>
                {#if active}<Check class="size-3.5 shrink-0" aria-hidden="true" />{/if}
              </button>
            {/each}
          </div>
        {/if}
      </div>

      <button
        type="button"
        data-project-toolbar-trigger="settings"
        class={toolbarIconButtonClass(false, projectToolbarPanel === "settings")}
        aria-label={t("projects.header.projectSettings")}
        title={t("projects.header.projectSettings")}
        aria-expanded={projectToolbarPanel === "settings"}
        onclick={() => {
          closeMobileMenus();
          projectNavigatorOpen = false;
          onToggleToolbarPanel("settings");
        }}
      >
        <Settings2 size={16} strokeWidth={1.75} />
      </button>
    </div>
  {:else}
    <div class="flex-1"></div>
    <nav class="flex shrink-0 items-center gap-0.5" aria-label={t("projects.toolbar.views")}>
      {#each PROJECT_VIEW_IDS as view}
        <CollectionViewButton class="text-xs" label={viewLabel(view)} title={viewShortcutTitle(view)} active={projects.activeView === view} onclick={() => { projects.activeView = view; }} />
      {/each}
    </nav>
    <div class="flex shrink-0 items-center gap-1">
      {#if projectChat}
        <button
          type="button"
          class={toolbarIconButtonClass(false)}
          aria-label={t("projects.header.openChat")}
          title={t("projects.header.openChat")}
          onclick={() => { void openProjectChat(); }}
        >
          <MessageSquare size={14} strokeWidth={1.75} />
        </button>
        {#if projectWorkingFolders.length > 1}
          <Select
            inline
            class="w-auto max-w-32"
            value=""
            triggerLabel={t("projects.header.chatFolder")}
            ariaLabel={t("projects.header.chatFolder")}
            popoverAlign="end"
            options={projectWorkingFolders.map((folder) => ({ value: folder.id, label: folder.displayName }))}
            onChange={(workingFolderId) => {
              if (workingFolderId) void openProjectChat(workingFolderId);
            }}
          />
        {/if}
      {/if}
      <button
        type="button"
        data-project-toolbar-trigger="filters"
        class={toolbarIconButtonClass(taskFiltersActive, projectToolbarPanel === "filters")}
        aria-label={t("projects.filters.title")}
        title={t("projects.filters.title")}
        aria-expanded={projectToolbarPanel === "filters"}
        onclick={() => onToggleToolbarPanel("filters")}
      >
        <ListFilter size={14} strokeWidth={1.75} />
      </button>
      <button
        type="button"
        data-project-toolbar-trigger="sort"
        class={toolbarIconButtonClass(false, projectToolbarPanel === "sort")}
        aria-label={t("projects.toolbar.sort")}
        title={t("projects.toolbar.sort")}
        aria-expanded={projectToolbarPanel === "sort"}
        onclick={() => onToggleToolbarPanel("sort")}
      >
        <ArrowUpDown size={14} strokeWidth={1.75} />
      </button>
      <button
        type="button"
        data-project-toolbar-trigger="customize"
        class={toolbarIconButtonClass(taskCustomizeActive, projectToolbarPanel === "customize")}
        aria-label={t("projects.toolbar.customize")}
        title={t("projects.toolbar.customize")}
        aria-expanded={projectToolbarPanel === "customize"}
        onclick={() => onToggleToolbarPanel("customize")}
      >
        <Columns3 size={14} strokeWidth={1.75} />
      </button>
      <button
        type="button"
        data-project-toolbar-trigger="group"
        class={toolbarIconButtonClass(taskGroupingActive, projectToolbarPanel === "group")}
        aria-label={t("projects.toolbar.group")}
        title={t("projects.toolbar.group")}
        aria-expanded={projectToolbarPanel === "group"}
        onclick={() => onToggleToolbarPanel("group")}
      >
        <Layers size={14} strokeWidth={1.75} />
      </button>
      <button
        type="button"
        data-project-toolbar-trigger="settings"
        class={toolbarIconButtonClass(false, projectToolbarPanel === "settings")}
        aria-label={t("projects.header.projectSettings")}
        title={t("projects.header.projectSettings")}
        aria-expanded={projectToolbarPanel === "settings"}
        onclick={() => onToggleToolbarPanel("settings")}
      >
        <Settings2 size={14} strokeWidth={1.75} />
      </button>
    </div>
  {/if}
</div>
