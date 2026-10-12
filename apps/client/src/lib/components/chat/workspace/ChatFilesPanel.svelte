<script lang="ts">
  import { onMount } from "svelte";
  import Copy from "@lucide/svelte/icons/copy";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Eye from "@lucide/svelte/icons/eye";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import PanelLeftClose from "@lucide/svelte/icons/panel-left-close";
  import PanelLeftOpen from "@lucide/svelte/icons/panel-left-open";
  import Paperclip from "@lucide/svelte/icons/paperclip";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Save from "@lucide/svelte/icons/save";
  import Search from "@lucide/svelte/icons/search";
  import MessageSquarePlus from "@lucide/svelte/icons/message-square-plus";
  import TextSelect from "@lucide/svelte/icons/text-select";
  import * as chatApi from "$lib/api/chat";
  import type {
    ChatWorkspaceChangeBatch,
    ChatWorkspaceObserverStatusRead,
    ProjectWorkingFolderFileEntry,
    ProjectWorkingFolderPathRead,
  } from "$lib/chat/contracts";
  import { flattenChatFileTree, type ChatFileTreeRow } from "$lib/chat/workspace/file-tree-model";
  import {
    mergeWorkspaceChangeBatches,
    workspaceDirectoryRefreshTargets,
    workspacePathAfterRenames,
  } from "$lib/chat/workspace/change-model";
  import {
    subscribeChatWorkspaceChanges,
    subscribeChatWorkspaceObserverStatus,
  } from "$lib/chat/workspace/observer-client";
  import { splitPaneResizeBounds } from "$lib/chat/workspace/inspector-model";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { alignPanelSizeToDevicePixel, panelWidthFromKey } from "$lib/chat/responsive-layout";
  import { ChatFileEditorSession } from "$lib/chat/workspace/file-editor-session.svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import ChatCodePreview from "./ChatCodePreview.svelte";
  import ChatFileIcon from "./ChatFileIcon.svelte";
  import ChatPaneResizeHandle from "./ChatPaneResizeHandle.svelte";
  import ChatWorkspaceFileTree from "./ChatWorkspaceFileTree.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  let {
    active = true,
    directoryPath,
    selectedPath,
    treeVisible,
    treeWidthPx,
    onStateChange,
    onReviewCreated = () => {},
  }: {
    active?: boolean;
    directoryPath: string;
    selectedPath: string | null;
    treeVisible: boolean;
    treeWidthPx: number;
    onStateChange: (change: {
      directoryPath?: string;
      selectedPath?: string | null;
      treeVisible?: boolean;
      treeWidthPx?: number;
    }) => void;
    onReviewCreated?: () => void;
  } = $props();

  const DEFAULT_TREE_WIDTH = 220;
  const MIN_TREE_WIDTH = 144;
  const MIN_PREVIEW_WIDTH = 120;
  const MAX_TREE_WIDTH = 360;

  const localization = getLocalization();
  const { t } = localization;
  const chat = getChat();
  let rootEntries = $state<ProjectWorkingFolderFileEntry[]>([]);
  let childrenByDirectory = $state<Record<string, ProjectWorkingFolderFileEntry[]>>({});
  let expandedPaths = $state<string[]>([]);
  let loadingPaths = $state<string[]>([]);
  let previewElement: HTMLElement | undefined = $state();
  let panelElement: HTMLDivElement | undefined = $state();
  let panelWidth = $state(0);
  let renderedTreeWidth = $state(DEFAULT_TREE_WIDTH);
  let resizingTree = $state(false);
  let query = $state("");
  let searchResults = $state<ProjectWorkingFolderPathRead[]>([]);
  let includeIgnored = $state(false);
  let loadingRoot = $state(false);
  let loadingRootVisible = $state(false);
  let refreshingTree = $state(false);
  let observerStatus = $state<ChatWorkspaceObserverStatusRead | null>(null);
  let error = $state<string | null>(null);
  let loadedScope = "";
  let selectedPathSyncKey = "";
  let treeRequestId = 0;
  let visibleTreeRefreshRequestId = 0;
  let searchRefreshRevision = $state(0);
  let pendingWorkspaceChange: ChatWorkspaceChangeBatch | null = null;
  let reconcilingWorkspaceChanges = false;
  let filesPanelMounted = false;
  let treeResizeFrame: number | null = null;
  let treeResizeEndFrame: number | null = null;
  let rootLoadingTimer: number | null = null;
  let activeScopeKey = "";
  const editorSession = new ChatFileEditorSession({
    selectedPath: () => selectedPath,
    setSelectedPath: (path) => onStateChange({ selectedPath: path }),
    refreshParentDirectory: (path) => refreshParentDirectory(path),
    onReviewCreated: () => onReviewCreated(),
    discardUnsavedMessage: () => t("chat.inspector.discardUnsaved"),
    confirmOverwriteMessage: () => t("chat.inspector.confirmOverwriteFile"),
    confirmRecreateMessage: () => t("chat.inspector.confirmRecreateFile"),
    previewUnavailableMessage: () => t("chat.inspector.previewUnavailable"),
  });
  const workingFolderId = $derived(chat.selectedWorkingFolderId);
  const treeRows = $derived(flattenChatFileTree(rootEntries, childrenByDirectory, expandedPaths));
  const shownRows = $derived.by<ChatFileTreeRow[]>(() => {
    if (!query.trim()) return treeRows;
    return searchResults.map((entry) => ({
      entry: { ...entry, byteSize: null },
      depth: 0,
      expanded: entry.kind === "directory" && expandedPaths.includes(entry.relativePath),
    }));
  });
  const changedPaths = $derived(new Set(
    chat.timelinePages.flatMap((page) => page.turns.flatMap((turn) => turn.changedFiles.map((file) => file.relativePath))),
  ));
  const fileDirty = $derived(editorSession.dirty);
  const visibleError = $derived(editorSession.error ?? error);
  const observerMatchesScope = $derived(
    observerStatus?.workingFolderId === workingFolderId
      && observerStatus?.executionEnvironmentId === chat.selectedExecutionEnvironmentId,
  );

  onMount(() => {
    filesPanelMounted = true;
    const observer = new ResizeObserver(([entry]) => {
      if (entry) panelWidth = entry.contentRect.width;
    });
    if (panelElement) observer.observe(panelElement);
    return () => {
      filesPanelMounted = false;
      pendingWorkspaceChange = null;
      observer.disconnect();
      if (treeResizeFrame !== null) window.cancelAnimationFrame(treeResizeFrame);
      if (treeResizeEndFrame !== null) window.cancelAnimationFrame(treeResizeEndFrame);
      editorSession.destroy();
      if (rootLoadingTimer !== null) window.clearTimeout(rootLoadingTimer);
    };
  });

  $effect(() => {
    if (!active) {
      pendingWorkspaceChange = null;
      observerStatus = null;
      return;
    }
    const unsubscribeChanges = subscribeChatWorkspaceChanges((batch) => {
      pendingWorkspaceChange = mergeWorkspaceChangeBatches(pendingWorkspaceChange, batch);
      if (!reconcilingWorkspaceChanges) void drainWorkspaceChanges();
    });
    const unsubscribeStatus = subscribeChatWorkspaceObserverStatus((status) => {
      observerStatus = status;
    });
    return () => {
      unsubscribeChanges();
      unsubscribeStatus();
    };
  });

  $effect(() => {
    const configuredWidth = treeWidthPx;
    const availableWidth = panelWidth;
    if (!resizingTree) {
      renderedTreeWidth = availableWidth > 0
        ? alignTreeWidth(configuredWidth)
        : configuredWidth;
    }
  });

  $effect(() => {
    const workspace = workingFolderId;
    const executionEnvironmentId = chat.selectedExecutionEnvironmentId;
    const scope = `${workspace ?? ""}:${chat.selectedThreadId ?? ""}:${executionEnvironmentId ?? ""}`;
    if (scope === loadedScope) return;
    loadedScope = scope;
    selectedPathSyncKey = "";
    treeRequestId += 1;
    pendingWorkspaceChange = null;
    rootEntries = [];
    childrenByDirectory = {};
    expandedPaths = [];
    loadingPaths = [];
    loadingRoot = false;
    loadingRootVisible = false;
    if (rootLoadingTimer !== null) window.clearTimeout(rootLoadingTimer);
    rootLoadingTimer = null;
    refreshingTree = false;
    editorSession.reset();
    activeScopeKey = "";
  });

  $effect(() => {
    const isActive = active;
    const workspace = workingFolderId;
    const executionEnvironmentId = chat.selectedExecutionEnvironmentId;
    const scopeKey = loadedScope;
    if (!isActive || !workspace) {
      activeScopeKey = "";
      return;
    }
    if (scopeKey === activeScopeKey) return;
    activeScopeKey = scopeKey;
    void loadRoot(workspace, directoryPath, executionEnvironmentId);
    if (editorSession.preview && selectedPath) {
      void editorSession.refreshFromDisk(selectedPath, false);
    }
  });

  $effect(() => {
    const workspace = workingFolderId;
    const environmentId = chat.selectedExecutionEnvironmentId;
    const path = selectedPath;
    const key = `${loadedScope}\u0000${path ?? ""}`;
    if (!active || !workspace || !path
      || editorSession.preview?.relativePath === path
      || selectedPathSyncKey === key) return;
    selectedPathSyncKey = key;
    const previousPath = editorSession.preview?.relativePath ?? null;
    void selectFile(path, environmentId).then((opened) => {
      if (!opened && previousPath && selectedPath === path && workingFolderId === workspace
        && chat.selectedExecutionEnvironmentId === environmentId) {
        onStateChange({ selectedPath: previousPath });
      }
    });
  });

  $effect(() => {
    const workspace = workingFolderId;
    const value = query.trim();
    const showIgnored = includeIgnored;
    const executionEnvironmentId = chat.selectedExecutionEnvironmentId;
    const requestedRevision = searchRefreshRevision;
    if (!active || !workspace || !value) {
      searchResults = [];
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void chatApi.searchChatWorkingFolderPaths(workspace, value, showIgnored, null, 100, executionEnvironmentId)
        .then((page) => {
          if (!cancelled && requestedRevision === searchRefreshRevision) searchResults = page.entries;
        })
        .catch((reason: unknown) => { if (!cancelled) error = message(reason); });
    }, 160);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  });

  async function loadRoot(
    workspace: string,
    pathToReveal = "",
    executionEnvironmentId = chat.selectedExecutionEnvironmentId,
  ): Promise<void> {
    const requestId = ++treeRequestId;
    beginRootLoading();
    error = null;
    try {
      const result = await chatApi.listProjectWorkingFolderDirectory(workspace, "", includeIgnored, executionEnvironmentId);
      if (requestId !== treeRequestId || workspace !== workingFolderId) return;
      rootEntries = result.entries;
      if (pathToReveal) await revealDirectory(pathToReveal);
    } catch (reason: unknown) {
      if (requestId === treeRequestId) error = message(reason);
    } finally {
      if (requestId === treeRequestId) finishRootLoading();
    }
  }

  async function loadDirectory(path: string): Promise<void> {
    const workspace = workingFolderId;
    if (!workspace || childrenByDirectory[path] || loadingPaths.includes(path)) return;
    const requestId = treeRequestId;
    loadingPaths = [...loadingPaths, path];
    try {
      const result = await chatApi.listProjectWorkingFolderDirectory(workspace, path, includeIgnored, chat.selectedExecutionEnvironmentId);
      if (requestId !== treeRequestId || workspace !== workingFolderId) return;
      childrenByDirectory = { ...childrenByDirectory, [path]: result.entries };
    } catch (reason: unknown) {
      if (requestId === treeRequestId) error = message(reason);
    } finally {
      loadingPaths = loadingPaths.filter((entry) => entry !== path);
    }
  }

  async function refreshLoadedDirectories(
    paths = ["", ...Object.keys(childrenByDirectory)],
    showProgress = true,
    showIgnored = includeIgnored,
  ): Promise<void> {
    const workspace = workingFolderId;
    if (!workspace || paths.length === 0) return;
    const requestId = ++treeRequestId;
    const environmentId = chat.selectedExecutionEnvironmentId;
    const uniquePaths = [...new Set(paths)];
    if (showProgress) {
      visibleTreeRefreshRequestId = requestId;
      refreshingTree = true;
    }
    const loadingEmptyRoot = rootEntries.length === 0 && uniquePaths.includes("");
    if (loadingEmptyRoot) beginRootLoading();
    error = null;
    try {
      for (let index = 0; index < uniquePaths.length; index += 3) {
        const group = uniquePaths.slice(index, index + 3);
        const reads = await Promise.all(group.map(async (path) => {
          try {
            const value = await chatApi.listProjectWorkingFolderDirectory(
              workspace,
              path,
              showIgnored,
              environmentId,
            );
            return { path, value, reason: null };
          } catch (reason: unknown) {
            return { path, value: null, reason };
          }
        }));
        if (requestId !== treeRequestId || workspace !== workingFolderId) return;
        const nextChildren = { ...childrenByDirectory };
        for (const read of reads) {
          if (read.value) {
            if (read.path === "") rootEntries = read.value.entries;
            else nextChildren[read.path] = read.value.entries;
            continue;
          }
          if (read.path !== "" && errorCode(read.reason) === "not_found") {
            for (const path of Object.keys(nextChildren)) {
              if (path === read.path || path.startsWith(`${read.path}/`)) delete nextChildren[path];
            }
            expandedPaths = expandedPaths.filter((path) => (
              path !== read.path && !path.startsWith(`${read.path}/`)
            ));
            continue;
          }
          if (read.reason !== null) error = message(read.reason);
        }
        childrenByDirectory = nextChildren;
      }
    } finally {
      if (requestId === treeRequestId && loadingEmptyRoot) finishRootLoading();
      if (showProgress && visibleTreeRefreshRequestId === requestId) refreshingTree = false;
    }
  }

  async function drainWorkspaceChanges(): Promise<void> {
    if (reconcilingWorkspaceChanges) return;
    reconcilingWorkspaceChanges = true;
    try {
      while (filesPanelMounted && pendingWorkspaceChange) {
        const batch = pendingWorkspaceChange;
        pendingWorkspaceChange = null;
        await reconcileWorkspaceChange(batch);
      }
    } catch (reason: unknown) {
      if (filesPanelMounted) error = message(reason);
    } finally {
      reconcilingWorkspaceChanges = false;
      if (filesPanelMounted && pendingWorkspaceChange) void drainWorkspaceChanges();
    }
  }

  async function reconcileWorkspaceChange(batch: ChatWorkspaceChangeBatch): Promise<void> {
    if (batch.workingFolderId !== workingFolderId
      || batch.executionEnvironmentId !== chat.selectedExecutionEnvironmentId) return;
    if (batch.renames.length > 0) {
      rootEntries = rootEntries.map((entry) => workspaceEntryAfterRenames(entry, batch));
      const nextChildren: Record<string, ProjectWorkingFolderFileEntry[]> = {};
      for (const [directory, entries] of Object.entries(childrenByDirectory)) {
        const mappedDirectory = workspacePathAfterRenames(directory, batch.renames);
        nextChildren[mappedDirectory] = entries.map((entry) => (
          workspaceEntryAfterRenames(entry, batch)
        ));
      }
      childrenByDirectory = nextChildren;
      expandedPaths = [...new Set(expandedPaths.map((path) => (
        workspacePathAfterRenames(path, batch.renames)
      )))];
    }
    const loadedDirectories = Object.keys(childrenByDirectory);
    const refreshTargets = workspaceDirectoryRefreshTargets(batch, loadedDirectories)
      .map((path) => workspacePathAfterRenames(path, batch.renames));
    if (refreshTargets.length > 0) {
      await refreshLoadedDirectories(refreshTargets, false);
    }
    searchRefreshRevision += 1;
    await editorSession.reconcileWorkspaceChange(batch);
  }

  function workspaceEntryAfterRenames(
    entry: ProjectWorkingFolderFileEntry,
    batch: ChatWorkspaceChangeBatch,
  ): ProjectWorkingFolderFileEntry {
    const relativePath = workspacePathAfterRenames(entry.relativePath, batch.renames);
    return {
      ...entry,
      relativePath,
      displayName: relativePath === entry.relativePath
        ? entry.displayName
        : relativePath.slice(relativePath.lastIndexOf("/") + 1),
    };
  }

  async function revealDirectory(path: string): Promise<void> {
    const parts = path.split("/").filter(Boolean);
    let current = "";
    for (const part of parts) {
      current = current ? `${current}/${part}` : part;
      if (!expandedPaths.includes(current)) expandedPaths = [...expandedPaths, current];
      await loadDirectory(current);
    }
  }

  function toggleDirectory(entry: ProjectWorkingFolderFileEntry): void {
    if (entry.kind !== "directory") return;
    if (query.trim()) {
      query = "";
      void revealDirectory(entry.relativePath);
      return;
    }
    if (expandedPaths.includes(entry.relativePath)) {
      expandedPaths = expandedPaths.filter((path) => path !== entry.relativePath);
      return;
    }
    expandedPaths = [...expandedPaths, entry.relativePath];
    onStateChange({ directoryPath: entry.relativePath });
    void loadDirectory(entry.relativePath);
  }

  async function selectFile(
    path: string,
    executionEnvironmentId = chat.selectedExecutionEnvironmentId,
    discardDirty = false,
  ): Promise<boolean> {
    selectedPathSyncKey = `${loadedScope}\u0000${path}`;
    return editorSession.select(path, executionEnvironmentId, discardDirty);
  }

  function beginRootLoading(): void {
    loadingRoot = true;
    loadingRootVisible = false;
    if (rootLoadingTimer !== null) window.clearTimeout(rootLoadingTimer);
    rootLoadingTimer = window.setTimeout(() => {
      rootLoadingTimer = null;
      if (loadingRoot) loadingRootVisible = true;
    }, 140);
  }

  function finishRootLoading(): void {
    loadingRoot = false;
    loadingRootVisible = false;
    if (rootLoadingTimer !== null) window.clearTimeout(rootLoadingTimer);
    rootLoadingTimer = null;
  }

  function openEntry(entry: ProjectWorkingFolderFileEntry): void {
    if (entry.kind === "directory") toggleDirectory(entry);
    else void selectFile(entry.relativePath);
  }

  function reloadTree(): void {
    void (async () => {
      await refreshLoadedDirectories();
      searchRefreshRevision += 1;
      if (selectedPath) await editorSession.refreshFromDisk(selectedPath, false);
    })();
  }

  function toggleIgnored(): void {
    const next = !includeIgnored;
    includeIgnored = next;
    void refreshLoadedDirectories(undefined, true, next);
  }

  async function refreshParentDirectory(path: string): Promise<void> {
    const parent = path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "";
    if (parent === "" || Object.hasOwn(childrenByDirectory, parent)) {
      await refreshLoadedDirectories([parent], false);
    }
    searchRefreshRevision += 1;
  }

  function attachFileReference(path: string): void {
    const mentions = chat.composer.mentions.filter((entry) => entry.relativePath !== path);
    chat.setComposerMentions([...mentions, { relativePath: path, kind: "file", ignored: false }]);
  }

  function treeResizeBounds(): { minimum: number; maximum: number } {
    return splitPaneResizeBounds(
      panelWidth,
      MIN_TREE_WIDTH,
      MIN_PREVIEW_WIDTH,
      MAX_TREE_WIDTH,
    );
  }

  function alignTreeWidth(value: number): number {
    const bounds = treeResizeBounds();
    const anchor = panelElement?.getBoundingClientRect().left ?? 0;
    return alignPanelSizeToDevicePixel({
      value,
      minimum: bounds.minimum,
      maximum: bounds.maximum,
      anchor,
      direction: "from-start",
      devicePixelRatio: window.devicePixelRatio,
    });
  }

  function beginTreeResize(event: PointerEvent): void {
    event.preventDefault();
    if (panelElement) panelWidth = panelElement.getBoundingClientRect().width;
    if (treeResizeEndFrame !== null) window.cancelAnimationFrame(treeResizeEndFrame);
    treeResizeEndFrame = null;
    resizingTree = true;
    const startX = event.clientX;
    const startWidth = renderedTreeWidth;
    const target = event.currentTarget as HTMLElement;
    let pendingWidth = startWidth;
    target.focus();
    target.setPointerCapture(event.pointerId);
    const move = (moveEvent: PointerEvent) => {
      pendingWidth = alignTreeWidth(startWidth + moveEvent.clientX - startX);
      if (treeResizeFrame !== null) return;
      treeResizeFrame = window.requestAnimationFrame(() => {
        renderedTreeWidth = pendingWidth;
        treeResizeFrame = null;
      });
    };
    const end = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", end);
      target.removeEventListener("pointercancel", end);
      if (treeResizeFrame !== null) window.cancelAnimationFrame(treeResizeFrame);
      treeResizeFrame = null;
      renderedTreeWidth = pendingWidth;
      onStateChange({ treeWidthPx: pendingWidth });
      treeResizeEndFrame = window.requestAnimationFrame(() => {
        treeResizeEndFrame = window.requestAnimationFrame(() => {
          resizingTree = false;
          treeResizeEndFrame = null;
        });
      });
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", end);
    target.addEventListener("pointercancel", end);
  }

  function resizeTreeFromKey(event: KeyboardEvent): void {
    const bounds = treeResizeBounds();
    const next = panelWidthFromKey({
      current: renderedTreeWidth,
      minimum: bounds.minimum,
      maximum: bounds.maximum,
      defaultValue: DEFAULT_TREE_WIDTH,
      step: 16,
      direction: "standard",
      key: event.key,
    });
    if (next === null) return;
    event.preventDefault();
    renderedTreeWidth = alignTreeWidth(next);
    onStateChange({ treeWidthPx: renderedTreeWidth });
  }

  function message(reason: unknown): string {
    return chatErrorMessage(reason);
  }

  function errorCode(reason: unknown): string | null {
    if (typeof reason !== "object" || reason === null || !("code" in reason)) return null;
    return typeof reason.code === "string" ? reason.code : null;
  }
