<script lang="ts">
  import { onMount } from "svelte";
  import {
    listNotesProjectHistoryVersions,
    loadNotesProjectHistoryPage,
    loadNotesProjectHistoryTree,
    previewNotesProjectHistoryRestore,
    restoreNotesProjectHistoryVersion,
  } from "$lib/api/notes/project-history";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type {
    NotesHistoricalPage,
    NotesProjectHistoryRestorePlan,
    NotesProjectHistoryTree,
    NotesProjectHistoryVersion,
  } from "$lib/notes/types";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import NotesHistoricalPagePreview from "./NotesHistoricalPagePreview.svelte";
  import NotesHistoricalProjectHome from "./NotesHistoricalProjectHome.svelte";
  import NotesVersionHistoryModalShell from "./NotesVersionHistoryModalShell.svelte";

  let {
    projectId,
    onClose,
    onRestored,
  }: {
    projectId: string;
    onClose: () => void;
    onRestored: () => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const viewport = getViewport();
  let versions = $state<NotesProjectHistoryVersion[]>([]);
  let cursorTime = $state<string | null>(null);
  let cursorId = $state<string | null>(null);
  let loadingVersions = $state(true);
  let loadingMore = $state(false);
  let error = $state<string | null>(null);
  let selectedVersionId = $state<string | null>(null);
  let historicalTree = $state<NotesProjectHistoryTree | null>(null);
  let selectedPage = $state<NotesHistoricalPage | null>(null);
  let loadingPreview = $state(false);
  let restorePlan = $state<NotesProjectHistoryRestorePlan | null>(null);
  let restoring = $state(false);
  let versionsRequestId = 0;
  let previewRequestId = 0;
  let pageRequestId = 0;
  const compact = $derived(viewport.width < 720);
  const compactPreviewOpen = $derived(compact && selectedVersionId !== null);
  const selectedVersion = $derived(
    versions.find((version) => version.id === selectedVersionId) ?? null,
  );

  function formatVersionDate(value: string): string {
    return new Intl.DateTimeFormat(localization.locale, {
      dateStyle: "medium",
    }).format(new Date(value));
  }

  function formatVersionHour(value: string): string {
    return new Intl.DateTimeFormat(localization.locale, {
      timeStyle: "short",
    }).format(new Date(value));
  }

  async function loadVersions(reset: boolean): Promise<void> {
    const currentRequest = ++versionsRequestId;
    if (reset) {
      loadingVersions = true;
      error = null;
    } else {
      loadingMore = true;
    }
    try {
      const result = await listNotesProjectHistoryVersions({
        projectId,
        cursorTime: reset ? null : cursorTime,
        cursorId: reset ? null : cursorId,
        pageSize: 40,
      });
      if (currentRequest !== versionsRequestId) return;
      versions = reset ? result.versions : [...versions, ...result.versions];
      cursorTime = result.nextCursorTime;
      cursorId = result.nextCursorId;
      if (reset && versions[0]) void selectVersion(versions[0].id, false);
    } catch (cause) {
      if (currentRequest !== versionsRequestId) return;
      const message = cause instanceof Error ? cause.message : String(cause);
      error = t("notes.projectHistoryLoadFailed", message);
    } finally {
      if (currentRequest === versionsRequestId) {
        loadingVersions = false;
        loadingMore = false;
      }
    }
  }

  async function selectVersion(versionId: string, shouldOpenCompactPreview = true): Promise<void> {
    selectedVersionId = versionId;
    historicalTree = null;
    selectedPage = null;
    loadingPreview = true;
    const currentRequest = ++previewRequestId;
    try {
      const tree = await loadNotesProjectHistoryTree(projectId, versionId);
      if (currentRequest !== previewRequestId || selectedVersionId !== versionId) return;
      historicalTree = tree;
      if (!shouldOpenCompactPreview && compact) selectedVersionId = null;
    } catch (cause) {
      if (currentRequest !== previewRequestId) return;
      const message = cause instanceof Error ? cause.message : String(cause);
      error = t("notes.projectHistoryLoadFailed", message);
    } finally {
      if (currentRequest === previewRequestId) loadingPreview = false;
    }
  }

  async function openHistoricalPage(pageId: string): Promise<void> {
    if (!selectedVersionId) return;
    const versionId = selectedVersionId;
    const currentRequest = ++pageRequestId;
    loadingPreview = true;
    try {
      const page = await loadNotesProjectHistoryPage(projectId, versionId, pageId);
      if (currentRequest !== pageRequestId || versionId !== selectedVersionId) return;
      selectedPage = page;
    } catch (cause) {
      if (currentRequest !== pageRequestId) return;
      const message = cause instanceof Error ? cause.message : String(cause);
      error = t("notes.projectHistoryLoadFailed", message);
    } finally {
      if (currentRequest === pageRequestId) loadingPreview = false;
    }
  }

  async function requestRestore(): Promise<void> {
    if (!selectedVersionId || restoring) return;
    restoring = true;
    error = null;
    try {
      restorePlan = await previewNotesProjectHistoryRestore(projectId, selectedVersionId);
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      error = t("notes.projectHistoryRestoreFailed", message);
    } finally {
      restoring = false;
    }
  }

  async function confirmRestore(): Promise<void> {
    const plan = restorePlan;
    if (!plan || restoring) return;
    restorePlan = null;
    restoring = true;
    error = null;
    try {
      await restoreNotesProjectHistoryVersion(projectId, plan.versionId);
      onRestored();
      onClose();
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      error = t("notes.projectHistoryRestoreFailed", message);
    } finally {
      restoring = false;
    }
  }

  function closeCompactPreview(): void {
    selectedVersionId = null;
    historicalTree = null;
    selectedPage = null;
  }

  onMount(() => {
    void loadVersions(true);
    return () => {
      versionsRequestId += 1;
      previewRequestId += 1;
      pageRequestId += 1;
    };
  });
</script>

<NotesVersionHistoryModalShell
  title={t("notes.projectHistoryTitle")}
  closeLabel={t("notes.projectHistoryClose")}
  backLabel={t("notes.projectHistoryBack")}
  restoreLabel={t("notes.projectHistoryRestore")}
  restoringLabel={t("notes.projectHistoryRestoring")}
  {compact}
  {compactPreviewOpen}
  previewBusy={loadingPreview}
  versionsBusy={loadingVersions || loadingMore}
  canRestore={selectedVersion !== null}
  {restoring}
  {error}
  dismissBlocked={restorePlan !== null}
  {onClose}
  onBack={closeCompactPreview}
  onRestore={() => { void requestRestore(); }}
>
  {#snippet preview()}
    {#if selectedPage}
      <NotesHistoricalPagePreview page={selectedPage} onBack={() => { selectedPage = null; }} />
    {:else if historicalTree}
      <NotesHistoricalProjectHome
        pages={historicalTree.pages}
        selectedPageId={null}
        onOpenPage={(pageId) => { void openHistoricalPage(pageId); }}
      />
    {/if}
  {/snippet}

  {#snippet versionList()}
    {#if !loadingVersions && versions.length === 0}
      <div class="px-2 py-3 text-[0.8rem] text-muted-foreground">
        {t("notes.projectHistoryEmpty")}
      </div>
    {:else}
      {#each versions as version (version.id)}
        <button
          type="button"
          class={`mb-1 block w-full rounded-md px-3 py-2 text-left transition-colors hover:bg-accent ${
            selectedVersionId === version.id ? "bg-accent" : ""
          }`}
          onclick={() => { void selectVersion(version.id); }}
        >
          <span class="block text-[0.866667rem] font-medium">
            {formatVersionDate(version.createdTime)} · {formatVersionHour(version.createdTime)}
          </span>
          <span class="mt-0.5 block truncate text-[0.733333rem] text-muted-foreground">
            {version.displayName.resolved_name}
          </span>
        </button>
      {/each}
      {#if cursorTime && cursorId}
        <button
          type="button"
          class="w-full rounded-md px-3 py-2 text-[0.8rem] text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50"
          disabled={loadingMore}
          onclick={() => { void loadVersions(false); }}
        >
          {t("notes.projectHistoryLoadMore")}
        </button>
      {/if}
    {/if}
  {/snippet}
</NotesVersionHistoryModalShell>

{#if restorePlan}
  <ConfirmDialog
    title={t("notes.projectHistoryRestoreTitle")}
    message={t(
      "notes.projectHistoryRestoreMessage",
      restorePlan.removeCount,
      restorePlan.recreateCount,
      restorePlan.changeCount,
      restorePlan.copyCount,
    )}
    confirmLabel={t("notes.projectHistoryRestore")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => { void confirmRestore(); }}
    onCancel={() => { restorePlan = null; }}
  />
{/if}
