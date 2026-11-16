<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import Archive from "@lucide/svelte/icons/archive";
  import Check from "@lucide/svelte/icons/check";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import CornerDownLeft from "@lucide/svelte/icons/corner-down-left";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import Hash from "@lucide/svelte/icons/hash";
  import Plus from "@lucide/svelte/icons/plus";
  import Search from "@lucide/svelte/icons/search";
  import SquarePen from "@lucide/svelte/icons/square-pen";
  import NotesPageIcon from "$lib/components/notes/pages/NotesPageIcon.svelte";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";
  import ProjectLifecycleIcon from "$lib/components/projects/ProjectLifecycleIcon.svelte";
  import ProjectNameDraftForm from "$lib/components/projects/pickers/ProjectNameDraftForm.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import SidebarOrderMenu from "$lib/components/ui/SidebarOrderMenu.svelte";
  import StarredIconBadge from "$lib/components/ui/StarredIconBadge.svelte";
  import { isProjectNameTakenError } from "$lib/api/projects";
  import type { ChatChannelRead } from "$lib/chat/contracts";
  import { chatChannelPickerRows, normalizeChatChannelSearch } from "$lib/chat/channel-search";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { chatPaneEvents } from "$lib/chat/pane-events";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { availableAppViews, type View } from "$lib/navigation";
  import { notesFailure } from "$lib/notes/failure";
  import type { NotesFolder, NotesPage } from "$lib/notes/types";
  import {
    notesArchivedParentPageIds,
    notesHierarchyArchivedChildren,
    notesHierarchyChildren,
    type NotesHierarchyOrder,
  } from "$lib/notes/navigation/hierarchy-menu";
  import { notesFoldersForProject } from "$lib/notes/navigation/tree";
  import { NOTES_PAGE_CHROME_EMOJI_SCALE } from "$lib/notes/pages/icon";
  import { notesPageTitle } from "$lib/notes/pages/title";
  import { notesPagesForProject } from "$lib/notes/project-membership";
  import { isProjectNameTaken, projectNameKey } from "$lib/projects/text";
  import {
    DEFAULT_PROJECT_GROUP_ICON,
    DEFAULT_PROJECT_ICON,
    type Project,
    type ProjectGroup,
  } from "$lib/projects/types";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getChatSidebarOrder } from "$lib/stores/chat/sidebar-order.svelte";
  import { getNotes } from "$lib/stores/notes.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { useProjects } from "$lib/stores/projects.svelte";
  import { getWorkspaceTabs } from "$lib/stores/workspace-tabs.svelte";
  import { anchoredPopover } from "$lib/utils/anchored-popover";
  import type { SidebarSortOrder } from "$lib/utils/sidebar-order";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import {
    tabLauncherDigitsPick,
    tabLauncherHighlightIndex,
    tabLauncherKeyAction,
    tabLauncherKindDrills,
    tabLauncherLevel,
    tabLauncherProjectRows,
    tabLauncherNumberKey,
    tabLauncherRowNumbers,
    tabLauncherCreateKinds,
    type TabLauncherCreateKind,
    type TabLauncherPagesParent,
    type TabLauncherRowShape,
    type TabLauncherStep,
  } from "$lib/workspace-tabs/launcher";
  import type { WorkspaceTab } from "$lib/workspace-tabs/model";
  import {
    workspaceTabFullLabel,
    workspaceTabIcon,
    workspaceTabKindIcon,
    workspaceTabKindLabel,
  } from "$lib/workspace-tabs/presentation";

  let {
    anchor,
    showOpenTabs,
    onClose,
  }: {
    anchor: HTMLElement | undefined;
    /** List the open tabs too, for when the strip cannot show them all. */
    showOpenTabs: boolean;
    onClose: () => void;
  } = $props();

  /** What the unnumbered first row of a level opens: the Notes page whose children it lists. */
  type HereTarget = { projectId: string; pageId: string };

  type LauncherRow = TabLauncherRowShape & { key: string } & (
    | { type: "create"; kind: TabLauncherCreateKind }
    | { type: "kind"; kind: View }
    | { type: "tab"; tab: WorkspaceTab }
    | { type: "here"; target: HereTarget; label: string }
    | { type: "group"; group: ProjectGroup }
    | { type: "project"; kind: View; project: Project }
    | { type: "channel"; projectId: string; channel: ChatChannelRead }
    | { type: "archived-channel"; projectId: string; channel: ChatChannelRead }
    | { type: "page"; projectId: string; page: NotesPage; title: string }
    | { type: "archived-page"; projectId: string; page: NotesPage; title: string }
    | { type: "folder"; projectId: string; folder: NotesFolder }
  );

  /** The inline form a level's footer shows in place of its create buttons. */
  type LauncherCreator =
    | { type: "folder" }
    | { type: "group" }
    | { type: "project"; groupId: string | null };

  /** A failed launcher action, with raw detail for the copy button. */
  interface LauncherFailure {
    message: string;
    detail: string | undefined;
  }

  /** A listed entry: a row, a section label, or a status line in place of rows still loading or missing. */
  type LauncherEntry =
    | { type: "row"; row: LauncherRow }
    | { type: "label"; key: string; text: string }
    | { type: "loading"; key: string }
    | { type: "status"; key: string; text: string };

  const ICON_SIZE = 13;
  const ICON_STROKE_WIDTH = 1.6;
  const EMOJI_SCALE = 0.94;
  const HEADER_BUTTON_CLASS =
    "flex size-6 shrink-0 items-center justify-center rounded text-popover-foreground/60 hover:bg-accent hover:text-accent-foreground";
  const FOOTER_BUTTON_CLASS =
    "flex min-h-8 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-floating-item px-1 text-popover-foreground transition-colors hover:bg-accent hover:text-accent-foreground data-highlighted:bg-accent data-highlighted:text-accent-foreground";

  const tabs = getWorkspaceTabs();
  const projects = useProjects();
  const chat = getChat();
  const chatSidebarOrder = getChatSidebarOrder();
  const notes = getNotes();
  const preferences = getPreferences();
  const { t } = getLocalization();
  const kinds = availableAppViews();
  const rowIdPrefix = `tab-launcher-${Math.random().toString(36).slice(2)}`;

  let menu: HTMLDivElement | undefined = $state();
  let searchInput: HTMLInputElement | undefined = $state();
  let path = $state<TabLauncherStep[]>([]);
  let query = $state("");
  let highlightKey = $state<string | null>(null);
  let loadFailure = $state<string | null>(null);
  let footer: HTMLDivElement | undefined = $state();
  let folderInput: HTMLInputElement | undefined = $state();
  let creator = $state<LauncherCreator | null>(null);
  let folderDraft = $state("");
  let groupDraft = $state("");
  let groupIcon = $state(DEFAULT_PROJECT_GROUP_ICON);
  let projectDraft = $state("");
  let projectIcon = $state(DEFAULT_PROJECT_ICON);
  /** The name the backend refused as taken, so the form keeps saying so until the name changes. */
  let refusedProjectName = $state<string | null>(null);
  let actionFailure = $state<LauncherFailure | null>(null);
  let archivedChannelsError = $state<string | null>(null);
  let busy = false;
  const requestedChildPageIds = new SvelteSet<string>();
  const loadingChildPageIds = new SvelteSet<string>();

  const level = $derived(tabLauncherLevel(path));
  const atKinds = $derived(level.type === "kinds");
  const searchable = $derived(!atKinds);
  const digitsPick = $derived(tabLauncherDigitsPick(query, searchable));
  const entries = $derived(buildEntries());
  /** The project pickers hide their create buttons while searching; the Notes and Chat pickers keep theirs. */
  const footerShown = $derived(level.type === "channels" || level.type === "pages" || (level.type === "projects" && !query.trim()));
  /** The footer's create actions, which stand in for its buttons while no create form is open. */
  const createRows = $derived<LauncherRow[]>(
    footerShown && creator === null
      ? tabLauncherCreateKinds(level).map(({ kind, primary }) => ({
        type: "create", key: `create:${kind}`, numbered: false, canDrill: false, create: { primary }, kind,
      }))
      : [],
  );
  /** The listed rows, then the footer's create actions, so the arrows reach those last. */
  const rows = $derived([...entries.flatMap((entry) => (entry.type === "row" ? [entry.row] : [])), ...createRows]);
  const rowNumbers = $derived(tabLauncherRowNumbers(rows, digitsPick));
  const highlight = $derived(tabLauncherHighlightIndex(rows, highlightKey, query));
  const levelDrills = $derived(rows.some((value) => value.canDrill));
  const breadcrumb = $derived(path.map((step) => step.label).join(" / "));
  /** The steps the header shows: the kind alone at its first step, then the route inside it, so the kind stops taking room. */
  const shownPath = $derived(path.length > 1 ? path.slice(1) : path);
  const searchPlaceholder = $derived(
    level.type === "channels"
      ? t("chat.channels.search")
      : level.type === "pages"
        ? t("notes.searchPlaceholder")
        : t("calendar.eventPanel.searchProjects"),
  );
  const projectDraftError = $derived.by(() => {
    if (creator?.type !== "project") return null;
    const taken = isProjectNameTaken(projects.projects, creator.groupId, projectDraft)
      || (projectNameKey(projectDraft) !== "" && refusedProjectName === projectNameKey(projectDraft));
    if (!taken) return null;
    return creator.groupId === null ? t("projects.navigator.ungroupedProjectNameTaken") : t("projects.navigator.projectNameTaken");
  });

  $effect(() => {
    if (level.type === "pages" && notes.showArchivedInPickers) untrack(() => notes.ensurePickerArchivedPages());
  });

  $effect(() => {
    archivedChannelsError = null;
    if (level.type !== "channels" || !chat.showArchivedChannelsInPickers) return;
    const projectId = level.projectId;
    let cancelled = false;
    untrack(() => chat.loadArchivedChannelsForPicker(projectId)).catch((error: unknown) => {
      if (!cancelled) archivedChannelsError = chatErrorMessage(error, t("chatErrors.actions.loadArchivedChannels"));
    });
    return () => { cancelled = true; };
  });

  $effect(() => {
    if (level.type === "kinds") return;
    if (!projects.loaded) loadFor(() => projects.ensureLoaded(), "projects", t("titleBar.tab.projects"));
    if (level.type === "channels" && !chat.loaded) {
      loadFor(() => chat.prewarmForProject(null), "Chat channels", t("titleBar.tab.chat"));
    }
    if (level.type === "pages" && !notes.loaded) loadFor(() => notes.ensureLoaded(), "Notes pages", t("titleBar.tab.notes"));
  });

  /** Start loading data a level lists; a failure is logged and replaces the level's rows with a notice. */
  function loadFor(load: () => Promise<void>, context: string, label: string): void {
    load().catch((error: unknown) => {
      console.error(`load ${context} for the tab launcher failed`, error);
      loadFailure = t("common.viewLoadFailed", label);
    });
  }

  function row(value: LauncherRow): LauncherEntry {
    return { type: "row", row: value };
  }

  function buildEntries(): LauncherEntry[] {
    if (level.type === "kinds") return kindEntries();
    if (level.type === "projects") return projectEntries(level.kind, level.groupId);
    if (level.type === "channels") return channelEntries(level.projectId);
    return pageEntries(level.projectId, level.parent);
  }

  function kindEntries(): LauncherEntry[] {
    const list: LauncherEntry[] = kinds.map((kind) =>
      row({ type: "kind", key: `kind:${kind}`, numbered: true, canDrill: tabLauncherKindDrills(kind), kind }),
    );
    if (!showOpenTabs || tabs.tabs.length === 0) return list;
    list.push({ type: "label", key: "label:tabs", text: t("titleBar.tab.openTabs") });
    for (const tab of tabs.tabs) list.push(row({ type: "tab", key: `tab:${tab.id}`, numbered: false, canDrill: false, tab }));
    return list;
  }

  function hereEntry(target: HereTarget, label: string): LauncherEntry | null {
    return query ? null : row({ type: "here", key: "here", numbered: false, canDrill: false, target, label });
  }

  function statusEntries(): LauncherEntry[] | null {
    if (loadFailure) return [{ type: "status", key: "status:failed", text: loadFailure }];
    return null;
  }

  function projectEntries(kind: View, groupId: string | null): LauncherEntry[] {
    const list: LauncherEntry[] = [];
    const status = statusEntries();
    if (status) return [...list, ...status];
    if (!projects.loaded) return [...list, { type: "loading", key: "loading" }];
    const { groups, sections } = tabLauncherProjectRows(projects.projectLists(projects.showInactiveProjects), groupId, query);
    for (const group of groups) list.push(row({ type: "group", key: `group:${group.id}`, numbered: true, canDrill: true, group }));
    for (const { list: projectList, labeled } of sections) {
      if (labeled) {
        list.push({ type: "label", key: `label:${projectList.key}`, text: projectList.group?.name ?? t("projects.navigator.noGroup") });
      }
      for (const project of projectList.projects) {
        list.push(row({ type: "project", key: `project:${project.id}`, numbered: true, canDrill: kind !== "projects", kind, project }));
      }
    }
    if (list.every((entry) => entry.type !== "row")) {
      list.push({ type: "status", key: "status:empty", text: t("calendar.eventPanel.noProjectsFound") });
    }
    return list;
  }

  function channelEntries(projectId: string): LauncherEntry[] {
    const list: LauncherEntry[] = [];
    const status = statusEntries();
    if (status) return status;
    if (!chat.loaded) return [{ type: "loading", key: "loading" }];
    const normalizedQuery = normalizeChatChannelSearch(query);
    const showArchived = chat.showArchivedChannelsInPickers;
    const { active, archived } = chatChannelPickerRows(
      chatSidebarOrder.sortChannels(projectId, chat.channelsForProject(projectId)),
      showArchived ? chat.archivedChannelsForProject(projectId) : [],
      showArchived,
      normalizedQuery,
    );
    for (const channel of active) {
      list.push(row({ type: "channel", key: `channel:${channel.id}`, numbered: true, canDrill: false, projectId, channel }));
    }
    for (const channel of archived) {
      list.push(row({ type: "archived-channel", key: `archived-channel:${channel.id}`, numbered: true, canDrill: false, projectId, channel }));
    }
    if (active.length + archived.length === 0) {
      list.push({ type: "status", key: "status:empty", text: normalizedQuery ? t("chat.channels.noResults") : t("chat.channels.empty") });
      return list;
    }
    return list;
  }

  function pageEntries(projectId: string, parent: TabLauncherPagesParent): LauncherEntry[] {
    const list: LauncherEntry[] = [];
    // A note opens from inside its own children, which the list never shows. A project or folder level only lists what
    // it holds, since picking any of it opens the same project.
    const pageHere = parent.kind === "page"
      ? hereEntry({ projectId, pageId: parent.id }, t("titleBar.tab.launcherOpenPage"))
      : null;
    if (pageHere) list.push(pageHere);
    const status = statusEntries();
    if (status) return [...list, ...status];
    if (!notes.loaded) return [...list, { type: "loading", key: "loading" }];
    const untitled = t("notes.untitled");
    const order: NotesHierarchyOrder = {
      sortOrder: notes.sidebarOrderForProject(projectId),
      favoriteFolderIds: notes.favoriteFolderIds,
      favoritePageIds: notes.favoritePageIds,
      customOrderKeys: notes.customOrderForProject(projectId),
    };
    const normalizedQuery = query.trim().toLocaleLowerCase();
    const archivedPages = notes.showArchivedInPickers ? notesPagesForProject(notes.pickerArchivedPages, projectId) : [];
    const activePageIds = new Set(notes.allPages.map((page) => page.id));
    const nodes = notesHierarchyChildren(
      notesPagesForProject(notes.allPages, projectId),
      notesFoldersForProject(notes.folders, projectId),
      parent,
      untitled,
      // Notes holding only archived children still open a level while archived notes are listed.
      [...notes.sidebarPageIdsWithChildren, ...notesArchivedParentPageIds(archivedPages, activePageIds)],
      order,
    );
    let listed = 0;
    for (const node of nodes) {
      if (node.kind === "folder") {
        if (normalizedQuery && !node.folder.name.toLocaleLowerCase().includes(normalizedQuery)) continue;
        list.push(row({ type: "folder", key: node.key, numbered: true, canDrill: true, projectId, folder: node.folder }));
      } else {
        const title = notesPageTitle(node.page, untitled);
        if (normalizedQuery && !title.toLocaleLowerCase().includes(normalizedQuery)) continue;
        list.push(row({ type: "page", key: node.key, numbered: true, canDrill: node.hasChildren, projectId, page: node.page, title }));
      }
      listed += 1;
    }
    for (const node of notesHierarchyArchivedChildren(archivedPages, activePageIds, parent, untitled, order)) {
      if (node.kind !== "page") continue;
      const title = notesPageTitle(node.page, untitled);
      if (normalizedQuery && !title.toLocaleLowerCase().includes(normalizedQuery)) continue;
      list.push(row({ type: "archived-page", key: `archived-${node.key}`, numbered: true, canDrill: false, projectId, page: node.page, title }));
      listed += 1;
    }
    if (listed > 0) return list;
    if (parent.kind === "page" && loadingChildPageIds.has(parent.id)) return [...list, { type: "loading", key: "loading" }];
    list.push({ type: "status", key: "status:empty", text: normalizedQuery ? t("notes.noSearchResults") : t("notes.noPages") });
    return list;
  }

  function rowLabel(value: LauncherRow): string {
    switch (value.type) {
      case "kind": return workspaceTabKindLabel(value.kind, t);
      case "tab": return workspaceTabFullLabel(value.tab, t);
      case "here": return value.label;
      case "group": return value.group.name;
      case "project": return value.project.name;
      case "channel":
      case "archived-channel":
        return value.channel.name;
      case "page":
      case "archived-page":
        return value.title;
      case "folder": return value.folder.name;
      case "create": return createLabel(value.kind);
    }
  }

  function createLabel(kind: TabLauncherCreateKind): string {
    switch (kind) {
      case "group": return t("calendar.eventPanel.newGroup");
      case "project": return t("calendar.eventPanel.newProject");
      case "folder": return t("notes.newFolder");
      case "page": return t("notes.newPage");
      case "channel": return t("chat.channels.createTitle");
    }
  }

  /** Show a deeper level and put the keyboard in its search. */
  async function enter(step: TabLauncherStep): Promise<void> {
    path = [...path, step];
    resetLevelState();
    await focusLevel();
  }

  async function back(): Promise<void> {
    if (path.length === 0) return;
    path = path.slice(0, -1);
    resetLevelState();
    await focusLevel();
  }

  function resetLevelState(): void {
    query = "";
    highlightKey = null;
    loadFailure = null;
    creator = null;
    actionFailure = null;
  }

  async function focusLevel(): Promise<void> {
    await tick();
    if (searchable) searchInput?.focus();
    else menu?.focus();
  }

  /** Close the launcher and hand the keyboard back to the + button. */
  function finish(): void {
    onClose();
    anchor?.focus();
  }

  function open(action: () => void): void {
    onClose();
    action();
  }

  function openPage(projectId: string, pageId: string): void {
    open(() => tabs.openLocation("item", { kind: "notes", projectId, item: { type: "notes-page", id: pageId } }));
  }

  function pick(value: LauncherRow): void {
    switch (value.type) {
      case "kind":
        if (tabLauncherKindDrills(value.kind)) void enter({ type: "kind", kind: value.kind, label: rowLabel(value) });
        else open(() => tabs.openKind(value.kind));
        return;
      case "tab":
        open(() => tabs.activate(value.tab.id));
        return;
      case "here":
        openPage(value.target.projectId, value.target.pageId);
        return;
      case "group":
        void enter({ type: "group", groupId: value.group.id, label: value.group.name });
        return;
      case "project":
        if (value.kind === "projects") {
          open(() => tabs.openLocation("project", { kind: "projects", projectId: value.project.id, item: null }));
        } else {
          void enter({ type: "project", projectId: value.project.id, label: value.project.name });
        }
        return;
      case "channel":
      case "archived-channel":
        open(() => tabs.openLocation("item", {
          kind: "chat",
          projectId: value.projectId,
          item: { type: "chat-channel", id: value.channel.id },
        }));
        return;
      case "page":
        openPage(value.projectId, value.page.id);
        return;
      case "archived-page":
        void restoreArchivedPage(value.projectId, value.page.id);
        return;
      case "folder":
        void enter({ type: "folder", folderId: value.folder.id, label: value.folder.name });
        return;
      case "create":
        runCreate(value.kind);
        return;
    }
  }

  /** Show what is inside a row: a Notes page's children, or the level any other drillable row opens. */
  function drill(value: LauncherRow): void {
    if (!value.canDrill) return;
    if (value.type !== "page") {
      pick(value);
      return;
    }
    loadPageChildren(value.page.id);
    void enter({ type: "page", pageId: value.page.id, label: value.title });
  }

  /** Run one launcher action at a time, showing its failure above the rows. */
  async function runAction(action: () => Promise<void>, fallback: (error: unknown) => LauncherFailure): Promise<void> {
    if (busy) return;
    busy = true;
    actionFailure = null;
    try {
      await action();
    } catch (error: unknown) {
      console.error("tab launcher action failed", error);
      actionFailure = fallback(error);
    } finally {
      busy = false;
    }
  }

  /** Archived notes cannot open as they are, so picking one restores it and opens it in a new tab. */
  async function restoreArchivedPage(projectId: string, pageId: string): Promise<void> {
    await runAction(async () => {
      await notes.unarchivePage(pageId);
      openPage(projectId, pageId);
    }, (error) => notesFailure(error, t("notes.archivedNoteRestoreFailed")));
  }

  /** Highlight a row a creator just added and keep it in view. */
  async function highlightCreated(key: string): Promise<void> {
    query = "";
    highlightKey = key;
    await focusLevel();
    const index = rows.findIndex((candidate) => candidate.key === key);
    if (index >= 0) document.getElementById(rowId(index))?.scrollIntoView?.({ block: "nearest" });
  }

  function openCreator(next: LauncherCreator): void {
    creator = next;
    actionFailure = null;
    refusedProjectName = null;
    if (next.type === "group") {
      groupDraft = "";
      groupIcon = DEFAULT_PROJECT_GROUP_ICON;
    } else if (next.type === "project") {
      projectDraft = "";
      projectIcon = DEFAULT_PROJECT_ICON;
    } else {
      folderDraft = nextFolderName();
      void tick().then(() => {
        folderInput?.focus();
        folderInput?.select();
      });
    }
  }

  function closeCreator(): void {
    creator = null;
    actionFailure = null;
    void focusLevel();
  }

  function nextFolderName(): string {
    if (level.type !== "pages") return t("notes.defaultFolderName");
    const parentFolderId = level.parent.kind === "folder" ? level.parent.id : null;
    const baseName = t("notes.defaultFolderName");
    const siblingNames = new Set(
      notesFoldersForProject(notes.folders, level.projectId)
        .filter((folder) => folder.parent_folder_id === parentFolderId)
        .map((folder) => folder.name.trim().toLocaleLowerCase()),
    );
    if (!siblingNames.has(baseName.toLocaleLowerCase())) return baseName;
    let suffix = 2;
    while (siblingNames.has(`${baseName} ${suffix}`.toLocaleLowerCase())) suffix += 1;
    return `${baseName} ${suffix}`;
  }

  async function submitFolder(): Promise<void> {
    if (level.type !== "pages" || level.parent.kind === "page") return;
    const name = folderDraft.trim();
    if (!name) return;
    const { projectId, parent } = level;
    await runAction(async () => {
      const folder = await notes.createFolder(projectId, name, parent.kind === "folder" ? parent.id : null);
      creator = null;
      await highlightCreated(`folder:${folder.id}`);
    }, (error) => notesFailure(error, t("notes.folderActionFailed")));
  }

  /** Create a note at this level and open it in a new tab, leaving the current tab where it is. */
  async function createPage(): Promise<void> {
    if (level.type !== "pages") return;
    const { projectId, parent } = level;
    await runAction(async () => {
      const pageId = parent.kind === "page"
        ? await notes.createPageInBackground("", parent.id)
        : await notes.createPageInBackground("", null, { projectId, folderId: parent.kind === "folder" ? parent.id : null });
      openPage(projectId, pageId);
    }, (error) => notesFailure(error, t("notes.pageCreationFailed")));
  }

  /** Open the project in a Chat tab of its own, then start a channel there with the Chat view's own dialog. */
  async function createChannel(): Promise<void> {
    if (level.type !== "channels") return;
    const projectId = level.projectId;
    onClose();
    tabs.openLocation("project", { kind: "chat", projectId, item: null });
    await tabs.whenRestored();
    await tick();
    chatPaneEvents(tabs.paneScope(tabs.activeTabId)).dispatchEvent(new Event("ganbaru:chat-new-channel"));
  }

  async function submitGroup(): Promise<void> {
    const name = groupDraft.trim();
    if (!name) return;
    await runAction(async () => {
      const groupId = await projects.addGroup(name, groupIcon);
      creator = null;
      if (groupId) await highlightCreated(`group:${groupId}`);
    }, (error) => notesFailure(error, t("projects.navigator.groupSaveFailed")));
  }

  /** Create a project without moving any pane to it, and highlight it so it can be opened from here. */
  async function submitProject(): Promise<void> {
    if (creator?.type !== "project") return;
    const { groupId } = creator;
    const name = projectDraft.trim();
    if (!name || projectDraftError) return;
    await runAction(async () => {
      let projectId: string | undefined;
      try {
        projectId = await projects.addProjectWithoutSelecting(groupId, name, projectIcon);
      } catch (error: unknown) {
        if (!isProjectNameTakenError(error)) throw error;
        refusedProjectName = projectNameKey(name);
        return;
      }
      creator = null;
      if (projectId) await highlightCreated(`project:${projectId}`);
    }, (error) => notesFailure(error, t("projects.navigator.projectCreateFailed")));
  }

  /** Run one of the footer's create actions: a folder, group, or project opens its form, and a note or channel opens in a new tab. */
  function runCreate(kind: TabLauncherCreateKind): void {
    switch (kind) {
      case "group":
        openCreator({ type: "group" });
        return;
      case "project":
        openCreator({ type: "project", groupId: level.type === "projects" ? level.groupId : null });
        return;
      case "folder":
        openCreator({ type: "folder" });
        return;
      case "page":
        void createPage();
        return;
      case "channel":
        void createChannel();
        return;
    }
  }

  /** Run the level's primary create action for the new item shortcut, even while a search hides the footer. */
  function createFromShortcut(): void {
    const primary = tabLauncherCreateKinds(level).find((candidate) => candidate.primary);
    if (!primary || creator?.type === primary.kind) return;
    if (primary.kind === "project") query = "";
    highlightKey = null;
    runCreate(primary.kind);
  }

  function loadPageChildren(pageId: string): void {
    if (requestedChildPageIds.has(pageId)) return;
    requestedChildPageIds.add(pageId);
    loadingChildPageIds.add(pageId);
    notes.loadNavigationChildren(pageId)
      .catch((error: unknown) => {
        requestedChildPageIds.delete(pageId);
        console.error("load Notes page children for the tab launcher failed", error);
        loadFailure = t("common.viewLoadFailed", t("titleBar.tab.notes"));
      })
      .finally(() => loadingChildPageIds.delete(pageId));
  }

  function caretAtEnd(): boolean {
    if (!searchInput) return true;
    const end = searchInput.value.length;
    return searchInput.selectionStart === end && searchInput.selectionEnd === end;
  }

  function handleKeydown(event: KeyboardEvent): void {
    if ((event.key === "Enter" || event.key === " ") && event.target instanceof HTMLButtonElement) return;
    if (creator && event.target instanceof Node && footer?.contains(event.target)) {
      // An open create form keeps the keyboard; Escape cancels it before going back a level.
      if (event.key === "Escape" && !event.isComposing) {
        event.preventDefault();
        event.stopPropagation();
        closeCreator();
      }
      return;
    }
    const action = tabLauncherKeyAction(event, {
      query,
      searchable,
      atKinds,
      rows,
      highlight,
      caretAtEnd: caretAtEnd(),
      createShortcut: !atKinds,
    });
    if (!action) return;
    event.preventDefault();
    event.stopPropagation();
    switch (action.type) {
      case "highlight": {
        const target = rows[action.index];
        if (!target) return;
        highlightKey = target.key;
        void tick().then(() => document.getElementById(rowId(action.index))?.scrollIntoView?.({ block: "nearest" }));
        return;
      }
      case "pick": {
        const target = rows[action.index];
        if (target) pick(target);
        return;
      }
      case "drill": {
        const target = rows[action.index];
        if (target) drill(target);
        return;
      }
      case "clear-search":
        query = "";
        highlightKey = null;
        return;
      case "back":
        void back();
        return;
      case "close":
        finish();
        return;
      case "create":
        createFromShortcut();
        return;
    }
  }

  function rowId(index: number): string {
    return `${rowIdPrefix}-${index}`;
  }

  function updateQuery(value: string): void {
    query = value;
    highlightKey = null;
  }

  onMount(() => {
    menu?.focus();
  });
