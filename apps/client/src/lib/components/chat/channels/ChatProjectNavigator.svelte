<script lang="ts">
  import { tick } from "svelte";
  import Hash from "@lucide/svelte/icons/hash";
  import { chatChannelSearchGroups } from "$lib/chat/channel-search";
  import type { ChatChannelRead } from "$lib/chat/contracts";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import StarredIconBadge from "$lib/components/ui/StarredIconBadge.svelte";
  import {
    createHoverDelay,
    isPointerAimingAtSubmenu,
    SUBMENU_AIM_TOLERANCES,
    type MenuAimPoint,
  } from "$lib/utils/menu-aim";
  import {
    projectPickerBridgeFrameStyle,
    projectPickerMenuAimRect,
    projectPickerPanelFrameStyle,
    projectPickerSubpanelAimOrigin,
    projectPickerSubpanelGeometry,
    projectPickerSubpanelSide,
  } from "$lib/projects/picker-panels";
  import type { ProjectNavigatorPanelMode } from "$lib/projects/toolbar";
  import type { Project } from "$lib/projects/types";
  import { useChat } from "$lib/stores/chat.svelte";
  import { getChatSidebarOrder } from "$lib/stores/chat/sidebar-order.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { getWorkspaceTabs } from "$lib/stores/workspace-tabs.svelte";
  import { openInNewTabIntent, preventMiddleClickAutoscroll, type OpenInNewTabIntent } from "$lib/workspace-tabs/gestures";
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

  const chat = useChat();
  const { t } = getLocalization();
  const chatSidebarOrder = getChatSidebarOrder();
  const projects = getProjects();
  const tabs = getWorkspaceTabs();
  const canOpenChannelTabs = $derived(tabs.canOpenTabs && !mobileLayout);
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
  const channelPanelDelay = createHoverDelay();
  $effect(() => () => channelPanelDelay.cancel());
  let previousPanelMode = $state<ProjectNavigatorPanelMode | null>(null);

  const activeProjectChannels = $derived(
    activeProjectId ? chatSidebarOrder.sortChannels(activeProjectId, chat.channelsForProject(activeProjectId)) : [],
  );
  /** The main panel search finds channels across the listed projects, in sidebar order. */
  const channelSearchGroups = $derived(chatChannelSearchGroups(
    projects.visibleGroups().flatMap((group) => showInactiveProjects
      ? projects.projectsForGroupIncludingInactive(group.id)
      : projects.projectsForGroup(group.id)),
    (projectId) => chatSidebarOrder.sortChannels(projectId, chat.channelsForProject(projectId)),
    projectSearch,
  ));
  const channelSearchRowCount = $derived(
    channelSearchGroups.reduce((count, group) => count + group.channels.length + 1, 0),
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
    channelPanelDelay.cancel();
    activeProjectId = project.id;
    activeProjectAnchorElement = anchor;
    activeProjectPanelElement = sourcePanel;
    updateChannelPanelGeometry();
    void tick().then(updateChannelPanelGeometry);
  }

  function closeChannelPanel(): void {
    channelPanelDelay.cancel();
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
    channelPanelDelay.schedule(closeChannelPanel);
  }

  async function selectProject(project: Project): Promise<void> {
    await projects.selectProject(project.id);
    closeChannelPanel();
    await onProjectSelected();
  }

  function openProjectInNewTab(project: Project, intent: OpenInNewTabIntent): void {
    closeChannelPanel();
    tabs.openLocation("project", { kind: "chat", projectId: project.id, item: null }, intent);
    if (!intent.background) onClose?.();
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

  async function selectSearchedChannel(channel: ChatChannelRead): Promise<void> {
    await selectChannel(channel);
    projectSearch = "";
  }

  function openChannelInNewTab(channel: ChatChannelRead, intent: OpenInNewTabIntent): void {
    tabs.openLocation("item", {
      kind: "chat",
      projectId: channel.projectId,
      item: { type: "chat-channel", id: channel.id },
    }, intent);
    if (intent.background) return;
    closeChannelPanel();
    onClose?.();
  }

  /** Route a search result click to a new tab when the gesture asks for one, else select the channel. */
  function handleSearchedChannelClick(event: MouseEvent, channel: ChatChannelRead): void {
    const intent = canOpenChannelTabs ? openInNewTabIntent(event) : null;
    if (intent) {
      event.preventDefault();
      openChannelInNewTab(channel, intent);
      return;
    }
    if (event.type === "click") void selectSearchedChannel(channel);
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

{#snippet channelSearchResults()}
  {#if channelSearchGroups.length === 0}
    <div class="px-3 py-2 text-popover-foreground/60">{t("chat.channels.noResults")}</div>
  {:else}
    <div class="grid px-1">
      {#each channelSearchGroups as resultGroup (resultGroup.project.id)}
        <div class="px-1 pb-1">
          <div class="menu-label">{resultGroup.project.name}</div>
          <div class="grid">
            {#each resultGroup.channels as channel (channel.id)}
              <button
                type="button"
                class={cn(
                  "menu-item gap-2 text-popover-foreground",
                  mobileLayout && "min-h-12 px-3 text-sm",
                  chat.selectedChannelId === channel.id && "font-medium",
                )}
                aria-current={chat.selectedChannelId === channel.id ? "page" : undefined}
                onmousedown={preventMiddleClickAutoscroll}
                onclick={(event) => handleSearchedChannelClick(event, channel)}
                onauxclick={(event) => handleSearchedChannelClick(event, channel)}
              >
                <StarredIconBadge starred={chatSidebarOrder.isChannelStarred(channel.id)}><Hash size={mobileLayout ? 18 : 13} strokeWidth={iconStrokeWidth} /></StarredIconBadge>
                <span class="min-w-0 flex-1 truncate">{channel.name}</span>
              </button>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  {/if}
{/snippet}

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
  showOrderMenu
  showLifecycleBadges
  showProjectChildren
  {activeProjectId}
  bind:projectSearch
  searchPlaceholder={t("chat.channels.search")}
  searchResults={channelSearchResults}
  searchResultCount={channelSearchRowCount}
  onShowInactiveProjectsChange={onShowInactiveProjectsChange}
  onProjectSelected={selectProject}
  onProjectOpenInNewTab={tabs.canOpenTabs && !mobileLayout ? openProjectInNewTab : undefined}
  onProjectPreview={showChannelPanel}
  onProjectPreviewClose={closeChannelPanel}
  projectChildContainsTarget={channelBoundaryContains}
  pointerAimingAtProjectChild={pointerAimingAtChannelPanel}
  projectChildHoverDelay={channelPanelDelay}
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
    onpointerenter={channelPanelDelay.cancel}
    onpointerleave={handleChannelBoundaryLeave}
  ></div>
  <ChatChannelPickerPanel
    bind:rootElement={channelPanelElement}
    channels={activeProjectChannels}
    orderProjectId={activeProjectId}
    selectedChannelId={chat.selectedChannelId}
    isChannelStarred={(channelId) => chatSidebarOrder.isChannelStarred(channelId)}
    showSearch={false}
    frameStyle={channelPanelStyle}
    className="fixed"
    {zIndexClass}
    {iconStrokeWidth}
    onLayoutChange={updateChannelPanelGeometry}
    onPointerEnter={channelPanelDelay.cancel}
    onPointerLeave={handleChannelBoundaryLeave}
    onChannelSelected={selectChannel}
    onChannelOpenInNewTab={canOpenChannelTabs ? openChannelInNewTab : undefined}
    onCreateChannel={createChannel}
  />
{/if}
