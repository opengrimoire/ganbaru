<script lang="ts">
  import { tick } from "svelte";
  import type { ChatChannelRead } from "$lib/chat/contracts";
  import {
    isPointerAimingAtSubmenu,
    SUBMENU_AIM_TOLERANCES,
    type MenuAimPoint,
  } from "$lib/utils/menu-aim";
  import {
    projectPickerBridgeFrameStyle,
    projectPickerMenuAimRect,
    projectPickerPanelFrameStyle,
    projectPickerPointerPoint,
    projectPickerSubpanelAimOrigin,
    projectPickerSubpanelGeometry,
    projectPickerSubpanelSide,
  } from "$lib/projects/picker-panels";
  import type { ProjectNavigatorPanelMode } from "$lib/projects/toolbar";
  import type { Project } from "$lib/projects/types";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { cn, type MaybePromise } from "$lib/utils";
  import ProjectPickerPanels from "$lib/components/projects/pickers/ProjectPickerPanels.svelte";
  import ChatChannelPickerPanel from "./ChatChannelPickerPanel.svelte";


  let {
    selectedProjectId,
    selectedGroupId = null,
    showInactiveProjects,
    iconStrokeWidth = 1.6,
    panelMode = "groups",
    panelMaxHeight = null,
    onShowInactiveProjectsChange,
    onProjectSelected,
    onChannelSelected,
    onCreateChannel,
    mobileLayout = false,
    initialMobileGroupId = null,
    onMobileProjectOpened = undefined,
    onClose = undefined,
  }: {
    selectedProjectId: string | null;
    selectedGroupId?: string | null;
    showInactiveProjects: boolean;
    iconStrokeWidth?: number;
    panelMode?: ProjectNavigatorPanelMode;
    panelMaxHeight?: number | null;
    onShowInactiveProjectsChange: (value: boolean) => void;
    onProjectSelected: () => MaybePromise<void>;
    onChannelSelected: () => MaybePromise<void>;
    onCreateChannel: (projectId: string) => MaybePromise<void>;
    mobileLayout?: boolean;
    initialMobileGroupId?: string | null;
    onMobileProjectOpened?: (projectId: string) => MaybePromise<void>;
    onClose?: () => void;
  } = $props();

  const chat = getChat();
  const projects = getProjects();
  const panelGap = 4;
  const panelListPadding = 8;
  const panelRowHeight = 32;
  const panelFooterHeight = 44;
  const zIndexClass = "z-81";

  let projectSearch = $state("");
  let activeProjectId = $state<string | null>(null);
  let activeProjectAnchorElement = $state<HTMLElement | null>(null);
  let activeProjectPanelElement = $state<HTMLDivElement | null>(null);
  let channelPanelElement = $state<HTMLDivElement | undefined>();
  let channelPanelBridgeElement = $state<HTMLDivElement | undefined>();
  let channelPanelStyle = $state("");
  let channelPanelBridgeStyle = $state("");
  let previousPanelMode = $state<ProjectNavigatorPanelMode | null>(null);

  const activeProjectChannels = $derived(
    activeProjectId ? chat.channelsForProject(activeProjectId) : [],
  );

  function currentBounds() {
    const margin = 8;
    const viewportBounds = {
      left: margin,
      right: window.innerWidth - margin,
      top: margin,
      bottom: window.innerHeight - margin,
    };
    const rect = activeProjectPanelElement?.closest(".chat-workspace")?.getBoundingClientRect();
    if (!rect) return viewportBounds;
    return {
      left: Math.max(rect.left, viewportBounds.left),
      right: Math.min(rect.right, viewportBounds.right),
      top: Math.max(rect.top, viewportBounds.top),
      bottom: Math.min(rect.bottom, viewportBounds.bottom),
    };
  }

  function pointerAimingAtChannelPanel(point: MenuAimPoint): boolean {
    if (!activeProjectAnchorElement || !channelPanelElement) return false;
    const anchorRect = activeProjectAnchorElement.getBoundingClientRect();
    const panelRect = channelPanelElement.getBoundingClientRect();
    const side = projectPickerSubpanelSide(anchorRect, panelRect);
    return isPointerAimingAtSubmenu({
      origin: projectPickerSubpanelAimOrigin(anchorRect),
      point,
      submenu: projectPickerMenuAimRect(panelRect),
      side,
      ...SUBMENU_AIM_TOLERANCES,
    });
  }

  function updateChannelPanelGeometry(): void {
    if (!activeProjectAnchorElement || !activeProjectPanelElement) return;
    const geometry = projectPickerSubpanelGeometry({
      anchorRect: activeProjectAnchorElement.getBoundingClientRect(),
      panelRect: activeProjectPanelElement.getBoundingClientRect(),
      bounds: currentBounds(),
      gap: panelGap,
      footerHeight: panelFooterHeight,
      panelHeight: channelPanelElement?.getBoundingClientRect().height || undefined,
      projectCount: Math.max(1, activeProjectChannels.length),
      visibleRows: null,
      listPadding: panelListPadding,
      rowHeight: panelRowHeight,
    });
    channelPanelStyle = projectPickerPanelFrameStyle(geometry.panel);
    channelPanelBridgeStyle = projectPickerBridgeFrameStyle(geometry.bridge);
  }

  function showChannelPanel(
    project: Project,
    anchor: HTMLElement,
    sourcePanel: HTMLDivElement | null,
  ): void {
    activeProjectId = project.id;
    activeProjectAnchorElement = anchor;
    activeProjectPanelElement = sourcePanel;
    updateChannelPanelGeometry();
    void tick().then(updateChannelPanelGeometry);
  }

  function closeChannelPanel(): void {
    activeProjectId = null;
    activeProjectAnchorElement = null;
    activeProjectPanelElement = null;
    channelPanelStyle = "";
    channelPanelBridgeStyle = "";
  }

  function channelBoundaryContains(target: EventTarget | null): boolean {
    if (!(target instanceof Node)) return false;
    return Boolean(
      activeProjectAnchorElement?.contains(target)
      || activeProjectPanelElement?.contains(target)
      || channelPanelBridgeElement?.contains(target)
      || channelPanelElement?.contains(target),
    );
  }

  function handleChannelBoundaryLeave(event: PointerEvent): void {
    if (channelBoundaryContains(event.relatedTarget)) return;
    if (pointerAimingAtChannelPanel(projectPickerPointerPoint(event))) return;
    closeChannelPanel();
  }

  async function selectProject(project: Project): Promise<void> {
    await projects.selectProject(project.id);
    closeChannelPanel();
    await onProjectSelected();
  }

  async function openMobileProject(project: Project): Promise<void> {
    await projects.selectProject(project.id);
    await chat.syncProjectSelection(project.id);
    closeChannelPanel();
    await onMobileProjectOpened?.(project.id);
  }

  async function selectChannel(channel: ChatChannelRead): Promise<void> {
    await chat.selectChannel(channel.id);
    closeChannelPanel();
    await onChannelSelected();
  }

  async function createChannel(): Promise<void> {
    const projectId = activeProjectId;
    if (!projectId) return;
    await projects.selectProject(projectId);
    await chat.syncProjectSelection(projectId);
    closeChannelPanel();
    await onCreateChannel(projectId);
  }

  $effect(() => {
    const nextPanelMode = panelMode;
    if (previousPanelMode === null) {
      previousPanelMode = nextPanelMode;
      return;
    }
    if (nextPanelMode !== previousPanelMode) {
      previousPanelMode = nextPanelMode;
      closeChannelPanel();
    }
  });

  $effect(() => {
    const channelCount = activeProjectChannels.length;
    void channelCount;
    requestAnimationFrame(updateChannelPanelGeometry);
  });