</script>

{#snippet orderMenu(order: SidebarSortOrder, override: SidebarSortOrder | null, allowGlobal: boolean, label: string, onSelect: (order: SidebarSortOrder | null) => void)}
  <SidebarOrderMenu
    {order}
    {override}
    {allowGlobal}
    {label}
    buttonClass={HEADER_BUTTON_CLASS}
    activeClass="bg-accent text-accent-foreground"
    iconSize={ICON_SIZE}
    iconStrokeWidth={ICON_STROKE_WIDTH}
    menuZIndexClass="z-90"
    {onSelect}
  />
{/snippet}

{#snippet eyeButton(pressed: boolean, label: string, toggle: () => void)}
  <button type="button" class={HEADER_BUTTON_CLASS} aria-pressed={pressed} aria-label={label} title={label} onclick={toggle}>
    {#if pressed}
      <EyeOff size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} aria-hidden="true" />
    {:else}
      <Eye size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} aria-hidden="true" />
    {/if}
  </button>
{/snippet}

{#snippet createIcon(kind: TabLauncherCreateKind)}
  {#if kind === "folder"}
    <FolderPlus size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} class="shrink-0" aria-hidden="true" />
  {:else if kind === "page"}
    <SquarePen size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} class="shrink-0" aria-hidden="true" />
  {:else}
    <Plus size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} class="shrink-0" aria-hidden="true" />
  {/if}
{/snippet}

