<script lang="ts">
  import { onMount, tick } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import Check from "@lucide/svelte/icons/check";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import CornerDownLeft from "@lucide/svelte/icons/corner-down-left";
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import Hash from "@lucide/svelte/icons/hash";
  import Search from "@lucide/svelte/icons/search";
  import NotesPageIcon from "$lib/components/notes/pages/NotesPageIcon.svelte";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";
  import ProjectLifecycleIcon from "$lib/components/projects/ProjectLifecycleIcon.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import StarredIconBadge from "$lib/components/ui/StarredIconBadge.svelte";
  import type { ChatChannelRead } from "$lib/chat/contracts";
  import { chatChannelMatchesSearch, normalizeChatChannelSearch } from "$lib/chat/channel-search";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { availableAppViews, type View } from "$lib/navigation";
  import type { NotesFolder, NotesPage } from "$lib/notes/types";
  import { notesHierarchyChildren, type NotesHierarchyOrder } from "$lib/notes/navigation/hierarchy-menu";
  import { notesFoldersForProject } from "$lib/notes/navigation/tree";
  import { NOTES_PAGE_CHROME_EMOJI_SCALE } from "$lib/notes/pages/icon";
  import { notesPageTitle } from "$lib/notes/pages/title";
  import { notesPagesForProject } from "$lib/notes/project-membership";
  import type { Project, ProjectGroup } from "$lib/projects/types";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getChatSidebarOrder } from "$lib/stores/chat/sidebar-order.svelte";
  import { getNotes } from "$lib/stores/notes.svelte";
  import { useProjects } from "$lib/stores/projects.svelte";
  import { getWorkspaceTabs } from "$lib/stores/workspace-tabs.svelte";
  import { anchoredPopover } from "$lib/utils/anchored-popover";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import {
    tabLauncherDigitsPick,
    tabLauncherHighlightIndex,
    tabLauncherKeyAction,
    tabLauncherKindDrills,
    tabLauncherLevel,
    tabLauncherProjectRows,
    tabLauncherRowNumbers,
    type TabLauncherPagesParent,
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

  /** What the unnumbered first row of a level opens: the kind where it was left, a project, or a Notes page. */
  type HereTarget =
    | { type: "continue"; kind: View }
    | { type: "project"; kind: View; projectId: string }
    | { type: "page"; projectId: string; pageId: string };

  type LauncherRow = { key: string; numbered: boolean; canDrill: boolean } & (
    | { type: "kind"; kind: View }
    | { type: "tab"; tab: WorkspaceTab }
    | { type: "here"; target: HereTarget; label: string }
    | { type: "group"; group: ProjectGroup }
    | { type: "project"; kind: View; project: Project }
    | { type: "channel"; projectId: string; channel: ChatChannelRead }
    | { type: "page"; projectId: string; page: NotesPage; title: string }
    | { type: "folder"; projectId: string; folder: NotesFolder }
  );

  /** A listed entry: a row, a section label, or a status line in place of rows still loading or missing. */
  type LauncherEntry =
    | { type: "row"; row: LauncherRow }
    | { type: "label"; key: string; text: string }
    | { type: "loading"; key: string }
    | { type: "status"; key: string; text: string };

  const ICON_SIZE = 13;
  const ICON_STROKE_WIDTH = 1.6;
  const EMOJI_SCALE = 0.94;

  const tabs = getWorkspaceTabs();
  const projects = useProjects();
  const chat = getChat();
  const chatSidebarOrder = getChatSidebarOrder();
  const notes = getNotes();
  const { t } = getLocalization();
  const kinds = availableAppViews();
  const rowIdPrefix = `tab-launcher-${Math.random().toString(36).slice(2)}`;

  let menu: HTMLDivElement | undefined = $state();
  let searchInput: HTMLInputElement | undefined = $state();
  let path = $state<TabLauncherStep[]>([]);
  let query = $state("");
  let highlightKey = $state<string | null>(null);
  let loadFailure = $state<string | null>(null);
  const requestedChildPageIds = new SvelteSet<string>();
  const loadingChildPageIds = new SvelteSet<string>();

  const level = $derived(tabLauncherLevel(path));
  const atKinds = $derived(level.type === "kinds");
  const searchable = $derived(!atKinds);
  const digitsPick = $derived(tabLauncherDigitsPick(query, searchable));
  const entries = $derived(buildEntries());
  const rows = $derived(entries.flatMap((entry) => (entry.type === "row" ? [entry.row] : [])));
  const rowNumbers = $derived(tabLauncherRowNumbers(rows, digitsPick));
  const highlight = $derived(tabLauncherHighlightIndex(rows, highlightKey, query));
  const breadcrumb = $derived(path.map((step) => step.label).join(" / "));
  const searchPlaceholder = $derived(
    level.type === "channels"
      ? t("chat.channels.search")
      : level.type === "pages"
        ? t("notes.searchPlaceholder")
        : t("calendar.eventPanel.searchProjects"),
  );

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
    const here = hereEntry({ type: "continue", kind }, t("titleBar.tab.launcherContinue"));
    if (here) list.push(here);
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
    if (list.every((entry) => entry.type !== "row" || entry.row.type === "here")) {
      list.push({ type: "status", key: "status:empty", text: t("calendar.eventPanel.noProjectsFound") });
    }
    return list;
  }

  function channelEntries(projectId: string): LauncherEntry[] {
    const list: LauncherEntry[] = [];
    const here = hereEntry({ type: "project", kind: "chat", projectId }, t("titleBar.tab.launcherOpenProject"));
    if (here) list.push(here);
    const status = statusEntries();
    if (status) return [...list, ...status];
    if (!chat.loaded) return [...list, { type: "loading", key: "loading" }];
    const normalizedQuery = normalizeChatChannelSearch(query);
    const channels = chatSidebarOrder.sortChannels(projectId, chat.channelsForProject(projectId))
      .filter((channel) => chatChannelMatchesSearch(channel, normalizedQuery));
    for (const channel of channels) {
      list.push(row({ type: "channel", key: `channel:${channel.id}`, numbered: true, canDrill: false, projectId, channel }));
    }
    if (channels.length === 0) {
      list.push({ type: "status", key: "status:empty", text: normalizedQuery ? t("chat.channels.noResults") : t("chat.channels.empty") });
    }
    return list;
  }

  function pageEntries(projectId: string, parent: TabLauncherPagesParent): LauncherEntry[] {
    const list: LauncherEntry[] = [];
    const here = parent.kind === "root"
      ? hereEntry({ type: "project", kind: "notes", projectId }, t("titleBar.tab.launcherOpenProject"))
      : parent.kind === "page"
        ? hereEntry({ type: "page", projectId, pageId: parent.id }, t("titleBar.tab.launcherOpenPage"))
        : null;
    if (here) list.push(here);
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
    const nodes = notesHierarchyChildren(
      notesPagesForProject(notes.allPages, projectId),
      notesFoldersForProject(notes.folders, projectId),
      parent,
      untitled,
      notes.sidebarPageIdsWithChildren,
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
      case "channel": return value.channel.name;
      case "page": return value.title;
      case "folder": return value.folder.name;
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

  function openHere(target: HereTarget): void {
    if (target.type === "continue") {
      open(() => tabs.openKind(target.kind));
    } else if (target.type === "project") {
      open(() => tabs.openLocation("project", { kind: target.kind, projectId: target.projectId, item: null }));
    } else {
      openPage(target.projectId, target.pageId);
    }
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
        openHere(value.target);
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
        open(() => tabs.openLocation("item", {
          kind: "chat",
          projectId: value.projectId,
          item: { type: "chat-channel", id: value.channel.id },
        }));
        return;
      case "page":
        openPage(value.projectId, value.page.id);
        return;
      case "folder":
        void enter({ type: "folder", folderId: value.folder.id, label: value.folder.name });
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
    const action = tabLauncherKeyAction(event, {
      query,
      searchable,
      atKinds,
      rows,
      highlight,
      caretAtEnd: caretAtEnd(),
    });
    if (!action) return;
    event.preventDefault();
    event.stopPropagation();
    switch (action.type) {
      case "highlight": {
        const target = rows[action.index];
        if (!target) return;
        highlightKey = target.key;
        void tick().then(() => document.getElementById(rowId(action.index))?.scrollIntoView({ block: "nearest" }));
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
    <div class="flex shrink-0 flex-col gap-1 border-b border-border/60 p-1">
      <div class="flex min-w-0 items-center gap-1">
        <button
          type="button"
          class="flex size-6 shrink-0 items-center justify-center rounded-md text-popover-foreground/70 hover:bg-accent hover:text-popover-foreground"
          aria-label={t("common.back")}
          title={t("common.back")}
          onclick={() => void back()}
        >
          <ChevronLeft size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} aria-hidden="true" />
        </button>
        <nav class="min-w-0 flex-1 truncate text-panel-detail text-muted-foreground" aria-label={t("titleBar.tab.launcherPath")}>
          {breadcrumb}
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
            aria-keyshortcuts={number === null ? undefined : String(number)}
            tabindex="-1"
            class="menu-item {item.type === 'page' && item.canDrill ? 'pr-8' : ''}"
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
            {:else if item.type === "folder"}
              <Folder size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} class="shrink-0" />
            {:else if item.page.icon}
              <NotesPageIcon icon={item.page.icon} size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} emojiScale={NOTES_PAGE_CHROME_EMOJI_SCALE} class="shrink-0" />
            {:else}
              <FileText size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} class="shrink-0" />
            {/if}
            <span class="min-w-0 flex-1 truncate">{label}</span>
            {#if item.type === "project" && item.project.status !== "active"}
              <ProjectLifecycleIcon status={item.project.status} size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} class="text-popover-foreground/60" />
            {/if}
            {#if number !== null}
              <span class="shrink-0 text-panel-detail text-muted-foreground">{t("common.pressKey", String(number))}</span>
            {/if}
            {#if item.type === "tab" && item.tab.id === tabs.activeTabId}
              <Check class="size-3.5 shrink-0" aria-hidden="true" />
            {/if}
            {#if item.canDrill && item.type !== "page"}
              <ChevronRight size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} class="shrink-0 text-popover-foreground/60" aria-hidden="true" />
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
</div>
