<script lang="ts">
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import Hash from "@lucide/svelte/icons/hash";
  import Menu from "@lucide/svelte/icons/menu";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import { summarizeChannelMembers } from "$lib/chat/teammates/channel-members";
  import {
    COMPACT_IDENTITY_EMOJI_SCALE,
    COMPACT_IDENTITY_ICON_SIZE,
    COMPACT_IDENTITY_ICON_STROKE_WIDTH,
  } from "$lib/icon-sizing";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { projectLifecycleBadgeClass, projectLifecycleLabel } from "$lib/projects/display";
  import { projectNavigatorPanelGeometry, type ProjectNavigatorPanelMode } from "$lib/projects/toolbar";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import { cn } from "$lib/utils";
  import ChatParticipantAvatar from "$lib/components/chat/identity/ChatParticipantAvatar.svelte";
  import LocalPersonAvatar from "$lib/components/contacts/LocalPersonAvatar.svelte";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";
  import ProjectPickerMobileDialog from "$lib/components/projects/pickers/ProjectPickerMobileDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import WorkspaceBreadcrumbTerminalIcon from "$lib/components/ui/WorkspaceBreadcrumbTerminalIcon.svelte";
  import ChatChannelPickerPanel from "$lib/components/chat/channels/ChatChannelPickerPanel.svelte";
  import ChatProjectNavigator from "$lib/components/chat/channels/ChatProjectNavigator.svelte";
  import { CHAT_OPEN_MEMBERS_EVENT, readChatOpenMembersDetail } from "$lib/components/chat/channels/members-panel-events";
  import ChatTitleEditor from "./ChatTitleEditor.svelte";

  type ChatNavigatorMode = ProjectNavigatorPanelMode | "channels";

  let {
    explorerExpanded,
    showRailButton,
    onOpenRail,
    reserveGlobalActions,
    mobilePresentation = false,
    editingTitle = $bindable(false),
  }: {
    explorerExpanded: boolean;
    showRailButton: boolean;
    onOpenRail: () => void;
    reserveGlobalActions: boolean;
    mobilePresentation?: boolean;
    editingTitle?: boolean;
  } = $props();

  const chat = getChat();
  const projects = getProjects();
  const viewport = getViewport();
  const { t, locale } = getLocalization();
  const identityIconSize = COMPACT_IDENTITY_ICON_SIZE;
  const identityIconStrokeWidth = COMPACT_IDENTITY_ICON_STROKE_WIDTH;
  const identityEmojiScale = COMPACT_IDENTITY_EMOJI_SCALE;
  /** Avatars shown in the header trigger before the count takes over. */
  const MEMBER_STACK_LIMIT = 3;
  const MEMBER_STACK_AVATAR_SIZE = 18;
  type ChatChannelMembersPanelComponent =
    typeof import("$lib/components/chat/channels/ChatChannelMembersPanel.svelte").default;
  let navigatorOpen = $state(false);
  let membersChannelId = $state<string | null>(null);
  let membersPickerOnOpen = $state(false);
  let MembersPanel = $state<ChatChannelMembersPanelComponent | null>(null);
  let membersPanelLoad: Promise<void> | null = null;
  let membersTrigger = $state<HTMLButtonElement | null>(null);
  let navigatorMode = $state<ChatNavigatorMode>("groups");
  let navigatorAnchorElement = $state<HTMLButtonElement | null>(null);
  let groupTriggerElement = $state<HTMLButtonElement | null>(null);
  let projectTriggerElement = $state<HTMLButtonElement | null>(null);
  let channelTriggerElement = $state<HTMLButtonElement | null>(null);
  let identityElement = $state<HTMLDivElement | null>(null);
  let headerElement = $state<HTMLDivElement | null>(null);
  let navigatorPanelElement = $state<HTMLDivElement | null>(null);
  let navigatorPanelStyle = $state("");
  let navigatorPanelMaxHeight = $state(0);
  let showInactiveProjects = $state(false);
  let actionError = $state<string | null>(null);
  const selectedProject = $derived(projects.selectedProject);
  const selectedGroup = $derived(projects.selectedGroup);
  const selectedChannel = $derived(chat.selectedChannel);
  const selectedProjectChannels = $derived(
    selectedProject ? chat.channelsForProject(selectedProject.id) : [],
  );
  const selectedFolder = $derived(chat.selectedWorkingFolder);
  const showChannelBreadcrumb = $derived(!explorerExpanded || (editingTitle && !!selectedChannel && !selectedChannel.isDefault));
  const memberSummary = $derived(summarizeChannelMembers(selectedChannel?.memberships ?? []));
  const memberStackTeammates = $derived(chat.teammates
    .filter((teammate) => memberSummary.teammateIds.has(teammate.participant.id))
    .slice(0, MEMBER_STACK_LIMIT - 1));

  function triggerForMode(mode: ChatNavigatorMode): HTMLButtonElement | null {
    if (mode === "groups") return groupTriggerElement;
    if (mode === "projects") return projectTriggerElement;
    return channelTriggerElement;
  }

  function refreshNavigatorGeometry(): void {
    if (mobilePresentation) {
      navigatorPanelStyle = "";
      navigatorPanelMaxHeight = 0;
      return;
    }
    if (!navigatorOpen || !navigatorAnchorElement) return;
    const anchor = navigatorAnchorElement.getBoundingClientRect();
    const rect = headerElement?.closest(".chat-workspace")?.getBoundingClientRect();
    const geometry = projectNavigatorPanelGeometry({
      anchorLeft: anchor.left,
      anchorBottom: anchor.bottom,
      viewportWidth: viewport.width,
      viewportHeight: viewport.height,
      boundsLeft: rect?.left ?? 0,
      boundsRight: rect?.right ?? viewport.width,
      boundsTop: rect?.top ?? 0,
      boundsBottom: rect?.bottom ?? viewport.height,
    });
    navigatorPanelStyle = `left:${Math.round(geometry.left)}px;top:${Math.round(geometry.top)}px;width:${Math.round(geometry.width)}px`;
    navigatorPanelMaxHeight = geometry.height;
  }

  function openNavigator(mode: ChatNavigatorMode): void {
    if (editingTitle) return;
    navigatorMode = mode;
    navigatorAnchorElement = triggerForMode(mode);
    navigatorOpen = true;
    requestAnimationFrame(refreshNavigatorGeometry);
  }

  function toggleNavigator(mode: ChatNavigatorMode): void {
    if (navigatorOpen && navigatorMode === mode) navigatorOpen = false;
    else openNavigator(mode);
  }

  function handleWindowPointerDown(event: PointerEvent): void {
    if (mobilePresentation) return;
    const target = event.target;
    if (!(target instanceof Node)) return;
    if (navigatorOpen && !identityElement?.contains(target) && !navigatorPanelElement?.contains(target)) navigatorOpen = false;
  }

  function selectProject(): void {
    navigatorOpen = false;
  }

  async function selectChannel(channelId: string): Promise<void> {
    await chat.selectChannel(channelId);
    navigatorOpen = false;
  }

  function createChannel(): void {
    navigatorOpen = false;
    window.dispatchEvent(new Event("ganbaru-ai:chat-new-channel"));
  }

  const membersOpen = $derived(membersChannelId !== null && membersChannelId === chat.selectedChannelId);

  /** Load the members panel only when the trigger is approached or used. */
  function loadMembersPanel(): Promise<void> {
    if (MembersPanel) return Promise.resolve();
    membersPanelLoad ??= import("$lib/components/chat/channels/ChatChannelMembersPanel.svelte")
      .then((module) => { MembersPanel = module.default; })
      .catch((error: unknown) => {
        console.error("Load Chat members panel failed", error);
        membersChannelId = null;
      })
      .finally(() => { membersPanelLoad = null; });
    return membersPanelLoad;
  }

  function toggleChannelMembers(): void {
    if (!selectedChannel) return;
    membersPickerOnOpen = false;
    membersChannelId = membersOpen ? null : selectedChannel.id;
    if (membersChannelId) void loadMembersPanel();
  }

  function closeChannelMembers(): void {
    membersChannelId = null;
    membersPickerOnOpen = false;
  }

  /** Reopens the panel for a surface that had to close while Settings covered it. */
  $effect(() => {
    const openMembers = (event: Event): void => {
      const detail = readChatOpenMembersDetail(event);
      if (!detail || detail.channelId !== chat.selectedChannelId) return;
      membersPickerOnOpen = detail.addMembers;
      membersChannelId = detail.channelId;
      void loadMembersPanel();
    };
    window.addEventListener(CHAT_OPEN_MEMBERS_EVENT, openMembers);
    return () => window.removeEventListener(CHAT_OPEN_MEMBERS_EVENT, openMembers);
  });

  async function commitTitle(title: string): Promise<void> {
    if (!selectedChannel || selectedChannel.isDefault || !editingTitle) return;
    await chat.updateChannelDetails(selectedChannel, title, selectedChannel.topic);
    editingTitle = false;
  }

  function run(action: () => Promise<unknown>): void {
    actionError = null;
    void action().catch((error: unknown) => { actionError = error instanceof Error ? error.message : String(error); });
  }

  $effect(() => {
    if (!navigatorOpen) return;
    void viewport.width;
    void viewport.height;
    requestAnimationFrame(refreshNavigatorGeometry);
  });

  $effect(() => {
    if (explorerExpanded && navigatorOpen && navigatorMode === "channels") navigatorOpen = false;
  });

