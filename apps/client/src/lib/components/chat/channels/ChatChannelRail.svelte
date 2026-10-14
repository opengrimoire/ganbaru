<script lang="ts">
  import { onMount, tick } from "svelte";
  import { COMPACT_IDENTITY_ICON_SIZE, COMPACT_IDENTITY_ICON_STROKE_WIDTH } from "$lib/icon-sizing";
  import SearchField from "$lib/components/ui/SearchField.svelte";
  import Archive from "@lucide/svelte/icons/archive";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ChevronsLeft from "@lucide/svelte/icons/chevrons-left";
  import ChevronsRight from "@lucide/svelte/icons/chevrons-right";
  import ContactRound from "@lucide/svelte/icons/contact-round";
  import EllipsisVertical from "@lucide/svelte/icons/ellipsis-vertical";
  import Folder from "@lucide/svelte/icons/folder";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import Hash from "@lucide/svelte/icons/hash";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Mail from "@lucide/svelte/icons/mail";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Plus from "@lucide/svelte/icons/plus";
  import Star from "@lucide/svelte/icons/star";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import X from "@lucide/svelte/icons/x";
  import type { ChatChannelRead, ChatMessageSearchResultRead } from "$lib/chat/contracts";
  import { chatParticipantDisplayName } from "$lib/chat/teammates/participant-display";
  import {
    moveChatChannelToSection,
    normalizeChatSidebarSections,
    orderChatSidebarSections,
    readChatSidebarSections,
    saveChatSidebarSections,
    withChatSidebarSectionStarred,
    type ChatSidebarSection,
  } from "$lib/chat/channel-sections";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getChatSidebarOrder } from "$lib/stores/chat/sidebar-order.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
  import { onActiveVaultIdentityChange } from "$lib/vault/active-vault";
  import { overflowTooltip } from "$lib/utils/overflow-tooltip";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { scrollOverflowAction } from "$lib/utils/scroll-overflow";
  import { SUBMENU_CLOSE_DELAY_MS, SUBMENU_OPEN_DELAY_MS } from "$lib/utils/menu-aim";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import ExplorerRowStar from "$lib/components/ui/ExplorerRowStar.svelte";

  /** Which header opened the section menu: the default Channels group, a custom section, or Direct messages. */
  type SectionMenuTarget =
    | { kind: "channels" }
    | { kind: "section"; section: ChatSidebarSection }
    | { kind: "directMessages" };
  type ChatChannelSetupDialogComponent = typeof import("./ChatChannelSetupDialog.svelte").default;
  type ParticipantPickerComponent =
    typeof import("$lib/components/contacts/ParticipantPicker.svelte").default;

  let {
    presentation = "column",
    expanded,
    showCollapsedStrip,
    onExpand,
    onCollapse,
  }: {
    presentation?: "column" | "sheet" | "surface";
    expanded: boolean;
    showCollapsedStrip: boolean;
    onExpand: () => void;
    onCollapse: () => void;
  } = $props();

  const chat = getChat();
  const chatSidebarOrder = getChatSidebarOrder();
  const preferences = getPreferences();
  const projects = getProjects();
  const settings = getSettingsLauncher();
  const { t } = getLocalization();
  let query = $state("");
  let directMessagesCollapsed = $state(false);
  let directMessageButton = $state<HTMLButtonElement | null>(null);
  let directMessagePickerOpen = $state(false);
  let railElement = $state<HTMLElement | null>(null);
  let expandButton = $state<HTMLButtonElement>();
  let collapseButton = $state<HTMLButtonElement>();
  let channelMenuTrigger: HTMLElement | null = null;
  const menuViewportGap = 8;
  let sectionInput = $state<HTMLInputElement>();
  let sectionButton = $state<HTMLButtonElement>();
  let renameSectionInput = $state<HTMLInputElement>();
  let renamingSectionId = $state<string | null>(null);
  let sectionNameDraft = $state("");
  let sectionRenameControlPointerActive = false;
  let sectionMenuTrigger: HTMLButtonElement | null = null;
  let sectionContextMenuElement = $state<HTMLElement | null>(null);
  let sectionContextMenu = $state<{ target: SectionMenuTarget; x: number; y: number } | null>(null);
  let channelContextMenuElement = $state<HTMLElement | null>(null);
  let channelContextMenu = $state<{ channel: ChatChannelRead; x: number; y: number } | null>(null);
  let moveMenuTrigger = $state<HTMLButtonElement | null>(null);
  let moveMenuElement = $state<HTMLElement | null>(null);
  let moveMenuOpen = $state(false);
  let moveMenuPosition = $state<{ x: number; y: number } | null>(null);
  let moveMenuHoverTimer: ReturnType<typeof setTimeout> | null = null;
  let setupChannel = $state<ChatChannelRead | null | undefined>(undefined);
  let setupSectionId = $state<string | null>(null);
  let SetupDialog = $state<ChatChannelSetupDialogComponent | null>(null);
  let setupDialogLoad: Promise<void> | null = null;
  let DirectMessagePicker = $state<ParticipantPickerComponent | null>(null);
  let directMessagePickerLoad: Promise<void> | null = null;
  let sections = $state<ChatSidebarSection[]>([]);
  let loadedProjectId = $state<string | null>(null);
  let newSectionName = $state("");
  let creatingSection = $state(false);
  let channelsCollapsed = $state(false);
  let archiveCandidate = $state<ChatChannelRead | null>(null);
  let deleteSectionCandidate = $state<ChatSidebarSection | null>(null);
  let railError = $state<string | null>(null);
  let messageSearchResults = $state<ChatMessageSearchResultRead[]>([]);
  let messageSearchLoading = $state(false);
  let messageSearchRequest = 0;
  const matchingChannels = $derived(chat.activeChannels.filter(matchesQuery));
  const assignedChannelIds = $derived(new Set(sections.flatMap((section) => section.channelIds)));
  const unsectionedChannels = $derived(
    chatSidebarOrder.sortChannels(loadedProjectId, matchingChannels.filter((channel) => !assignedChannelIds.has(channel.id))),
  );
  const orderedSections = $derived(orderChatSidebarSections(sections, chatSidebarOrder.orderForProject(loadedProjectId)));

  function searchAuthorDisplayName(result: ChatMessageSearchResultRead): string {
    return chatParticipantDisplayName(
      {
        id: result.authorParticipantId,
        kind: result.authorKind,
        displayName: result.authorDisplayName,
      },
      preferences.profileDisplayName,
      t("chat.timeline.you"),
    );
  }

  $effect(() => {
    const projectId = projects.selectedProjectId;
    if (!projectId || projectId === loadedProjectId) return;
    const projectChannels = chat.activeChannels.filter((channel) => channel.projectId === projectId);
    if (projectChannels.length === 0 && chat.activeChannels.length > 0) return;
    loadedProjectId = projectId;
    renamingSectionId = null;
    sectionNameDraft = "";
    sections = normalizeChatSidebarSections(readChatSidebarSections(projectId), projectChannels.map((channel) => channel.id));
  });

  $effect(() => {
    const normalized = query.trim();
    if (normalized.length < 2) {
      messageSearchRequest += 1;
      messageSearchResults = [];
      messageSearchLoading = false;
      return;
    }
    const request = ++messageSearchRequest;
    messageSearchLoading = true;
    const timeout = window.setTimeout(() => {
      void chat.searchOrganizationalMessages(normalized)
        .then((results) => {
          if (request === messageSearchRequest) messageSearchResults = results;
        })
        .catch((cause: unknown) => {
          if (request === messageSearchRequest) {
            railError = cause instanceof Error ? cause.message : String(cause);
          }
        })
        .finally(() => {
          if (request === messageSearchRequest) messageSearchLoading = false;
        });
    }, 180);
    return () => window.clearTimeout(timeout);
  });

  $effect(() => {
    const projectId = loadedProjectId;
    const channelIds = chat.activeChannels
      .filter((channel) => channel.projectId === projectId)
      .map((channel) => channel.id);
    if (!projectId) return;
    const normalized = normalizeChatSidebarSections(sections, channelIds);
    if (JSON.stringify(normalized) !== JSON.stringify(sections)) sections = normalized;
  });

  function matchesQuery(channel: ChatChannelRead): boolean {
    const normalized = query.trim().toLocaleLowerCase();
    return !normalized
      || channel.name.toLocaleLowerCase().includes(normalized)
      || channel.topic.toLocaleLowerCase().includes(normalized);
  }

  function sectionChannels(section: ChatSidebarSection): ChatChannelRead[] {
    const byId = new Map(matchingChannels.map((channel) => [channel.id, channel]));
    return chatSidebarOrder.sortChannels(loadedProjectId, section.channelIds.flatMap((id) => byId.get(id) ?? []));
  }

  function persist(next: ChatSidebarSection[]): void {
    sections = next;
    if (loadedProjectId) saveChatSidebarSections(loadedProjectId, next);
  }

  function toggleSection(sectionId: string): void {
    persist(sections.map((section) => section.id === sectionId
      ? { ...section, collapsed: !section.collapsed }
      : section));
  }

  function sectionToggleLabel(name: string, collapsed: boolean): string {
    return collapsed
      ? t("chat.channels.expandSection", name)
      : t("chat.channels.collapseSection", name);
  }

  /** Reveal the section draft and focus its name, or return to the section menu. */
  function toggleSectionDraft(): void {
    creatingSection = !creatingSection;
    newSectionName = "";
    if (creatingSection) void tick().then(() => sectionInput?.focus());
    else sectionButton?.focus();
  }

  function createSection(): void {
    const name = newSectionName.trim();
    if (!name || [...name].length > 80) return;
    persist([...sections, { id: `section:${crypto.randomUUID()}`, name, collapsed: false, channelIds: [] }]);
    newSectionName = "";
    creatingSection = false;
    sectionButton?.focus();
  }

  function deleteSection(section: ChatSidebarSection): void {
    persist(sections.filter((entry) => entry.id !== section.id));
    deleteSectionCandidate = null;
  }

  function confirmDeleteSection(): void {
    const section = deleteSectionCandidate;
    if (section) deleteSection(section);
  }

  function startSectionRename(section: ChatSidebarSection): void {
    renamingSectionId = section.id;
    sectionNameDraft = section.name;
    void tick().then(() => {
      if (renamingSectionId !== section.id) return;
      renameSectionInput?.focus();
      renameSectionInput?.select();
    });
  }

  function finishSectionRename(save: boolean): void {
    const sectionId = renamingSectionId;
    if (!sectionId) return;
    const name = sectionNameDraft.trim();
    renamingSectionId = null;
    sectionNameDraft = "";
    sectionRenameControlPointerActive = false;
    if (!save || !name || [...name].length > 80) return;
    const section = sections.find((entry) => entry.id === sectionId);
    if (!section || name === section.name) return;
    persist(sections.map((entry) => entry.id === sectionId ? { ...entry, name } : entry));
  }

  function handleSectionRenameKeydown(event: KeyboardEvent): void {
    if (event.key !== "Enter" && event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    finishSectionRenameAndFocus(event, event.key === "Enter");
  }

  function handleSectionRenameControlKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    finishSectionRenameAndFocus(event, false);
  }

  function finishSectionRenameAndFocus(event: Event, save: boolean): void {
    const target = event.currentTarget;
    const heading = target instanceof HTMLElement ? target.closest<HTMLElement>(".section-heading") : null;
    finishSectionRename(save);
    void tick().then(() => heading?.querySelector<HTMLButtonElement>(".section-toggle")?.focus());
  }

  function handleSectionRenameFocusOut(event: FocusEvent): void {
    const target = event.currentTarget;
    if (sectionRenameControlPointerActive || (target instanceof HTMLElement && event.relatedTarget instanceof Node && target.contains(event.relatedTarget))) return;
    finishSectionRename(true);
  }

  function releaseSectionRenameControlPointer(): void {
    window.setTimeout(() => { sectionRenameControlPointerActive = false; }, 0);
  }

  function closeSectionContextMenu(): void {
    sectionContextMenu = null;
  }

  async function toggleSectionContextMenu(event: MouseEvent, menuTarget: SectionMenuTarget): Promise<void> {
    const trigger = event.currentTarget;
    if (!(trigger instanceof HTMLButtonElement)) return;
    if (sectionContextMenu && sectionMenuTrigger === trigger) {
      closeSectionContextMenu();
      return;
    }
    closeChannelContextMenu();
    sectionMenuTrigger = trigger;
    const bounds = trigger.getBoundingClientRect();
    const initialX = bounds.left;
    const initialY = bounds.bottom;
    sectionContextMenu = { target: menuTarget, x: initialX, y: initialY };
    await tick();
    if (!sectionContextMenu || !sectionContextMenuElement || sectionMenuTrigger !== trigger) return;
    const menuBounds = sectionContextMenuElement.getBoundingClientRect();
    sectionContextMenu = {
      target: menuTarget,
      x: Math.min(Math.max(menuViewportGap, initialX), Math.max(menuViewportGap, window.innerWidth - menuBounds.width - menuViewportGap)),
      y: Math.min(Math.max(menuViewportGap, initialY), Math.max(menuViewportGap, window.innerHeight - menuBounds.height - menuViewportGap)),
    };
    (sectionContextMenuElement.querySelector<HTMLButtonElement>("button:not(:disabled)") ?? sectionContextMenuElement).focus();
  }

  function contextMenuSection(): ChatSidebarSection | null {
    return sectionContextMenu?.target.kind === "section" ? sectionContextMenu.target.section : null;
  }

  function createSectionFromContextMenu(): void {
    closeSectionContextMenu();
    toggleSectionDraft();
  }

  function renameSectionFromContextMenu(): void {
    const section = contextMenuSection();
    closeSectionContextMenu();
    if (section) startSectionRename(section);
  }

  function toggleSectionStarFromContextMenu(): void {
    const section = contextMenuSection();
    closeSectionContextMenu();
    if (section) persist(withChatSidebarSectionStarred(sections, section.id, section.starred !== true));
  }

  function deleteSectionFromContextMenu(): void {
    const section = contextMenuSection();
    closeSectionContextMenu();
    if (section) deleteSectionCandidate = section;
  }

  function openContactsFromContextMenu(contactsTab: "contacts" | "invitations"): void {
    closeSectionContextMenu();
    settings.open("contacts", { contactsTab });
  }

  function moveChannel(channelId: string, sectionId: string | null): void {
    persist(moveChatChannelToSection(sections, channelId, sectionId));
  }

  function handleDrop(event: DragEvent, sectionId: string | null): void {
    event.preventDefault();
    const channelId = event.dataTransfer?.getData("application/x-ganbaru-chat-channel") ?? "";
    if (channelId) moveChannel(channelId, sectionId);
  }

  function beginDrag(event: DragEvent, channelId: string): void {
    event.dataTransfer?.setData("application/x-ganbaru-chat-channel", channelId);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  }

  async function openChannelContextMenu(event: MouseEvent, channel: ChatChannelRead): Promise<void> {
    event.preventDefault();
    const trigger = event.currentTarget instanceof HTMLElement ? event.currentTarget : null;
    if (event.type === "click" && trigger?.classList.contains("explorer-row-action") && channelContextMenu?.channel.id === channel.id) {
      closeChannelContextMenu();
      return;
    }
    closeSectionContextMenu();
    hideMoveMenu();
    channelMenuTrigger = trigger;
    const triggerBounds = trigger?.getBoundingClientRect();
    const openedFromKeyboard = event.type === "click" || (event.clientX === 0 && event.clientY === 0);
    const initialX = openedFromKeyboard ? triggerBounds?.left ?? menuViewportGap : event.clientX;
    const initialY = openedFromKeyboard ? triggerBounds?.bottom ?? menuViewportGap : event.clientY;
    channelContextMenu = { channel, x: initialX, y: initialY };
    await tick();
    if (!channelContextMenuElement || channelContextMenu?.channel.id !== channel.id) return;
    const menuBounds = channelContextMenuElement.getBoundingClientRect();
    channelContextMenu = {
      channel,
      x: Math.min(Math.max(menuViewportGap, initialX), Math.max(menuViewportGap, window.innerWidth - menuBounds.width - menuViewportGap)),
      y: Math.min(Math.max(menuViewportGap, initialY), Math.max(menuViewportGap, window.innerHeight - menuBounds.height - menuViewportGap)),
    };
    channelContextMenuElement.querySelector<HTMLButtonElement>("button")?.focus();
  }

  function clearMoveMenuHoverTimer(): void {
    if (moveMenuHoverTimer !== null) clearTimeout(moveMenuHoverTimer);
    moveMenuHoverTimer = null;
  }

  /** Open or close the move submenu after the shared hover delay, so a passing pointer does not toggle it. */
  function scheduleMoveMenu(open: boolean): void {
    clearMoveMenuHoverTimer();
    if (moveMenuOpen === open) return;
    moveMenuHoverTimer = setTimeout(() => {
      moveMenuHoverTimer = null;
      if (open) void showMoveMenu();
      else hideMoveMenu();
    }, open ? SUBMENU_OPEN_DELAY_MS : SUBMENU_CLOSE_DELAY_MS);
  }

  function hideMoveMenu(): void {
    clearMoveMenuHoverTimer();
    moveMenuOpen = false;
    moveMenuPosition = null;
  }

  function closeChannelContextMenu(): void {
    channelContextMenu = null;
    hideMoveMenu();
  }

  async function showMoveMenu(focusFirst = false): Promise<void> {
    clearMoveMenuHoverTimer();
    if (!channelContextMenu) return;
    moveMenuOpen = true;
    await tick();
    const triggerBounds = moveMenuTrigger?.getBoundingClientRect();
    const menuBounds = channelContextMenuElement?.getBoundingClientRect();
    const submenuBounds = moveMenuElement?.getBoundingClientRect();
    if (!triggerBounds || !menuBounds || !submenuBounds || !moveMenuOpen) return;

    const spaceRight = window.innerWidth - menuBounds.right - menuViewportGap;
    const spaceLeft = menuBounds.left - menuViewportGap;
    const preferredX = spaceRight >= submenuBounds.width || spaceRight >= spaceLeft
      ? menuBounds.right - 1
      : menuBounds.left - submenuBounds.width + 1;
    moveMenuPosition = {
      x: Math.min(Math.max(menuViewportGap, preferredX), Math.max(menuViewportGap, window.innerWidth - submenuBounds.width - menuViewportGap)),
      y: Math.min(Math.max(menuViewportGap, triggerBounds.top), Math.max(menuViewportGap, window.innerHeight - submenuBounds.height - menuViewportGap)),
    };
    if (focusFirst) {
      await tick();
      moveMenuElement?.querySelector<HTMLButtonElement>("button")?.focus();
    }
  }

  function editChannelFromContextMenu(): void {
    const menu = channelContextMenu;
    if (!menu) return;
    closeChannelContextMenu();
    openEdit(menu.channel);
  }

  function moveChannelFromContextMenu(sectionId: string | null): void {
    const menu = channelContextMenu;
    if (!menu) return;
    closeChannelContextMenu();
    moveChannel(menu.channel.id, sectionId);
  }

  function toggleChannelStarFromContextMenu(): void {
    const menu = channelContextMenu;
    if (!menu) return;
    closeChannelContextMenu();
    chatSidebarOrder.setChannelStarred(menu.channel.id, !chatSidebarOrder.isChannelStarred(menu.channel.id));
  }

  function archiveChannelFromContextMenu(): void {
    const menu = channelContextMenu;
    if (!menu) return;
    closeChannelContextMenu();
    archiveCandidate = menu.channel;
  }

  async function confirmArchive(): Promise<void> {
    const channel = archiveCandidate;
    archiveCandidate = null;
    if (!channel) return;
    railError = null;
    try {
      await chat.archiveChannel(channel);
    } catch (cause: unknown) {
      railError = cause instanceof Error ? cause.message : String(cause);
    }
  }

  function savedChannel(channel: ChatChannelRead, sectionId: string | null): void {
    moveChannel(channel.id, sectionId);
    setupChannel = undefined;
    setupSectionId = null;
    if (presentation === "surface") onCollapse();
  }

  /** Collapse the docked rail and keep focus on the control that reopens it. */
  function collapseToStrip(): void {
    onCollapse();
    if (showCollapsedStrip) void tick().then(() => expandButton?.focus());
  }

  /** Expand the docked rail and keep focus on the control that collapses it. */
  function expandFromStrip(): void {
    onExpand();
    void tick().then(() => collapseButton?.focus());
  }

  /** Focus the persistent search field after the rail has opened. */
  function openSearch(): void {
    void tick().then(() => railElement?.querySelector<HTMLInputElement>('input[type="search"]')?.focus());
  }

  /** Load the channel setup dialog only when a channel is created or edited. */
  function loadSetupDialog(): Promise<void> {
    if (SetupDialog) return Promise.resolve();
    setupDialogLoad ??= import("./ChatChannelSetupDialog.svelte")
      .then((module) => { SetupDialog = module.default; })
      .catch((cause: unknown) => {
        railError = cause instanceof Error ? cause.message : String(cause);
        setupChannel = undefined;
        setupSectionId = null;
      })
      .finally(() => { setupDialogLoad = null; });
    return setupDialogLoad;
  }

  /** Load the direct message picker only when the rail approaches or opens it. */
  function loadDirectMessagePicker(): Promise<void> {
    if (DirectMessagePicker) return Promise.resolve();
    directMessagePickerLoad ??= import("$lib/components/contacts/ParticipantPicker.svelte")
      .then((module) => { DirectMessagePicker = module.default; })
      .catch((cause: unknown) => {
        railError = cause instanceof Error ? cause.message : String(cause);
        directMessagePickerOpen = false;
      })
      .finally(() => { directMessagePickerLoad = null; });
    return directMessagePickerLoad;
  }

  function openDirectMessagePicker(): void {
    directMessagePickerOpen = true;
    void loadDirectMessagePicker();
  }

  function openCreate(sectionId: string | null = null): void {
    onExpand();
    setupChannel = null;
    setupSectionId = sectionId;
    void loadSetupDialog();
  }

  function openEdit(channel: ChatChannelRead): void {
    setupChannel = channel;
    setupSectionId = sections.find((section) => section.channelIds.includes(channel.id))?.id ?? null;
    void loadSetupDialog();
  }

  function editChannelFromEvent(event: Event): void {
    const channelId = event instanceof CustomEvent
      && typeof event.detail === "object"
      && event.detail !== null
      && "channelId" in event.detail
      && typeof event.detail.channelId === "string"
      ? event.detail.channelId
      : null;
    const channel = chat.activeChannels.find((entry) => entry.id === channelId);
    if (channel) openEdit(channel);
  }

  async function selectChannel(channelId: string): Promise<void> {
    railError = null;
    try {
      await chat.selectChannel(channelId);
      if (presentation === "surface") onCollapse();
    } catch (cause: unknown) {
      railError = cause instanceof Error ? cause.message : String(cause);
    }
  }

  async function openMessageResult(result: ChatMessageSearchResultRead): Promise<void> {
    railError = null;
    try {
      await chat.openMessageSearchResult(result);
      query = "";
      if (presentation === "surface") onCollapse();
    } catch (cause: unknown) {
      railError = cause instanceof Error ? cause.message : String(cause);
    }
  }

  function searchExcerpt(result: ChatMessageSearchResultRead): string {
    return result.excerpt.replace(/<\/?mark>/g, "");
  }

  function projectName(projectId: string): string {
    return projects.projects.find((project) => project.id === projectId)?.name ?? t("chat.title");
  }

  onMount(() => {
    const createChannel = () => openCreate();
    const focusSearch = () => {
      onExpand();
      openSearch();
    };
    const closeOpenMenus = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (sectionContextMenu && !sectionContextMenuElement?.contains(target)
        && !sectionMenuTrigger?.contains(target)) closeSectionContextMenu();
      if (channelContextMenu && !channelContextMenuElement?.contains(target)
        && !(channelMenuTrigger?.classList.contains("explorer-row-action") && channelMenuTrigger.contains(target))) {
        closeChannelContextMenu();
      }
    };
    window.addEventListener("ganbaru-ai:chat-new-channel", createChannel);
    window.addEventListener("ganbaru-ai:chat-edit-channel", editChannelFromEvent);
    window.addEventListener("ganbaru-ai:chat-focus-search", focusSearch);
    document.addEventListener("pointerdown", closeOpenMenus);
    const unsubscribeVault = onActiveVaultIdentityChange(() => {
      loadedProjectId = null;
      sections = [];
      renamingSectionId = null;
      sectionNameDraft = "";
    });
    return () => {
      window.removeEventListener("ganbaru-ai:chat-new-channel", createChannel);
      window.removeEventListener("ganbaru-ai:chat-edit-channel", editChannelFromEvent);
      window.removeEventListener("ganbaru-ai:chat-focus-search", focusSearch);
      document.removeEventListener("pointerdown", closeOpenMenus);
      unsubscribeVault();
      clearMoveMenuHoverTimer();
    };
  });
