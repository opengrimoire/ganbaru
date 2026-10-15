<script lang="ts">
  import ArrowUpDown from "@lucide/svelte/icons/arrow-up-down";
  import Check from "@lucide/svelte/icons/check";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
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
    projectNavigatorPanelGeometry,
    projectToolbarPanelsForView,
    type ProjectToolbarPanel,
    type ProjectNavigatorPanelMode,
  } from "$lib/projects/toolbar";
  import {
    type Project,
    type ProjectChatIntegration,
    type ProjectGroup,
    type ProjectView,
  } from "$lib/projects/types";
  import { useProjects } from "$lib/stores/projects.svelte";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import { cn } from "$lib/utils";
  import { createHoverMenu } from "$lib/utils/menu-aim";
  import ProjectIcon from "./ProjectIcon.svelte";
  import WorkspaceBreadcrumbTerminalIcon from "$lib/components/ui/WorkspaceBreadcrumbTerminalIcon.svelte";
  import ProjectNavigator from "./ProjectNavigator.svelte";
  import ProjectPickerMobileDialog from "$lib/components/projects/pickers/ProjectPickerMobileDialog.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import MobileContextSwitcher from "$lib/components/mobile/MobileContextSwitcher.svelte";
  import ProjectMobileViewSwitcher from "./ProjectMobileViewSwitcher.svelte";

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
    onOpenSidebar = undefined,
    onSelectView = undefined,
    mobileLayout = false,
    projectChat = null,
    viewControlsTarget = $bindable(null),
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
    /** Opens the mobile Projects sidebar, where views are chosen and managed. */
    onOpenSidebar?: () => void;
    /** Opens a view of the selected project from the phone toolbar view switcher. */
    onSelectView?: (view: ProjectView) => void;
    mobileLayout?: boolean;
    projectChat?: ProjectChatIntegration | null;
    /** The header slot where the active view may show its own controls, or null while they do not fit. */
    viewControlsTarget?: HTMLElement | null;
  } = $props();

  const projects = useProjects();
  const mobileBackStack = getMobileBackStack();
  const viewport = getViewport();
  const { t } = getLocalization();
  const projectIdentityIconSize = COMPACT_IDENTITY_ICON_SIZE;
  const projectIdentityIconStrokeWidth = COMPACT_IDENTITY_ICON_STROKE_WIDTH;
  const projectIdentityEmojiScale = COMPACT_IDENTITY_EMOJI_SCALE;

  let projectNavigatorOpen = $state(false);
  let mobileCustomizationMenuOpen = $state(false);
  let projectNavigatorMode = $state<ProjectNavigatorPanelMode>("groups");
  let projectHeaderElement = $state<HTMLDivElement | null>(null);
  let headerSpacerElement = $state<HTMLDivElement | null>(null);
  let viewControlsSlot = $state<HTMLDivElement | null>(null);
  let viewControlsFit = $state(true);
  let viewControlsShown = $state(false);
  let viewControlsWidth = 0;
  let projectNavigatorAnchorElement = $state<HTMLButtonElement | null>(null);
  let projectGroupTriggerElement = $state<HTMLButtonElement | null>(null);
  let projectTriggerElement = $state<HTMLButtonElement | null>(null);
  let projectNavigatorPanelElement = $state<HTMLDivElement | null>(null);
  let projectNavigatorPanelStyle = $state("");
  let projectNavigatorPanelMaxHeight = $state(0);
  const projectNavigatorHover = createHoverMenu({
    isOpen: () => projectNavigatorOpen,
    close: () => {
      projectNavigatorOpen = false;
    },
  });
  $effect(() => {
    if (!projectNavigatorOpen) projectNavigatorHover.cancel();
  });
  $effect(() => () => projectNavigatorHover.cancel());
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

  function toolbarIconButtonClass(active: boolean, open = false): string {
    return cn(
      "flex shrink-0 items-center justify-center text-foreground transition-colors hover:bg-accent",
      mobileLayout ? "mobile-chrome-button w-(--mobile-chrome-column)" : "h-7 w-7 rounded-md",
      (active || open) && "bg-accent",
    );
  }

  const toolbarPanels = $derived(projects.activeTaskView ? projectToolbarPanelsForView(projects.activeTaskView) : (["settings"] as const));
  const customizationPanels = $derived(toolbarPanels.filter((panel) => panel !== "settings"));

  function panelIcon(panel: ProjectToolbarPanel) {
    if (panel === "filters") return ListFilter;
    if (panel === "sort") return ArrowUpDown;
    if (panel === "customize") return Columns3;
    if (panel === "group") return Layers;
    return Settings2;
  }

  function panelLabel(panel: ProjectToolbarPanel): string {
    if (panel === "filters") return t("projects.filters.title");
    if (panel === "sort") return t("projects.toolbar.sort");
    if (panel === "customize") return t("projects.toolbar.customize");
    if (panel === "group") return t("projects.toolbar.group");
    return t("projects.header.projectSettings");
  }

  function panelActive(panel: ProjectToolbarPanel): boolean {
    if (panel === "filters") return taskFiltersActive;
    if (panel === "customize") return taskCustomizeActive;
    if (panel === "group") return taskGroupingActive;
    return false;
  }

  function closeMobileMenus(): void {
    mobileCustomizationMenuOpen = false;
  }

  function selectMobileCustomization(panel: ProjectToolbarPanel): void {
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
    const triggerPanelOpen = projectNavigatorOpen && projectNavigatorMode === mode;
    if (projectNavigatorHover.claimClick(triggerPanelOpen)) return;
    if (triggerPanelOpen) {
      projectNavigatorOpen = false;
      return;
    }
    openProjectNavigator(mode);
  }

  function closeProjectNavigator(): void {
    projectNavigatorOpen = false;
  }

  $effect(() => {
    if (!mobileLayout || !mobileCustomizationMenuOpen) return;
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

  $effect(() => {
    viewControlsTarget = !mobileLayout && viewControlsFit ? viewControlsSlot : null;
  });

  $effect(() => {
    if (mobileLayout || !projectHeaderElement || !viewControlsSlot || !headerSpacerElement) return;
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(refreshViewControlsFit);
    observer.observe(projectHeaderElement);
    observer.observe(viewControlsSlot);
    observer.observe(headerSpacerElement);
    return () => observer.disconnect();
  });

  /**
   * Keeps view controls in the header row only while the whole row fits. Shown controls leave as soon as the row
   * overflows, and return once the free space can hold their last measured width.
   */
  function refreshViewControlsFit(): void {
    if (!projectHeaderElement || !viewControlsSlot || !headerSpacerElement) return;
    const slotWidth = viewControlsSlot.getBoundingClientRect().width;
    viewControlsShown = slotWidth > 0;
    if (slotWidth > 0) {
      viewControlsWidth = slotWidth;
      if (projectHeaderElement.scrollWidth > projectHeaderElement.clientWidth + 1) viewControlsFit = false;
      return;
    }
    const gap = Number.parseFloat(getComputedStyle(projectHeaderElement).columnGap);
    const requiredWidth = viewControlsWidth + (Number.isFinite(gap) ? gap : 0) + 1;
    viewControlsFit = viewControlsWidth === 0
      || headerSpacerElement.getBoundingClientRect().width >= requiredWidth;
  }

</script>

<svelte:window onpointerdown={handleProjectWindowPointerDown} />

<div
  bind:this={projectHeaderElement}
  class={cn(
    "flex shrink-0 items-center",
    mobileLayout
      ? "gap-0 overflow-visible px-(--mobile-chrome-inset)"
      : viewControlsShown ? "gap-1 overflow-visible px-3" : "hide-scrollbar gap-1 overflow-x-auto px-3",
  )}
  style="height: var(--toolbar-h); background-color: var(--cal-bg);"
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
    {#if mobileLayout}
      <MobileContextSwitcher
        bind:triggerElement={projectTriggerElement}
        title={selectedProject.name}
        expanded={projectNavigatorOpen}
        onclick={() => toggleProjectNavigator("projects")}
      >
        {#snippet icon()}
          <ProjectIcon
            name={selectedProject.icon}
            size={projectIdentityIconSize}
            strokeWidth={projectIdentityIconStrokeWidth}
            emojiScale={projectIdentityEmojiScale}
          />
        {/snippet}
      </MobileContextSwitcher>
      <div class="flex h-12 min-w-0 items-center">
        {#if onOpenSidebar}
          <button
            type="button"
            class={toolbarIconButtonClass(false)}
            aria-label={t("projects.sidebar.expand")}
            title={t("projects.sidebar.expand")}
            data-project-mobile-sidebar-button
            onclick={() => {
              closeMobileMenus();
              projectNavigatorOpen = false;
              onOpenSidebar();
            }}
          >
            <PanelLeft size={16} strokeWidth={1.75} />
          </button>
        {/if}
        {#if onSelectView}
          <ProjectMobileViewSwitcher projectId={selectedProject.id} {onSelectView} />
        {/if}
      </div>
    {:else}
    <div class="flex h-7 min-w-0 max-w-full items-center gap-0.5 text-identity font-medium">
      <button
        bind:this={projectGroupTriggerElement}
        type="button"
        class={cn(
          "flex h-7 min-w-0 items-center gap-1.5 rounded-md px-1.5 text-left hover:bg-accent",
          projectNavigatorOpen && projectNavigatorMode === "groups" && "bg-accent",
        )}
        aria-label={t("projects.navigator.open")}
        aria-haspopup="dialog"
        aria-expanded={projectNavigatorOpen && projectNavigatorMode === "groups"}
        onpointerenter={(event) => { projectNavigatorHover.enterTrigger(event, () => openProjectNavigator("groups")); }}
        onpointerleave={projectNavigatorHover.leave}
        onclick={() => toggleProjectNavigator("groups")}
      >
        <ProjectIcon
          name={selectedGroup.icon}
          size={projectIdentityIconSize}
          strokeWidth={projectIdentityIconStrokeWidth}
          emojiScale={projectIdentityEmojiScale}
          class="shrink-0"
        />
        <span class="min-w-0 truncate text-foreground">{selectedGroup.name}</span>
      </button>
      <span class="shrink-0 px-0.5 font-semibold text-muted-foreground">/</span>
      <button
        bind:this={projectTriggerElement}
        type="button"
        class={cn(
          "flex h-7 min-w-0 items-center gap-1.5 rounded-md px-1.5 text-left hover:bg-accent",
          projectNavigatorOpen && projectNavigatorMode === "projects" && "bg-accent",
        )}
        aria-label={t("projects.navigator.open")}
        aria-haspopup="dialog"
        aria-expanded={projectNavigatorOpen && projectNavigatorMode === "projects"}
        onpointerenter={(event) => { projectNavigatorHover.enterTrigger(event, () => openProjectNavigator("projects")); }}
        onpointerleave={projectNavigatorHover.leave}
        onclick={() => toggleProjectNavigator("projects")}
      >
        <ProjectIcon
          name={selectedProject.icon}
          size={projectIdentityIconSize}
          strokeWidth={projectIdentityIconStrokeWidth}
          emojiScale={projectIdentityEmojiScale}
          class="shrink-0"
        />
        <span class="min-w-0 truncate text-foreground">{selectedProject.name}</span>
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
    {/if}
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
          onpointerenter={projectNavigatorHover.enterPanel}
          onpointerleave={projectNavigatorHover.leave}
          onpointerdown={projectNavigatorHover.pin}
          onkeydown={projectNavigatorHover.pin}
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
            onClose={closeProjectNavigator}
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
          class={toolbarIconButtonClass(
            customizationPanels.some(panelActive),
            mobileCustomizationMenuOpen,
          )}
          aria-label={t("projects.toolbar.customization")}
          title={t("projects.toolbar.customization")}
          aria-haspopup="menu"
          aria-expanded={mobileCustomizationMenuOpen}
          onclick={() => {
            projectNavigatorOpen = false;
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
            {#each customizationPanels as panel (panel)}
              {@const Icon = panelIcon(panel)}
              {@const label = panelLabel(panel)}
              {@const active = panelActive(panel) || projectToolbarPanel === panel}
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
    <div bind:this={headerSpacerElement} class="flex-1"></div>
    <div class="flex shrink-0 items-center gap-1">
      <div bind:this={viewControlsSlot} class="flex shrink-0 items-center empty:hidden"></div>
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
      {#each toolbarPanels as panel (panel)}
        {@const Icon = panelIcon(panel)}
        {@const label = panelLabel(panel)}
        <button
          type="button"
          data-project-toolbar-trigger={panel}
          class={toolbarIconButtonClass(panelActive(panel), projectToolbarPanel === panel)}
          aria-label={label}
          title={label}
          aria-expanded={projectToolbarPanel === panel}
          onclick={() => onToggleToolbarPanel(panel)}
        >
          <Icon size={14} strokeWidth={1.75} />
        </button>
      {/each}
    </div>
  {/if}
</div>