{#snippet archivedMarker(label: string)}
  <span role="img" class="shrink-0 text-popover-foreground/60" aria-label={label}>
    <Archive size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} aria-hidden="true" />
  </span>
{/snippet}

<div
  bind:this={menu}
  use:portal
  use:anchoredPopover={{ anchor, onOutside: onClose, horizontalAlign: "start" }}
  class="surface-floating fixed z-90 flex flex-col overflow-hidden {atKinds ? 'w-floating-sm' : 'w-floating'}"
  role={atKinds ? "menu" : "dialog"}
  tabindex="-1"
  aria-label={t("titleBar.tab.launcher")}
  aria-activedescendant={atKinds && highlight >= 0 ? rowId(highlight) : undefined}
  data-app-floating-surface
  onkeydown={handleKeydown}
>
  {#if !atKinds}
    <div class="flex shrink-0 flex-col gap-1 p-1">
      <div class="flex min-w-0 items-center gap-1">
        <button
          type="button"
          class="flex size-(--panel-row-height) shrink-0 items-center justify-center rounded-floating-item text-popover-foreground hover:bg-accent hover:text-accent-foreground"
          aria-label={t("common.back")}
          title={t("common.back")}
          onclick={() => void back()}
        >
          <ChevronLeft size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} aria-hidden="true" />
        </button>
        <nav class="min-w-0 flex-1 truncate text-popover-foreground" aria-label={t("titleBar.tab.launcherPath")}>
          {#each shownPath as step, index (index)}
            {#if index > 0}<span class="px-1 text-muted-foreground" aria-hidden="true">/</span>{/if}
            {#if index === shownPath.length - 1}
              <span class="font-medium" aria-current="location">{step.label}</span>
            {:else}
              <span>{step.label}</span>
            {/if}
          {/each}
        </nav>
      </div>
      <div class="field flex items-center gap-1.5 py-0 pl-2 pr-1">
        <Search size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} class="shrink-0 text-popover-foreground/60" aria-hidden="true" />
        <input
          bind:this={searchInput}
          type="search"
          value={query}
          oninput={(event) => updateQuery(event.currentTarget.value)}
          placeholder={searchPlaceholder}
          aria-label={searchPlaceholder}
          aria-controls={`${rowIdPrefix}-list`}
          aria-activedescendant={highlight >= 0 ? rowId(highlight) : undefined}
          autocomplete="off"
          spellcheck="false"
          class="field-bare text-popover-foreground"
        />
        {#if level.type === "projects"}
          {@render orderMenu(preferences.projectsSidebarOrder, null, false, t("projects.navigator.sortGroupsAndProjects"), (order) => {
            if (order) preferences.setProjectsSidebarOrder(order);
          })}
          {@render eyeButton(
            projects.showInactiveProjects,
            projects.showInactiveProjects ? t("projects.navigator.hideInactive") : t("projects.navigator.showInactive"),
            () => { projects.showInactiveProjects = !projects.showInactiveProjects; },
          )}
        {:else if level.type === "channels"}
          {@const projectId = level.projectId}
          {@render orderMenu(chatSidebarOrder.orderForProject(projectId), chatSidebarOrder.orderOverride(projectId), true, t("chat.channels.sort"), (order) => {
            chatSidebarOrder.setProjectOrder(projectId, order);
          })}
          {@render eyeButton(
            chat.showArchivedChannelsInPickers,
            chat.showArchivedChannelsInPickers ? t("chat.channels.hideArchived") : t("chat.channels.showArchived"),
            () => { chat.showArchivedChannelsInPickers = !chat.showArchivedChannelsInPickers; },
          )}
        {:else if level.type === "pages"}
          {@const projectId = level.projectId}
          {@render orderMenu(notes.sidebarOrderForProject(projectId), notes.sidebarOrderOverride(projectId), true, t("notes.sortExplorer"), (order) => {
            notes.setProjectSidebarOrder(projectId, order);
          })}
          {@render eyeButton(
            notes.showArchivedInPickers,
            notes.showArchivedInPickers ? t("notes.hideArchivedNotes") : t("notes.showArchivedNotes"),
            () => { notes.showArchivedInPickers = !notes.showArchivedInPickers; },
          )}
        {/if}
      </div>
    </div>
  {/if}

  <div
    id={`${rowIdPrefix}-list`}
    class="surface-floating-body flex min-h-0 flex-1 flex-col overflow-y-auto overflow-x-hidden"
    role={atKinds ? undefined : "menu"}
    aria-label={atKinds ? undefined : breadcrumb}
    use:scrollEdgeFadeAction
  >
    {#if actionFailure}
      <Notice class="px-3 py-2" message={actionFailure.message} copy={actionFailure.detail} onDismiss={() => (actionFailure = null)} />
    {/if}
    {#if level.type === "pages" && notes.showArchivedInPickers && notes.pickerArchiveError}
      <Notice
        class="px-3 py-2"
        message={t("notes.archivedNotesLoadFailed")}
        copy={notes.pickerArchiveError}
        onRetry={() => notes.reloadPickerArchivedPages()}
      />
    {/if}
    {#if level.type === "channels" && archivedChannelsError}
      <Notice class="px-3 py-2" message={archivedChannelsError} />
    {/if}
    {#each entries as entry (entry.type === "row" ? entry.row.key : entry.key)}
      {#if entry.type === "label"}
        {#if entry.key === "label:tabs"}<div role="separator" class="menu-separator"></div>{/if}
        <p class="menu-label">{entry.text}</p>
      {:else if entry.type === "loading"}
        <Loading layout="rows" rows={3} label={t("common.loading")} />
      {:else if entry.type === "status"}
        <p class="px-2 py-1.5 text-panel-detail text-muted-foreground">{entry.text}</p>
      {:else}
        {@const item = entry.row}
        {@const index = rows.indexOf(item)}
        {@const number = rowNumbers[index] ?? null}
        {@const label = rowLabel(item)}
        <div class="relative">
          <button
            type="button"
            id={rowId(index)}
            role={item.type === "tab" ? "menuitemradio" : "menuitem"}
            aria-checked={item.type === "tab" ? item.tab.id === tabs.activeTabId : undefined}
            aria-keyshortcuts={number === null ? undefined : tabLauncherNumberKey(number)}
            aria-label={item.type === "archived-page" ? t("notes.archivedNoteRestore", label) : undefined}
            title={item.type === "archived-page" ? t("notes.archivedNoteRestore", label) : undefined}
            tabindex="-1"
            class="menu-item"
            data-highlighted={index === highlight || undefined}
            onpointermove={() => (highlightKey = item.key)}
            onmousedown={(event) => event.preventDefault()}
            onclick={() => pick(item)}
          >
            {#if item.type === "kind"}
              {@const Icon = workspaceTabKindIcon(item.kind)}
              <Icon />
            {:else if item.type === "tab"}
              {@const Icon = workspaceTabIcon(item.tab)}
              <Icon />
            {:else if item.type === "here"}
              <CornerDownLeft size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} class="shrink-0 text-popover-foreground/60" />
            {:else if item.type === "group"}
              <ProjectIcon name={item.group.icon} size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} emojiScale={EMOJI_SCALE} class="shrink-0" />
            {:else if item.type === "project"}
              <StarredIconBadge starred={projects.isProjectStarred(item.project.id)}>
                <ProjectIcon name={item.project.icon} size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} emojiScale={EMOJI_SCALE} />
              </StarredIconBadge>
            {:else if item.type === "channel"}
              <StarredIconBadge starred={chatSidebarOrder.isChannelStarred(item.channel.id)}>
                <Hash size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} />
              </StarredIconBadge>
            {:else if item.type === "archived-channel"}
              <Hash size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} class="shrink-0" />
            {:else if item.type === "folder"}
              <Folder size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} class="shrink-0" />
            {:else if item.type === "create"}
              {@render createIcon(item.kind)}
            {:else if item.page.icon}
              <NotesPageIcon icon={item.page.icon} size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} emojiScale={NOTES_PAGE_CHROME_EMOJI_SCALE} class="shrink-0" />
            {:else}
              <FileText size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} class="shrink-0" />
            {/if}
            <span class="min-w-0 flex-1 truncate">{label}</span>
            {#if item.type === "archived-channel"}
              {@render archivedMarker(t("chat.channels.archivedChannel"))}
            {:else if item.type === "archived-page"}
              {@render archivedMarker(t("notes.archivedNote"))}
            {/if}
            {#if item.type === "project" && item.project.status !== "active"}
              <ProjectLifecycleIcon status={item.project.status} size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} class="text-popover-foreground/60" />
            {/if}
            {#if number !== null}
              <span class="shrink-0 text-panel-detail text-muted-foreground tabular-nums">{t("common.pressKey", tabLauncherNumberKey(number))}</span>
            {/if}
            {#if item.type === "tab" && item.tab.id === tabs.activeTabId}
              <Check class="size-3.5 shrink-0" aria-hidden="true" />
            {/if}
            {#if levelDrills}
              <!-- Every row of a level that has a drillable row keeps the chevron's slot, so number hints line up. -->
              <span class="flex w-4 shrink-0 justify-center" aria-hidden="true">
                {#if item.canDrill && item.type !== "page"}
                  <ChevronRight size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} class="text-popover-foreground/60" />
                {/if}
              </span>
            {/if}
          </button>
          {#if item.type === "page" && item.canDrill}
            <button
              type="button"
              tabindex="-1"
              class="absolute inset-y-0 right-1 my-auto flex size-6 items-center justify-center rounded-md text-popover-foreground/60 hover:bg-popover hover:text-popover-foreground"
              aria-label={t("titleBar.tab.launcherShowInside", label)}
              title={t("titleBar.tab.launcherShowInside", label)}
              onpointermove={() => (highlightKey = item.key)}
              onmousedown={(event) => event.preventDefault()}
              onclick={() => drill(item)}
            >
              <ChevronRight size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} aria-hidden="true" />
            </button>
          {/if}
        </div>
      {/if}
    {/each}
  </div>

  {#if footerShown}
    <div bind:this={footer} class="relative z-10 shrink-0 bg-popover p-1.5">
      <div class="pointer-events-none absolute left-1.5 right-1.5 top-0 border-t border-border/70"></div>
      {#if creator?.type === "folder"}
        <form class="flex gap-1" onsubmit={(event) => { event.preventDefault(); void submitFolder(); }}>
          <input
            bind:this={folderInput}
            bind:value={folderDraft}
            placeholder={t("notes.defaultFolderName")}
            aria-label={t("notes.newFolder")}
            class="field min-w-0 flex-1 text-popover-foreground"
          />
          <button type="submit" class="min-h-8 rounded-floating-item bg-primary px-2 text-panel-detail font-medium text-primary-foreground">
            {t("common.save")}
          </button>
        </form>
      {:else if creator?.type === "group"}
        <ProjectNameDraftForm
          bind:name={groupDraft}
          bind:icon={groupIcon}
          placeholder={t("projects.navigator.groupNamePlaceholder")}
          iconStrokeWidth={ICON_STROKE_WIDTH}
          onSubmit={() => void submitGroup()}
          onCancel={closeCreator}
        />
      {:else if creator?.type === "project"}
        <ProjectNameDraftForm
          bind:name={projectDraft}
          bind:icon={projectIcon}
          placeholder={t("projects.navigator.projectNamePlaceholder")}
          error={projectDraftError}
          iconStrokeWidth={ICON_STROKE_WIDTH}
          onSubmit={() => void submitProject()}
          onCancel={closeCreator}
        />
      {:else}
        <div class="flex min-h-8 items-center">
          {#each createRows as item, position (item.key)}
            {@const index = rows.indexOf(item)}
            {#if position > 0}<div class="mx-1 h-5 border-l border-border/70" aria-hidden="true"></div>{/if}
            <button
              type="button"
              id={rowId(index)}
              class={FOOTER_BUTTON_CLASS}
              aria-keyshortcuts={item.create?.primary ? "Control+N Meta+N" : undefined}
              data-highlighted={index === highlight || undefined}
              onpointermove={() => (highlightKey = item.key)}
              onclick={() => pick(item)}
            >
              {#if item.type === "create"}{@render createIcon(item.kind)}{/if}
              <span class="truncate">{rowLabel(item)}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>
