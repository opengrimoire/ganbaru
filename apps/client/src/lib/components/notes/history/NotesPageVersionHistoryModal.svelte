<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import Copy from "@lucide/svelte/icons/copy";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesLocalUserDisplayName } from "$lib/notes/collaboration/local-user";
  import { notesPageTitle } from "$lib/notes/pages/title";
  import type {
    NotesHistoricalPage,
    NotesLoadedPage,
    NotesPageHistorySnapshot,
  } from "$lib/notes/types";
  import { getNotesEditor } from "$lib/components/notes/editor-context";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import NotesHistoricalPagePreview from "./NotesHistoricalPagePreview.svelte";
  import NotesVersionHistoryModalShell from "./NotesVersionHistoryModalShell.svelte";

  let {
    pageId,
    onClose,
  }: {
    pageId: string;
    onClose: () => void;
  } = $props();

  const notes = getNotesEditor();
  const localization = getLocalization();
  const { t } = localization;
  const viewport = getViewport();
  let initializing = $state(true);
  let selectedSnapshotId = $state<string | null>(null);
  let loadedSnapshotId = $state<string | null>(null);
  let restoreSnapshotId = $state<string | null>(null);
  let restoring = $state(false);
  let copying = $state(false);
  let previewRequestId = 0;
  const compact = $derived(viewport.width < 720);
  const compactPreviewOpen = $derived(compact && selectedSnapshotId !== null);
  const selectedSnapshot = $derived(
    notes.pageHistorySnapshots.find((snapshot) => snapshot.id === selectedSnapshotId) ?? null,
  );
  const restoreSnapshot = $derived(
    notes.pageHistorySnapshots.find((snapshot) => snapshot.id === restoreSnapshotId) ?? null,
  );
  const historicalPage = $derived.by(() => {
    if (
      !selectedSnapshotId
      || loadedSnapshotId !== selectedSnapshotId
      || !notes.pageHistoryVersion
    ) {
      return null;
    }
    return historicalPageFromLoaded(notes.pageHistoryVersion);
  });
  const error = $derived(
    notes.pageHistorySnapshotsError
      ? t("notes.loadPageHistoryFailed", notes.pageHistorySnapshotsError)
      : notes.pageHistoryVersionError
        ? t("notes.loadPageHistoryVersionFailed", notes.pageHistoryVersionError)
        : notes.pageHistoryActionError
          ? t("notes.pageHistoryActionFailed", notes.pageHistoryActionError)
          : null,
  );

  function historicalPageFromLoaded(loaded: NotesLoadedPage): NotesHistoricalPage {
    return {
      id: loaded.page.id,
      title: notesPageTitle(loaded.page, t("notes.untitled")),
      properties: loaded.page.properties,
      icon: loaded.page.icon,
      cover: loaded.page.cover,
      inTrash: loaded.page.in_trash,
      archived: loaded.page.archived ?? false,
      blocks: loaded.blocks.results as unknown as Record<string, unknown>[],
      databases: [],
      dataSources: [],
      databaseViews: [],
    };
  }

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

  function formatVersionTimestamp(value: string): string {
    return new Intl.DateTimeFormat(localization.locale, {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(new Date(value));
  }

  function snapshotAuthorName(snapshot: NotesPageHistorySnapshot): string {
    if (notes.localUser?.id === snapshot.created_by.id) {
      return notesLocalUserDisplayName(notes.localUser.display_name);
    }
    return snapshot.created_by.id;
  }

  async function openSnapshot(snapshotId: string): Promise<void> {
    selectedSnapshotId = snapshotId;
    loadedSnapshotId = null;
    const requestId = ++previewRequestId;
    await notes.loadPageHistoryVersion(snapshotId);
    if (
      requestId === previewRequestId
      && selectedSnapshotId === snapshotId
      && !notes.pageHistoryVersionError
    ) {
      loadedSnapshotId = snapshotId;
    }
  }

  function closeCompactPreview(): void {
    selectedSnapshotId = null;
    loadedSnapshotId = null;
    previewRequestId += 1;
  }

  async function copySelectedBlocks(): Promise<void> {
    const snapshot = selectedSnapshot;
    if (!snapshot || snapshot.block_count === 0 || copying || restoring) return;
    copying = true;
    await notes.copyPageHistoryBlocks(snapshot.id);
    copying = false;
  }

  async function confirmRestore(): Promise<void> {
    const snapshot = restoreSnapshot;
    if (!snapshot || restoring) return;
    restoreSnapshotId = null;
    restoring = true;
    await notes.restorePageHistoryVersion(snapshot.id);
    restoring = false;
    if (!notes.pageHistoryActionError) onClose();
  }

  $effect(() => {
    if (initializing || notes.pageHistorySnapshotsLoading || selectedSnapshotId) return;
    const firstSnapshot = notes.pageHistorySnapshots[0];
    if (!firstSnapshot || compact) return;
    void openSnapshot(firstSnapshot.id);
  });

  $effect(() => {
    if (!selectedSnapshotId) return;
    if (notes.pageHistorySnapshots.some((snapshot) => snapshot.id === selectedSnapshotId)) return;
    closeCompactPreview();
  });

  onMount(() => {
    if (!notes.localUser && !notes.localUserLoading) {
      void notes.ensureOptionalSubsystem("local-user");
    }
    void notes.reloadPageHistory(pageId).finally(() => {
      initializing = false;
    });
  });

  onDestroy(() => {
    previewRequestId += 1;
  });
</script>

<NotesVersionHistoryModalShell
  title={t("notes.projectHistoryTitle")}
  closeLabel={t("notes.projectHistoryClose")}
  backLabel={t("notes.projectHistoryBack")}
  restoreLabel={t("notes.restorePageHistoryVersion")}
  restoringLabel={t("notes.projectHistoryRestoring")}
  {compact}
  {compactPreviewOpen}
  previewBusy={notes.pageHistoryVersionLoading}
  versionsBusy={initializing || notes.pageHistorySnapshotsLoading}
  canRestore={selectedSnapshot !== null}
  {restoring}
  {error}
  dismissBlocked={restoreSnapshotId !== null}
  {onClose}
  onBack={closeCompactPreview}
  onRestore={() => {
    if (selectedSnapshot) restoreSnapshotId = selectedSnapshot.id;
  }}
>
  {#snippet preview()}
    {#if historicalPage}
      <NotesHistoricalPagePreview page={historicalPage} showBack={false} onBack={() => {}} />
    {/if}
  {/snippet}

  {#snippet versionList()}
    {#if !initializing && !notes.pageHistorySnapshotsLoading && notes.pageHistorySnapshots.length === 0}
      <div class="px-2 py-3 text-[0.8rem] text-muted-foreground">
        {t("notes.noPageHistory")}
      </div>
    {:else}
      {#each notes.pageHistorySnapshots as snapshot (snapshot.id)}
        <button
          type="button"
          class={`mb-1 block w-full rounded-md px-3 py-2 text-left transition-colors hover:bg-accent ${
            selectedSnapshotId === snapshot.id ? "bg-accent" : ""
          }`}
          aria-label={t("notes.pageHistoryVersionLabel", formatVersionTimestamp(snapshot.created_time))}
          onclick={() => { void openSnapshot(snapshot.id); }}
        >
          <span class="block text-[0.866667rem] font-medium">
            {formatVersionDate(snapshot.created_time)} · {formatVersionHour(snapshot.created_time)}
          </span>
          <span class="mt-0.5 block truncate text-[0.733333rem] text-muted-foreground">
            {snapshotAuthorName(snapshot)}
          </span>
        </button>
      {/each}
    {/if}
  {/snippet}

  {#snippet footerActions()}
    <button
      type="button"
      class="flex h-9 shrink-0 items-center justify-center gap-1.5 rounded-md border border-border bg-card px-3 text-[0.8rem] font-medium text-foreground hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50 dark:bg-transparent"
      disabled={!selectedSnapshot || selectedSnapshot.block_count === 0 || copying || restoring}
      onclick={() => { void copySelectedBlocks(); }}
    >
      <Copy class="size-3.5" />
      <span>{t("notes.copyPageHistoryBlocks")}</span>
    </button>
  {/snippet}
</NotesVersionHistoryModalShell>

{#if restoreSnapshotId && restoreSnapshot}
  <ConfirmDialog
    title={t("notes.restorePageHistoryConfirmTitle")}
    message={t(
      "notes.restorePageHistoryConfirmMessage",
      formatVersionTimestamp(restoreSnapshot.created_time),
    )}
    confirmLabel={t("notes.restorePageHistoryConfirm")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => { void confirmRestore(); }}
    onCancel={() => { restoreSnapshotId = null; }}
  />
{/if}