</script>

<svelte:window onpointerdown={handleWindowPointerDown} />

<div bind:this={headerElement} class={cn("chat-workspace-header flex min-w-0 items-center gap-1", mobilePresentation ? "overflow-hidden pl-3" : "overflow-x-auto pl-3")} class:mobilePresentation style={`height:var(--toolbar-h);background-color:var(--cal-bg);padding-right:${mobilePresentation ? "0.75rem" : `var(--chat-header-action-inset, ${reserveGlobalActions ? "6.5rem" : "0.75rem"})`}`} onscroll={refreshNavigatorGeometry} data-chat-workspace-header>
  {#if showRailButton}<button type="button" class="chat-toolbar-icon-button" aria-label={t("chat.openRail")} onclick={onOpenRail}><Menu size={14} /></button>{/if}
  <div bind:this={identityElement} class={cn("relative", mobilePresentation ? "min-w-0 flex-1 overflow-hidden" : "min-w-36 shrink-0 min-[760px]:max-w-3xl")}>
    <div class={cn("flex min-w-0 max-w-full items-center gap-0.5 overflow-hidden text-identity font-medium", mobilePresentation ? "h-12" : "h-7")}>
      {#if selectedProject && selectedGroup}
        <button bind:this={groupTriggerElement} type="button" class={cn("chat-context-segment", navigatorOpen && navigatorMode === "groups" && "bg-accent")} aria-label={t("projects.navigator.open")} aria-expanded={navigatorOpen && navigatorMode === "groups"} data-chat-group-trigger onpointerenter={() => { if (!mobilePresentation) openNavigator("groups"); }} onclick={() => toggleNavigator("groups")}>{#if !mobilePresentation}<ProjectIcon name={selectedGroup.icon} size={identityIconSize} strokeWidth={identityIconStrokeWidth} emojiScale={identityEmojiScale} />{/if}<span class="truncate">{selectedGroup.name}</span></button>
        <span class="chat-context-divider">/</span>
        <button bind:this={projectTriggerElement} type="button" class={cn("chat-context-segment", navigatorOpen && navigatorMode === "projects" && "bg-accent")} aria-label={t("projects.navigator.open")} aria-expanded={navigatorOpen && navigatorMode === "projects"} data-chat-project-trigger onpointerenter={() => { if (!mobilePresentation) openNavigator("projects"); }} onclick={() => toggleNavigator("projects")}>{#if !mobilePresentation}<ProjectIcon name={selectedProject.icon} size={identityIconSize} strokeWidth={identityIconStrokeWidth} emojiScale={identityEmojiScale} />{/if}<span class="truncate">{selectedProject.name}</span>{#if selectedProject.status !== "active"}<span class={cn("rounded border px-1.5 py-0.5 text-[0.666667rem]", projectLifecycleBadgeClass(selectedProject.status))}>{projectLifecycleLabel(selectedProject.status, t)}</span>{/if}{#if !selectedChannel || !showChannelBreadcrumb}<WorkspaceBreadcrumbTerminalIcon kind="chevron" context="chat" class="shrink-0 text-muted-foreground" />{/if}</button>
        {#if selectedChannel && showChannelBreadcrumb}
          <span class="chat-context-divider">/</span>
          {#if editingTitle && !selectedChannel.isDefault}
            <div class="min-w-36 max-w-64 px-1"><ChatTitleEditor title={selectedChannel.name} onCommit={commitTitle} onCancel={() => { editingTitle = false; }} /></div>
          {:else}
            <button bind:this={channelTriggerElement} type="button" class={cn("chat-context-segment", navigatorOpen && navigatorMode === "channels" && "bg-accent")} aria-label={t("chat.channels.navigatorLabel")} aria-expanded={navigatorOpen && navigatorMode === "channels"} data-chat-channel-trigger onpointerenter={() => { if (!mobilePresentation) openNavigator("channels"); }} onclick={() => toggleNavigator("channels")}>{#if !mobilePresentation}<Hash size={identityIconSize} strokeWidth={identityIconStrokeWidth} class="shrink-0" />{/if}<span class="truncate">{selectedChannel.name}</span><WorkspaceBreadcrumbTerminalIcon kind="chevron" context="chat" class="shrink-0 text-muted-foreground" /></button>
          {/if}
          {#if selectedChannel.topic && !explorerExpanded}<span class="hidden max-w-80 truncate px-1 text-xs text-muted-foreground @min-[760px]:inline" data-chat-channel-topic>{selectedChannel.topic}</span>{/if}
        {/if}
        <button type="button" class="chat-inline-new-button" aria-label={t("chat.channels.createTitle")} data-chat-new-channel-button onclick={() => window.dispatchEvent(new Event("ganbaru-ai:chat-new-channel"))}><WorkspaceBreadcrumbTerminalIcon kind="plus" /></button>
      {:else}
        <div class="chat-context-segment"><MessageSquare size={identityIconSize} /><span>{t("chat.title")}</span></div>
      {/if}
    </div>
    {#if navigatorOpen}
      {#if mobilePresentation}
        <ProjectPickerMobileDialog
          label={navigatorMode === "channels" ? t("chat.channels.navigatorLabel") : t("projects.navigator.pickerLabel")}
          closeLabel={t("common.close")}
          onClose={() => { navigatorOpen = false; }}
        >
          {#if navigatorMode === "channels"}
            <ChatChannelPickerPanel
              channels={selectedProjectChannels}
              selectedChannelId={selectedChannel?.id ?? null}
              frameStyle="height:100%;max-height:100%"
              iconStrokeWidth={identityIconStrokeWidth}
              onChannelSelected={(channel) => selectChannel(channel.id)}
              onCreateChannel={createChannel}
              mobileLayout={mobilePresentation}
              title={selectedProject?.name}
              onBack={() => { navigatorMode = "projects"; }}
              onClose={() => { navigatorOpen = false; }}
            />
          {:else}
            <ChatProjectNavigator
              selectedProjectId={projects.selectedProjectId}
              selectedGroupId={selectedGroup?.id ?? null}
              iconStrokeWidth={identityIconStrokeWidth}
              {showInactiveProjects}
              panelMode="groups"
              onShowInactiveProjectsChange={(value) => { showInactiveProjects = value; }}
              onProjectSelected={selectProject}
              onChannelSelected={() => { navigatorOpen = false; }}
              onCreateChannel={() => { createChannel(); }}
              mobileLayout={mobilePresentation}
              initialMobileGroupId={navigatorMode === "projects" ? selectedGroup?.id ?? null : null}
              onMobileProjectOpened={() => { navigatorMode = "channels"; }}
              onClose={() => { navigatorOpen = false; }}
            />
          {/if}
        </ProjectPickerMobileDialog>
      {:else}
        <div bind:this={navigatorPanelElement} class="fixed z-80" style={navigatorPanelStyle} role="dialog" tabindex="-1" aria-label={navigatorMode === "channels" ? t("chat.channels.navigatorLabel") : t("projects.navigator.pickerLabel")}>
          {#if navigatorMode === "channels"}
            <ChatChannelPickerPanel
              channels={selectedProjectChannels}
              selectedChannelId={selectedChannel?.id ?? null}
              frameStyle={`width:100%;max-height:${navigatorPanelMaxHeight}px`}
              iconStrokeWidth={identityIconStrokeWidth}
              onChannelSelected={(channel) => selectChannel(channel.id)}
              onCreateChannel={createChannel}
            />
          {:else}
            <ChatProjectNavigator
              selectedProjectId={projects.selectedProjectId}
              selectedGroupId={selectedGroup?.id ?? null}
              iconStrokeWidth={identityIconStrokeWidth}
              {showInactiveProjects}
              panelMode={navigatorMode}
              panelMaxHeight={navigatorPanelMaxHeight}
              onShowInactiveProjectsChange={(value) => { showInactiveProjects = value; }}
              onProjectSelected={selectProject}
              onChannelSelected={() => { navigatorOpen = false; }}
              onCreateChannel={() => { createChannel(); }}
            />
          {/if}
        </div>
      {/if}
    {/if}
  </div>
  {#if !mobilePresentation}<div class="flex-1"></div>{/if}
  {#if actionError}<Notice class="header-action-error max-w-48 text-[0.666667rem]" message={actionError} />{/if}
  <div class="flex shrink-0 items-center gap-1">
    {#if selectedFolder?.bindingStatus === "available"}<button type="button" class="chat-toolbar-icon-button" title={t("chat.openFolder")} aria-label={t("chat.openFolder")} onclick={() => run(() => chat.openWorkingFolder(selectedFolder.workingFolder.id))}><FolderOpen size={14} /></button>{/if}
    {#if selectedChannel}
      <button bind:this={membersTrigger} type="button" class="chat-members-trigger" class:open={membersOpen} aria-label={t("chat.organization.membersPanel", memberSummary.count)} data-app-tooltip={t("chat.organization.members")} aria-haspopup="dialog" aria-expanded={membersOpen} data-chat-members-trigger onpointerenter={() => { void loadMembersPanel(); }} onfocus={() => { void loadMembersPanel(); }} onclick={toggleChannelMembers}>
        <span class="chat-members-stack" aria-hidden="true">
          <LocalPersonAvatar size={MEMBER_STACK_AVATAR_SIZE} />
          {#each memberStackTeammates as teammate (teammate.participant.id)}<ChatParticipantAvatar participant={teammate.participant} {teammate} size={MEMBER_STACK_AVATAR_SIZE} />{/each}
        </span>
        <span class="chat-members-count tabular-nums">{formatNumber(locale, memberSummary.count)}</span>
      </button>
    {/if}
    {#if selectedFolder?.currentBranch}<span class="chat-branch" title={t("chat.header.branch", selectedFolder.currentBranch)}><GitBranch size={13} /><span>{selectedFolder.currentBranch}</span></span>{/if}
  </div>
</div>

{#if membersOpen && selectedChannel && MembersPanel}
  {@const LoadedMembersPanel = MembersPanel}
  <LoadedMembersPanel anchor={membersTrigger} channel={selectedChannel} mobileLayout={mobilePresentation} initialPickerOpen={membersPickerOnOpen} onClose={closeChannelMembers} />
{/if}

<style>
  .chat-workspace-header :global(.header-action-error .notice-message) { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chat-workspace-header.mobilePresentation { scrollbar-width: none; }
  .chat-workspace-header.mobilePresentation::-webkit-scrollbar { display: none; }
  .chat-context-segment { display:flex;height:1.75rem;min-width:0;align-items:center;gap:0.375rem;border-radius:0.375rem;padding-inline:0.375rem;text-align:left; }
  button.chat-context-segment:hover { background:var(--accent);color:var(--accent-foreground); }
  .chat-context-divider { flex:0 0 auto;padding-inline:0.125rem;font-weight:600;color:var(--muted-foreground); }
  .chat-inline-new-button,.chat-toolbar-icon-button { display:flex;height:1.75rem;width:1.75rem;flex:0 0 auto;align-items:center;justify-content:center;border-radius:0.375rem;color:var(--foreground); }
  .chat-inline-new-button:hover,.chat-toolbar-icon-button:hover { background:var(--accent); }
  .chat-members-trigger { display:flex;height:1.75rem;flex:0 0 auto;align-items:center;gap:0.3rem;border-radius:0.375rem;padding-inline:0.3rem 0.5rem;color:var(--foreground); }
  .chat-members-trigger:hover,.chat-members-trigger.open { background:var(--accent); }
  .chat-members-stack { display:flex;align-items:center; }
  .chat-members-stack > :global(span) { flex:0 0 auto;border-radius:22%;box-shadow:0 0 0 1.5px var(--cal-bg); }
  .chat-members-trigger:hover .chat-members-stack > :global(span),.chat-members-trigger.open .chat-members-stack > :global(span) { box-shadow:0 0 0 1.5px var(--accent); }
  .chat-members-stack > :global(span + span) { margin-inline-start:-0.3rem; }
  .chat-members-count { font-size: calc(0.733333rem * var(--type-scale));font-weight:500; }
  .chat-branch { display:none;min-width:0;max-width:9rem;align-items:center;gap:0.3rem;border-radius:0.375rem;padding:0.25rem 0.4rem;color:var(--muted-foreground);font-size: calc(0.666667rem * var(--type-scale)); }
  .chat-branch span { overflow:hidden;text-overflow:ellipsis;white-space:nowrap; }
  @container chat-shell (min-width:760px) { .chat-branch { display:inline-flex; } }
  @container chat-shell (max-width: 560px) {
    .chat-workspace-header.mobilePresentation { gap: 0; }
    .chat-workspace-header.mobilePresentation > .relative { min-width: 0; flex: 1 1 auto; }
    .mobilePresentation [data-chat-group-trigger], .mobilePresentation [data-chat-project-trigger], .mobilePresentation [data-chat-channel-trigger] { height: 3rem; min-width: 0; flex: 0 1 auto; font-size: calc(0.9rem * var(--type-scale)); }
    .mobilePresentation .chat-inline-new-button { display: none; }
    .mobilePresentation .chat-toolbar-icon-button { width: 2.5rem; height: 2.5rem; }
    .mobilePresentation .chat-members-trigger { height: 2.5rem; padding-inline: 0.5rem 0.625rem; }
  }
</style>