</script>

<ProjectPickerPanels
  {selectedProjectId}
  {selectedGroupId}
  mode={mobileLayout ? "groups" : panelMode}
  {iconStrokeWidth}
  {panelMaxHeight}
  mainVisibleRows={null}
  subpanelVisibleRows={null}
  boundsSelector=".chat-workspace"
  zIndexClass="z-81"
  {showInactiveProjects}
  showInactiveToggle
  showLifecycleBadges
  showProjectChildren
  {activeProjectId}
  bind:projectSearch
  onShowInactiveProjectsChange={onShowInactiveProjectsChange}
  onProjectSelected={selectProject}
  onProjectPreview={showChannelPanel}
  onProjectPreviewClose={closeChannelPanel}
  projectChildContainsTarget={channelBoundaryContains}
  pointerAimingAtProjectChild={pointerAimingAtChannelPanel}
  {mobileLayout}
  {initialMobileGroupId}
  onProjectDrilldown={mobileLayout ? openMobileProject : undefined}
  {onClose}
/>

{#if !mobileLayout && activeProjectId && activeProjectAnchorElement && activeProjectPanelElement}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={channelPanelBridgeElement}
    aria-hidden="true"
    class={cn("fixed bg-transparent", zIndexClass)}
    style={channelPanelBridgeStyle}
    onpointerleave={handleChannelBoundaryLeave}
  ></div>
  <ChatChannelPickerPanel
    bind:rootElement={channelPanelElement}
    channels={activeProjectChannels}
    selectedChannelId={chat.selectedChannelId}
    showSearch={false}
    frameStyle={channelPanelStyle}
    className="fixed"
    {zIndexClass}
    {iconStrokeWidth}
    onLayoutChange={updateChannelPanelGeometry}
    onPointerLeave={handleChannelBoundaryLeave}
    onChannelSelected={selectChannel}
    onCreateChannel={createChannel}
  />
{/if}