</script>

{#if expanded || showCollapsedStrip}
  <aside bind:this={railElement} class="explorer-sidebar flex h-full min-h-0 flex-col" class:explorer-dock={showCollapsedStrip} class:explorer-dock-collapsed={showCollapsedStrip && !expanded} class:explorer-touch={presentation === "surface"} aria-label={t("chat.channels.explorerLabel")}>
    {#if showCollapsedStrip}
      <div class="explorer-dock-rail" aria-hidden={expanded} inert={expanded}>
        <div class="explorer-toolbar justify-center"><button bind:this={expandButton} type="button" class="explorer-icon" aria-expanded="false" data-app-tooltip={t("chat.openRail")} aria-label={t("chat.openRail")} onclick={expandFromStrip}><ChevronsRight size={16} /></button></div>
      </div>
    {/if}
    <div class="flex h-full min-h-0 flex-col" class:explorer-dock-content={showCollapsedStrip} inert={!expanded}>
      <div class="explorer-toolbar">
        <SearchField bind:value={query} label={t("chat.channels.search")} clearLabel={t("chat.channels.clearSearch")} inline focusOnMount={false} />
        <button bind:this={collapseButton} type="button" class="explorer-icon" data-chat-rail-close aria-label={t("chat.collapseRail")} aria-expanded="true" data-app-tooltip={t("chat.collapseRail")} onclick={collapseToStrip}><ChevronsLeft size={16} /></button>
      </div>

      {#if creatingSection}
        <form class="mx-2 mb-2 flex gap-1" onsubmit={(event) => { event.preventDefault(); createSection(); }}>
          <input bind:this={sectionInput} class="field min-w-0 flex-1 text-xs" bind:value={newSectionName} maxlength="80" placeholder={t("chat.channels.sectionName")} aria-label={t("chat.channels.sectionName")} onkeydown={(event) => { if (event.key === "Escape") { event.stopPropagation(); toggleSectionDraft(); } }} />
          <button type="submit" class="explorer-icon" disabled={!newSectionName.trim()} aria-label={t("chat.channels.add")}><Plus size={16} /></button>
        </form>
      {/if}

      {#if railError}<Notice class="mx-2 mb-1 px-2 py-1 text-xs" message={railError} />{/if}

      <div use:scrollOverflowAction class="explorer-list min-h-0 flex-1 pb-2">
        {#if query.trim().length >= 2}
          <section class="message-results" aria-label={t("chat.organization.messageSearchResults")}>
            <div class="section-heading"><span>{t("chat.organization.messages")}</span>{#if messageSearchLoading}<LoaderCircle size={12} class="animate-spin" aria-label={t("common.loading")} />{/if}</div>
            {#each messageSearchResults as result (result.messageItemId)}
              <button type="button" class="message-result" onclick={() => void openMessageResult(result)}>
                <span><strong>{searchAuthorDisplayName(result)}</strong><small>{projectName(result.projectId)} · #{result.channelName}{#if result.replyThreadId} · {t("chat.organization.thread")}{/if}</small></span>
                <span>{searchExcerpt(result)}</span>
              </button>
            {/each}
            {#if !messageSearchLoading && messageSearchResults.length === 0}<p class="px-2 py-1 text-xs text-muted-foreground">{t("chat.organization.noMessageSearchResults")}</p>{/if}
          </section>
        {/if}
        <section class="channel-section" role="group" ondragover={(event) => event.preventDefault()} ondrop={(event) => handleDrop(event, null)}>
          <div class="section-heading">
            <button type="button" class="section-toggle" class:collapsed={channelsCollapsed} aria-label={sectionToggleLabel(t("chat.channels.defaultSection"), channelsCollapsed)} aria-expanded={!channelsCollapsed} onclick={() => { channelsCollapsed = !channelsCollapsed; }}>
              <span use:overflowTooltip={t("chat.channels.defaultSection")}>{t("chat.channels.defaultSection")}</span>
              {#if channelsCollapsed}<ChevronRight class="section-chevron" size={13} />{:else}<ChevronDown class="section-chevron" size={13} />{/if}
            </button>
            <button type="button" class="section-create-action" aria-label={t("chat.channels.createTitle")} data-app-tooltip={t("chat.channels.createTitle")} onclick={() => openCreate()}><Plus size={13} /></button>
            <button bind:this={sectionButton} type="button" class="explorer-icon explorer-row-action" aria-label={t("chat.moreActions")} data-app-tooltip-disabled="true" aria-haspopup="menu" aria-expanded={sectionContextMenu?.target.kind === "channels"} onclick={(event) => void toggleSectionContextMenu(event, { kind: "channels" })}><EllipsisVertical size={14} /></button>
          </div>
          {#if !channelsCollapsed}
            {#if chat.channelsLoading && projects.selectedProjectId}
              <div class="channel-row explorer-row-label channel-loading" role="status" aria-label={t("common.loading")}>
                <Hash size={COMPACT_IDENTITY_ICON_SIZE} strokeWidth={COMPACT_IDENTITY_ICON_STROKE_WIDTH} class="explorer-row-icon" />
                <span class="min-w-0 flex-1 truncate">general</span>
                <LoaderCircle size={12} class="animate-spin" />
              </div>
            {:else}
              {#each unsectionedChannels as channel (channel.id)}
                {@render ChannelRow({ channel })}
              {/each}
              {#if unsectionedChannels.length === 0 && query}<p class="px-2 py-1 text-xs text-muted-foreground">{t("chat.channels.noResults")}</p>{/if}
            {/if}
          {/if}
        </section>

        {#each orderedSections as section (section.id)}
          <section class="channel-section" role="group" ondragover={(event) => event.preventDefault()} ondrop={(event) => handleDrop(event, section.id)}>
            <div
              class="section-heading"
              class:section-renaming={renamingSectionId === section.id}
              onfocusout={(event) => { if (renamingSectionId === section.id) handleSectionRenameFocusOut(event); }}
            >
              {#if renamingSectionId === section.id}
                <input
                  bind:this={renameSectionInput}
                  class="section-rename-input"
                  bind:value={sectionNameDraft}
                  maxlength="80"
                  aria-label={t("chat.channels.sectionName")}
                  data-app-shortcuts="ignore"
                  onkeydown={handleSectionRenameKeydown}
                />
                <button type="button" class="explorer-icon section-rename-control" aria-label={t("chat.save")} disabled={!sectionNameDraft.trim() || [...sectionNameDraft.trim()].length > 80} data-app-tooltip-disabled="true" onpointerdown={() => { sectionRenameControlPointerActive = true; }} onpointerup={releaseSectionRenameControlPointer} onpointercancel={() => { sectionRenameControlPointerActive = false; }} onkeydown={handleSectionRenameControlKeydown} onclick={(event) => finishSectionRenameAndFocus(event, true)}><Check size={14} /></button>
                <button type="button" class="explorer-icon section-rename-control" aria-label={t("chat.cancel")} data-app-tooltip-disabled="true" onpointerdown={() => { sectionRenameControlPointerActive = true; }} onpointerup={releaseSectionRenameControlPointer} onpointercancel={() => { sectionRenameControlPointerActive = false; }} onkeydown={handleSectionRenameControlKeydown} onclick={(event) => finishSectionRenameAndFocus(event, false)}><X size={14} /></button>
              {:else}
                <button type="button" class="section-toggle" class:collapsed={section.collapsed} aria-label={sectionToggleLabel(section.name, section.collapsed)} aria-expanded={!section.collapsed} onclick={() => toggleSection(section.id)}>
                  {#if section.starred}<Star class="section-star size-2.5 shrink-0 fill-current text-primary" aria-hidden="true" />{/if}
                  <span use:overflowTooltip={section.name}>{section.name}</span>
                  {#if section.collapsed}<ChevronRight class="section-chevron" size={13} />{:else}<ChevronDown class="section-chevron" size={13} />{/if}
                </button>
                <button type="button" class="section-create-action" aria-label={t("chat.channels.createTitle")} data-app-tooltip={t("chat.channels.createTitle")} onclick={() => openCreate(section.id)}><Plus size={13} /></button>
                <button type="button" class="explorer-icon explorer-row-action" aria-label={`${t("chat.moreActions")}: ${section.name}`} data-app-tooltip-disabled="true" aria-haspopup="menu" aria-expanded={sectionContextMenu?.target.kind === "section" && sectionContextMenu.target.section.id === section.id} onclick={(event) => void toggleSectionContextMenu(event, { kind: "section", section })}><EllipsisVertical size={14} /></button>
              {/if}
            </div>
            {#if !section.collapsed}
              {#each sectionChannels(section) as channel (channel.id)}{@render ChannelRow({ channel })}{/each}
            {/if}
          </section>
        {/each}

        {#if !query.trim()}
          <section class="channel-section" role="group" data-chat-direct-messages>
            <div class="section-heading">
              <button type="button" class="section-toggle" class:collapsed={directMessagesCollapsed} aria-label={sectionToggleLabel(t("chat.channels.directMessages"), directMessagesCollapsed)} aria-expanded={!directMessagesCollapsed} onclick={() => { directMessagesCollapsed = !directMessagesCollapsed; }}>
                <span use:overflowTooltip={t("chat.channels.directMessages")}>{t("chat.channels.directMessages")}</span>
                {#if directMessagesCollapsed}<ChevronRight class="section-chevron" size={13} />{:else}<ChevronDown class="section-chevron" size={13} />{/if}
              </button>
              <button bind:this={directMessageButton} type="button" class="section-create-action" aria-label={t("chat.channels.newDirectMessage")} data-app-tooltip={t("chat.channels.newDirectMessage")} aria-haspopup="dialog" aria-expanded={directMessagePickerOpen} onpointerenter={() => { void loadDirectMessagePicker(); }} onfocus={() => { void loadDirectMessagePicker(); }} onclick={openDirectMessagePicker}><Plus size={13} /></button>
              <button type="button" class="explorer-icon explorer-row-action" aria-label={`${t("chat.moreActions")}: ${t("chat.channels.directMessages")}`} data-app-tooltip-disabled="true" aria-haspopup="menu" aria-expanded={sectionContextMenu?.target.kind === "directMessages"} onclick={(event) => void toggleSectionContextMenu(event, { kind: "directMessages" })}><EllipsisVertical size={14} /></button>
            </div>
          </section>
        {/if}

      </div>
    </div>
  </aside>
{/if}

{#if sectionContextMenu}
  <div
    bind:this={sectionContextMenuElement}
    class="section-context-menu surface-floating surface-floating-body fixed z-90 w-floating-sm"
    class:explorer-touch={presentation === "surface"}
    role="menu"
    tabindex="-1"
    style={`left:${sectionContextMenu.x}px;top:${sectionContextMenu.y}px`}
    data-app-floating-surface
    onkeydown={(event) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        closeSectionContextMenu();
        sectionMenuTrigger?.focus();
      }
    }}
  >
    {#if sectionContextMenu.target.kind === "channels"}
      <button type="button" role="menuitem" disabled={!projects.selectedProjectId} class="menu-item" onclick={createSectionFromContextMenu}><FolderPlus class="size-4" /><span>{t("chat.channels.newSection")}</span></button>
    {:else if sectionContextMenu.target.kind === "directMessages"}
      <button type="button" role="menuitem" class="menu-item" onclick={() => openContactsFromContextMenu("contacts")}><ContactRound class="size-4" /><span>{t("contacts.tabs.contacts")}</span></button>
      <button type="button" role="menuitem" class="menu-item" data-chat-invitations onclick={() => openContactsFromContextMenu("invitations")}><Mail class="size-4" /><span>{t("contacts.tabs.invitations")}</span></button>
    {:else}
      {@const sectionStarred = sectionContextMenu.target.section.starred === true}
      <button type="button" role="menuitem" class="menu-item" onclick={renameSectionFromContextMenu}><Pencil class="size-4" /><span>{t("chat.rename")}</span></button>
      <button type="button" role="menuitem" class="menu-item" onclick={toggleSectionStarFromContextMenu}><Star class={sectionStarred ? "size-4 fill-current text-primary" : "size-4"} /><span>{sectionStarred ? t("common.removeFromFavorites") : t("common.addToFavorites")}</span></button>
      <button type="button" role="menuitem" class="menu-item menu-item-destructive" onclick={deleteSectionFromContextMenu}><Trash2 class="size-4" /><span>{t("chat.channels.deleteSectionConfirm")}</span></button>
    {/if}
  </div>
{/if}

{#if channelContextMenu}
  {@const channelStarred = chatSidebarOrder.isChannelStarred(channelContextMenu.channel.id)}
  <div
    bind:this={channelContextMenuElement}
    class="channel-context-menu surface-floating surface-floating-body fixed z-90 w-floating-sm"
    class:explorer-touch={presentation === "surface"}
    role="menu"
    tabindex="-1"
    style={`left:${channelContextMenu.x}px;top:${channelContextMenu.y}px`}
    data-app-floating-surface
    onkeydown={(event) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        closeChannelContextMenu();
        channelMenuTrigger?.focus();
      }
    }}
    onscroll={hideMoveMenu}
  >
    <button type="button" role="menuitem" class="menu-item" onclick={editChannelFromContextMenu}><Pencil class="size-4" /><span>{t("chat.channels.edit")}</span></button>
    <div
      role="group"
      aria-label={t("chat.channels.moveTo")}
      onpointerenter={(event) => { if (event.pointerType !== "touch") scheduleMoveMenu(true); }}
      onpointerleave={(event) => { if (event.pointerType !== "touch") scheduleMoveMenu(false); }}
      onfocusout={(event) => { if (!(event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget))) hideMoveMenu(); }}
    >
      <button bind:this={moveMenuTrigger} type="button" role="menuitem" aria-haspopup="menu" aria-expanded={moveMenuOpen} class="menu-item" onfocus={() => void showMoveMenu()} onclick={() => void showMoveMenu(true)} onkeydown={(event) => { if (event.key === "ArrowRight") { event.preventDefault(); void showMoveMenu(true); } }}><FolderInput class="size-4" /><span class="min-w-0 flex-1 truncate">{t("chat.channels.moveTo")}</span><ChevronRight class="size-4" /></button>
      {#if moveMenuOpen}
        <div
          bind:this={moveMenuElement}
          class="channel-move-menu surface-floating fixed z-100 flex w-floating-sm flex-col overflow-hidden"
          class:invisible={moveMenuPosition === null}
          role="menu"
          tabindex="-1"
          aria-label={t("chat.channels.moveTo")}
          style={`left:${moveMenuPosition?.x ?? -9999}px;top:${moveMenuPosition?.y ?? -9999}px`}
          onkeydown={(event) => { if (event.key === "ArrowLeft") { event.preventDefault(); event.stopPropagation(); hideMoveMenu(); moveMenuTrigger?.focus(); } }}
        >
          <div class="surface-floating-body min-h-0 flex-1 overflow-y-auto" use:scrollEdgeFadeAction>
            <button type="button" role="menuitem" class="menu-item" onclick={() => moveChannelFromContextMenu(null)}><Hash class="size-4" /><span class="min-w-0 truncate">{t("chat.channels.defaultSection")}</span></button>
            {#each orderedSections as section (section.id)}<button type="button" role="menuitem" class="menu-item" onclick={() => moveChannelFromContextMenu(section.id)}><Folder class="size-4" /><span class="min-w-0 truncate">{section.name}</span></button>{/each}
          </div>
        </div>
      {/if}
    </div>
    <button type="button" role="menuitem" class="menu-item" onclick={toggleChannelStarFromContextMenu}><Star class={channelStarred ? "size-4 fill-current text-primary" : "size-4"} /><span>{channelStarred ? t("common.removeFromFavorites") : t("common.addToFavorites")}</span></button>
    {#if !channelContextMenu.channel.isDefault}<button type="button" role="menuitem" class="menu-item menu-item-destructive" onclick={archiveChannelFromContextMenu}><Archive class="size-4" /><span>{t("chat.archive")}</span></button>{/if}
  </div>
{/if}

{#snippet ChannelRow({ channel }: { channel: ChatChannelRead })}
  <div class="channel-row-group explorer-row" class:explorer-selected={chat.selectedChannelId === channel.id && !chat.channelArchiveOpen} role="group" aria-label={channel.name} draggable="true" ondragstart={(event) => beginDrag(event, channel.id)}>
    <button type="button" class="channel-row explorer-row-label" class:selected={chat.selectedChannelId === channel.id && !chat.channelArchiveOpen} aria-current={chat.selectedChannelId === channel.id && !chat.channelArchiveOpen ? "page" : undefined} onclick={() => void selectChannel(channel.id)} oncontextmenu={(event) => void openChannelContextMenu(event, channel)}>
      <Hash size={COMPACT_IDENTITY_ICON_SIZE} strokeWidth={COMPACT_IDENTITY_ICON_STROKE_WIDTH} class="explorer-row-icon" />
      <span class="min-w-0 flex-1 truncate" use:overflowTooltip={channel.name}>{channel.name}</span>
    </button>
    <span class="explorer-row-trailing">
      {#if chatSidebarOrder.isChannelStarred(channel.id)}<ExplorerRowStar />{/if}
      <button type="button" class="explorer-icon explorer-row-action" aria-label={`${t("chat.channels.actions")}: ${channel.name}`} data-app-tooltip-disabled="true" aria-haspopup="menu" aria-expanded={channelContextMenu?.channel.id === channel.id} onclick={(event) => void openChannelContextMenu(event, channel)}><EllipsisVertical size={14} /></button>
    </span>
  </div>
{/snippet}

{#if setupChannel !== undefined && SetupDialog}
  {@const LoadedSetupDialog = SetupDialog}
  <LoadedSetupDialog channel={setupChannel} sections={orderedSections} initialSectionId={setupSectionId} onSaved={savedChannel} onCancel={() => { setupChannel = undefined; setupSectionId = null; }} />
{/if}

{#if directMessagePickerOpen && DirectMessagePicker}
  {@const LoadedDirectMessagePicker = DirectMessagePicker}
  <LoadedDirectMessagePicker
    anchor={directMessageButton}
    title={t("chat.channels.newDirectMessage")}
    includeContacts
    includeTeammates={false}
    horizontalAlign="start"
    confirmLabel={t("chat.channels.startDirectMessage")}
    onClose={() => { directMessagePickerOpen = false; }}
  />
{/if}

{#if archiveCandidate}<ConfirmDialog title={t("chat.channels.archiveConfirmTitle", archiveCandidate.name)} message={t("chat.channels.archiveConfirmMessage")} confirmLabel={t("chat.archive")} cancelLabel={t("chat.cancel")} onConfirm={() => void confirmArchive()} onCancel={() => { archiveCandidate = null; }} />{/if}
{#if deleteSectionCandidate}<ConfirmDialog title={t("chat.channels.deleteSectionTitle", deleteSectionCandidate.name)} message={t("chat.channels.deleteSectionMessage")} confirmLabel={t("chat.channels.deleteSectionConfirm")} cancelLabel={t("chat.cancel")} onConfirm={confirmDeleteSection} onCancel={() => { deleteSectionCandidate = null; }} />{/if}

<style>
  .channel-section + .channel-section { margin-top: 0.5rem; }
  .message-results { margin-block:0.3rem 0.45rem; border-bottom:1px solid var(--border); padding-bottom:0.45rem; }
  .message-result { display:grid; width:100%; gap:0.18rem; border-radius:0.4rem; padding:0.42rem 0.5rem; text-align:left; }
  .message-result:hover,.message-result:focus-visible { background:var(--explorer-icon-hover, var(--accent)); }
  .message-result > span:first-child { display:flex; min-width:0; align-items:baseline; gap:0.35rem; }
  .message-result strong { flex:0 0 auto; font-size: calc(0.72rem * var(--type-scale)); }
  .message-result small { min-width:0; overflow:hidden; color:var(--muted-foreground); font-size: calc(0.6rem * var(--type-scale)); text-overflow:ellipsis; white-space:nowrap; }
  .message-result > span:last-child { display:-webkit-box; overflow:hidden; color:var(--muted-foreground); font-size: calc(0.68rem * var(--type-scale)); line-height: calc(1rem * var(--type-scale)); -webkit-box-orient:vertical; -webkit-line-clamp:2; line-clamp:2; }
  .section-heading { --section-heading-gap: 0.2rem; --section-heading-inset: 0.5rem; display: flex; min-height: var(--explorer-row-height); align-items: center; gap: var(--section-heading-gap); padding-inline: var(--section-heading-inset); color: var(--muted-foreground); }
  .channel-section > .section-heading { padding-inline-end: 0; }
  .section-heading.section-renaming { border-radius: 0.375rem; background: var(--explorer-row-hover, color-mix(in oklab, var(--accent) 50%, transparent)); }
  .section-rename-input { min-width: 0; width: 0; flex: 1; min-height: var(--explorer-action-size); border: 0; background: transparent; color: var(--foreground); caret-color: var(--primary); font: inherit; outline: none; }
  .section-rename-control { color: var(--foreground); }
  .section-heading > .section-rename-control:last-child { margin-inline-end: var(--explorer-row-action-end-inset); }
  .section-heading > button { display: flex; min-width: var(--explorer-action-size); min-height: var(--explorer-action-size); align-items: center; justify-content: center; gap: 0.2rem; border-radius: 0.3rem; }
  .section-heading > .section-create-action { margin-inline-end: calc(-1 * var(--section-heading-gap)); }
  .section-heading > .section-create-action:is(:hover, :focus-visible) { background: transparent; color: var(--foreground); }
  .channel-section > .section-heading > .explorer-row-action { margin-inline-end: 0; opacity: 1; }
  .section-heading > :is(.section-toggle, .section-rename-control):hover { background: var(--explorer-icon-hover, var(--accent)); color: var(--foreground); }
  .section-heading > .section-toggle { min-width: 0; flex: 1; justify-content: flex-start; margin-inline-start: calc(-1 * var(--section-heading-inset)); padding-inline: var(--section-heading-inset); border-radius: 0.375rem; }
  .section-toggle > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .section-toggle :global(.section-chevron) { flex: 0 0 auto; transition: opacity 120ms ease; }
  .section-context-menu, .channel-context-menu { max-height: min(22rem, calc(100vh - 4rem)); overflow-y: auto; }
  .channel-move-menu { max-height: min(22rem, calc(100vh - 4rem)); }
  .section-context-menu.explorer-touch button, .channel-context-menu.explorer-touch button { min-height: var(--touch-target-min); }
  .channel-row-group { position: relative; display: flex; min-width: 0; align-items: center; }
  .channel-row { display: flex; width: 100%; min-width: 0; min-height: var(--explorer-row-height); align-items: center; gap: 0.375rem; border-radius: 0.35rem; padding: var(--explorer-row-padding) 0.5rem; color: var(--foreground); text-align: left; }
  .channel-row-group .channel-row { flex: 1; }
  .explorer-touch .section-heading > button { min-width: var(--touch-target-min); min-height: var(--touch-target-min); }
  .explorer-touch .section-heading > .section-toggle { min-width: 0; }
  .channel-loading { padding-right: 0.55rem; opacity: 0.72; }
</style>