</script>

<div class="files-panel">
  <div bind:this={panelElement} class="files-layout" class:resizing={resizingTree} style={`--file-tree-width:${renderedTreeWidth}px`}>
    {#if treeVisible}
      <aside class="file-tree-pane" aria-label={t("chat.inspector.files")}>
        <div class="tree-toolbar">
          <label class="file-search"><Search size={13} /><input type="search" bind:value={query} placeholder={t("chat.inspector.searchFiles")} aria-label={t("chat.inspector.searchFiles")} /></label>
          <button type="button" class="tree-action" class:active={includeIgnored} aria-pressed={includeIgnored} title={t("chat.inspector.showIgnored")} aria-label={t("chat.inspector.showIgnored")} onclick={toggleIgnored}><Eye size={13} /></button>
          <button type="button" class="tree-action" title={t("chat.inspector.refreshFiles")} aria-label={t("chat.inspector.refreshFiles")} onclick={reloadTree}><RefreshCw size={13} class={refreshingTree ? "animate-spin" : ""} /></button>
          <button type="button" class="tree-action" title={t("chat.inspector.hideFileTree")} aria-label={t("chat.inspector.hideFileTree")} onclick={() => onStateChange({ treeVisible: false })}><PanelLeftClose size={13} /></button>
        </div>
        {#if observerMatchesScope && observerStatus?.mode === "polling"}
          <p role="status" class="live-update-status">{t("chat.inspector.liveUpdatesPolling")}</p>
        {:else if observerMatchesScope && observerStatus?.mode === "unavailable"}
          <p role="status" class="live-update-status warning">{t("chat.inspector.liveUpdatesUnavailable")}</p>
        {:else if observerMatchesScope && observerStatus?.degradedReason}
          <p role="status" class="live-update-status warning">{t("chat.inspector.liveUpdatesDegraded")}</p>
        {/if}
        {#if visibleError}<Notice class="border-b border-border p-2 text-xs" message={visibleError} />{/if}
        {#if loadingRoot && !loadingRootVisible}
          <span aria-hidden="true"></span>
        {:else if loadingRootVisible}
          <Loading layout="rows" rows={3} delay={0} />
        {:else if shownRows.length > 0}
          <ChatWorkspaceFileTree
            rows={shownRows}
            {selectedPath}
            {changedPaths}
            {loadingPaths}
            refreshing={refreshingTree}
            onToggle={toggleDirectory}
            onSelect={openEntry}
          />
        {:else}
          <p class="p-2 text-xs text-muted-foreground">{query.trim() ? t("chat.inspector.noFileResults") : t("chat.inspector.emptyDirectory")}</p>
        {/if}
      </aside>
      <ChatPaneResizeHandle orientation="vertical" value={renderedTreeWidth} minimum={treeResizeBounds().minimum} maximum={treeResizeBounds().maximum} label={t("chat.resizeFileTree")} active={resizingTree} onPointerDown={beginTreeResize} onKeyDown={resizeTreeFromKey} />
    {/if}

    <section bind:this={previewElement} class="file-editor" aria-label={t("chat.inspector.filePreview")}>
      <header class="editor-heading">
        {#if !treeVisible}<button type="button" class="chat-icon-button" title={t("chat.inspector.showFileTree")} aria-label={t("chat.inspector.showFileTree")} onclick={() => onStateChange({ treeVisible: true })}><PanelLeftOpen size={13} /></button>{/if}
        {#if editorSession.preview}
          <ChatFileIcon path={editorSession.preview.relativePath} />
          <strong class="min-w-0 flex-1 truncate text-[0.733333rem] font-medium" title={editorSession.preview.relativePath}>{editorSession.preview.relativePath}</strong>
          {#if fileDirty}<span class="dirty-indicator" title={t("chat.inspector.unsavedChanges")} aria-label={t("chat.inspector.unsavedChanges")}></span>{/if}
          <button type="button" class="chat-icon-button" disabled={editorSession.saving || (!editorSession.fileDeleted && (editorSession.saveConflict || !fileDirty || !editorSession.preview.contentRevision))} title={editorSession.saving ? t("chat.inspector.savingFile") : editorSession.fileDeleted ? t("chat.inspector.recreateFile") : t("chat.inspector.saveFile")} aria-label={editorSession.saving ? t("chat.inspector.savingFile") : editorSession.fileDeleted ? t("chat.inspector.recreateFile") : t("chat.inspector.saveFile")} onclick={() => void editorSession.save()}><Save size={13} /></button>
          <button type="button" class="chat-icon-button" title={t("chat.inspector.copyPath")} aria-label={t("chat.inspector.copyPath")} onclick={() => navigator.clipboard.writeText(selectedPath ?? "")}><Copy size={13} /></button>
          <button type="button" class="chat-icon-button" disabled={editorSession.fileDeleted} title={t("chat.inspector.attachFile")} aria-label={t("chat.inspector.attachFile")} onclick={() => selectedPath && attachFileReference(selectedPath)}><Paperclip size={13} /></button>
          <button type="button" class="chat-icon-button" title={t("chat.inspector.attachSelection")} aria-label={t("chat.inspector.attachSelection")} onclick={() => void editorSession.attachSelection()}><TextSelect size={13} /></button>
          <button type="button" class="chat-icon-button" disabled={editorSession.fileDeleted || !editorSession.editorSelection?.text || !editorSession.preview.contentRevision || !chat.selectedThreadId} title={t("chat.review.addComment")} aria-label={t("chat.review.addComment")} onclick={() => { editorSession.reviewComposerOpen = !editorSession.reviewComposerOpen; }}><MessageSquarePlus size={13} /></button>
          <button type="button" class="chat-icon-button" disabled={editorSession.fileDeleted} title={t("chat.inspector.openExternally")} aria-label={t("chat.inspector.openExternally")} onclick={() => workingFolderId && selectedPath && chatApi.openProjectWorkingFolderFile(workingFolderId, selectedPath, chat.selectedExecutionEnvironmentId)}><ExternalLink size={13} /></button>
        {:else}
          <span class="min-w-0 flex-1 truncate text-[0.733333rem] text-muted-foreground">{t("chat.inspector.filePreview")}</span>
        {/if}
      </header>
      {#if editorSession.reviewComposerOpen && editorSession.editorSelection?.text}
        <form class="review-composer" onsubmit={(event) => { event.preventDefault(); void editorSession.createReviewComment(); }}>
          <label for="chat-review-comment">{t("chat.review.commentOnSelection", editorSession.editorSelection.startLine, editorSession.editorSelection.endLine)}</label>
          <textarea class="field" id="chat-review-comment" bind:value={editorSession.reviewDraft} maxlength="65536" placeholder={t("chat.review.commentPlaceholder")}></textarea>
          <div>
            <button type="button" onclick={() => editorSession.closeReviewComposer()}>{t("common.cancel")}</button>
            <button type="submit" class="primary inline-flex items-center gap-1" disabled={!editorSession.reviewDraft.trim() || editorSession.creatingReview} aria-busy={editorSession.creatingReview || undefined}>{#if editorSession.creatingReview}<LoaderCircle class="size-3 animate-spin" aria-hidden="true" />{/if}{t("chat.review.add")}</button>
          </div>
        </form>
      {/if}
      {#if visibleError && !treeVisible}<Notice class="border-b border-border p-2 text-xs" message={visibleError} />{/if}
      {#if editorSession.fileDeleted}
        <div role="alert" class="save-conflict">
          <span>{fileDirty ? t("chat.inspector.fileDeletedWithUnsavedChanges") : t("chat.inspector.fileDeletedOnDisk")}</span>
          <div><button type="button" onclick={() => void editorSession.recreateDeletedFile()}>{t("chat.inspector.recreateFile")}</button></div>
          {#if fileDirty}
            <form onsubmit={(event) => { event.preventDefault(); void editorSession.saveConflictCopy(); }}><input class="field" bind:value={editorSession.saveCopyPath} aria-label={t("chat.inspector.saveCopyPath")} /><button type="submit" disabled={!editorSession.saveCopyPath.trim() || editorSession.saving}>{t("chat.inspector.saveCopy")}</button></form>
          {/if}
        </div>
      {:else if editorSession.saveConflict}
        <div role="alert" class="save-conflict">
          <span>{t("chat.inspector.saveConflict")}</span>
          <div><button type="button" onclick={() => void editorSession.compareConflict()}>{t("chat.inspector.compareFile")}</button><button type="button" onclick={() => editorSession.reloadSelectedFile()}>{t("chat.inspector.reloadFile")}</button><button type="button" onclick={() => void editorSession.overwriteExternalFile()}>{t("chat.inspector.overwriteFile")}</button></div>
          <form onsubmit={(event) => { event.preventDefault(); void editorSession.saveConflictCopy(); }}><input class="field" bind:value={editorSession.saveCopyPath} aria-label={t("chat.inspector.saveCopyPath")} /><button type="submit" disabled={!editorSession.saveCopyPath.trim() || editorSession.saving}>{t("chat.inspector.saveCopy")}</button></form>
        </div>
      {/if}
      {#if editorSession.conflictDiskText !== null && editorSession.preview}
        <section class="conflict-compare" aria-label={t("chat.inspector.compareFile")}>
          <div><strong>{t("chat.inspector.diskVersion")}</strong><pre>{editorSession.conflictDiskText}</pre></div>
          <div><strong>{t("chat.inspector.yourVersion")}</strong><pre>{editorSession.draftText}</pre></div>
        </section>
      {/if}
      {#if editorSession.loading && !editorSession.loadingVisible && !editorSession.preview}
        <span aria-hidden="true"></span>
      {:else if editorSession.loadingVisible && !editorSession.preview}
        <Loading layout="fill" delay={0} class="min-h-0 flex-1" />
      {:else if editorSession.preview && editorSession.preview.text !== null}
        <ChatCodePreview
          text={editorSession.draftText}
          relativePath={editorSession.preview.relativePath}
          onChange={(text) => editorSession.setDraftText(text)}
          onSelectionChange={(selection) => editorSession.setSelection(selection)}
          onSave={() => void editorSession.save()}
        />
        {#if editorSession.loadingVisible}<div class="preview-loading-indicator" aria-hidden="true"></div>{/if}
      {:else if editorSession.preview}
        <div class="m-auto p-4 text-center text-xs text-muted-foreground">
          <p>{editorSession.preview.binary ? t("chat.inspector.binary") : editorSession.preview.oversized ? t("chat.inspector.previewOversized") : t("chat.inspector.previewUnavailable")}</p>
          <p>{t("chat.inspector.fileSizeBytes", formatNumber(localization.locale, editorSession.preview.byteSize))}</p>
        </div>
      {:else}
        <p class="m-auto p-4 text-xs text-muted-foreground">{t("chat.inspector.selectFile")}</p>
      {/if}
    </section>
  </div>
</div>

<style>
  .files-panel { display: flex; height: 100%; min-height: 0; flex-direction: column; }
  .files-layout { display: flex; min-height: 0; flex: 1; }
  .files-layout.resizing, .files-layout.resizing * { user-select: none; }
  .file-tree-pane { display: flex; width: var(--file-tree-width); min-width: 0; min-height: 0; flex: 0 0 var(--file-tree-width); flex-direction: column; overflow: hidden; background: color-mix(in srgb, var(--cal-bg) 96%, var(--muted)); }
  .tree-toolbar { display: flex; min-height: 2.45rem; flex: 0 0 auto; align-items: center; gap: 0.15rem; border-bottom: 1px solid var(--border); padding: 0.3rem; }
  .file-search { display: flex; min-width: 0; min-height: 1.75rem; flex: 1; align-items: center; gap: 0.35rem; border-radius: 0.4rem; padding-inline: 0.4rem; color: var(--muted-foreground); }
  .file-search:focus-within { color: var(--foreground); }
  .file-search input { min-width: 0; flex: 1; background: transparent; color: var(--foreground); font-size: calc(0.7rem * var(--type-scale)); outline: none; }
  .tree-action { display: inline-grid; width: 1.7rem; height: 1.7rem; flex: 0 0 auto; place-items: center; border-radius: 0.35rem; color: var(--foreground); }
  .tree-action:hover, .tree-action.active { background: var(--accent); }
  .live-update-status { flex: 0 0 auto; border-bottom: 1px solid var(--border); padding: 0.3rem 0.45rem; color: var(--muted-foreground); font-size: calc(0.66rem * var(--type-scale)); line-height: 1.2; }
  .live-update-status.warning { color: var(--status-tentative); }
  .file-editor { position: relative; display: flex; min-width: 0; min-height: 0; flex: 1; flex-direction: column; overflow: hidden; }
  .preview-loading-indicator { position: absolute; top: 2.38rem; right: 0; left: 0; z-index: 2; height: 2px; background: linear-gradient(90deg, transparent, var(--primary), transparent); opacity: 0.65; }
  .editor-heading { display: flex; min-height: 2.45rem; flex: 0 0 auto; align-items: center; gap: 0.25rem; border-bottom: 1px solid var(--border); padding: 0.3rem 0.4rem; }
  .dirty-indicator { width: 0.45rem; height: 0.45rem; flex: 0 0 auto; border-radius: 999px; background: var(--status-tentative); }
  .save-conflict { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 0.35rem; border-bottom: 1px solid color-mix(in srgb, var(--destructive) 35%, var(--border)); background: color-mix(in srgb, var(--destructive) 8%, var(--background)); padding: 0.4rem 0.55rem; color: var(--destructive); font-size: calc(0.7rem * var(--type-scale)); }
  .save-conflict > div { display: flex; gap: 0.25rem; }
  .save-conflict form { grid-column: 1 / -1; display: flex; min-width: 0; gap: 0.35rem; }
  .save-conflict input { min-width: 0; flex: 1; color: var(--foreground); }
  .save-conflict button { flex: 0 0 auto; border-radius: 0.35rem; padding: 0.2rem 0.45rem; color: var(--foreground); }
  .save-conflict button:hover { background: var(--accent); }
  .conflict-compare { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); max-height: 35%; overflow: hidden; border-bottom: 1px solid var(--border); }
  .conflict-compare > div { min-width: 0; overflow: auto; padding: 0.4rem; }
  .conflict-compare > div + div { border-left: 1px solid var(--border); }
  .conflict-compare strong { font-size: calc(0.66rem * var(--type-scale)); }
  .conflict-compare pre { margin-top: 0.3rem; white-space: pre-wrap; overflow-wrap: anywhere; font-size: calc(0.66rem * var(--type-scale)); }
  .review-composer { display: grid; flex: 0 0 auto; gap: 0.4rem; border-bottom: 1px solid var(--border); padding: 0.55rem; background: var(--background); }
  .review-composer label { font-size: calc(0.7rem * var(--type-scale)); color: var(--muted-foreground); }
  .review-composer textarea { min-height: 4rem; max-height: 9rem; resize: vertical; color: var(--foreground); font-size: var(--panel-font-size); }
  .review-composer div { display: flex; justify-content: flex-end; gap: 0.35rem; }
  .review-composer button { border-radius: 0.35rem; padding: 0.3rem 0.55rem; font-size: calc(0.7rem * var(--type-scale)); }
  .review-composer button:hover { background: var(--accent); }
  .review-composer button.primary { background: var(--primary); color: var(--primary-foreground); }
  .review-composer button:disabled { opacity: 0.5; }
</style>
