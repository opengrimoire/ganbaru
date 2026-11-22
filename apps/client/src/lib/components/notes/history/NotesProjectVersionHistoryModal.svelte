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
  import { notesFailure, type NotesFailure } from "$lib/notes/failure";
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
  /** A request in flight, named by a token so a response for a superseded request is dropped. */
  type HistoryRequest<Kind extends string> =
    | { readonly phase: "idle" }
    | { readonly phase: "loading"; readonly kind: Kind; readonly token: symbol };

  let versions = $state<NotesProjectHistoryVersion[]>([]);
  let cursorTime = $state<string | null>(null);
  let cursorId = $state<string | null>(null);
  /** Starts loading so the empty state never shows before the first page arrives. */
  let versionsRequest = $state.raw<HistoryRequest<"reset" | "more">>({ phase: "loading", kind: "reset", token: Symbol("versions") });
  let error = $state<NotesFailure | null>(null);
  let selectedVersionId = $state<string | null>(null);
  let historicalTree = $state<NotesProjectHistoryTree | null>(null);
  let selectedPage = $state<NotesHistoricalPage | null>(null);
  /** The tree or page preview in flight; a newer preview or unmounting supersedes it. */
  let previewRequest = $state.raw<HistoryRequest<"tree" | "page">>({ phase: "idle" });
  let restorePlan = $state<NotesProjectHistoryRestorePlan | null>(null);
  let restoring = $state(false);
  /** Cleared once the modal closes, so a restore finishing afterwards does not close a history opened since. */
  let mounted = true;
  const loadingVersions = $derived(versionsRequest.phase === "loading" && versionsRequest.kind === "reset");
  const versionsBusy = $derived(versionsRequest.phase === "loading");
  const loadingPreview = $derived(previewRequest.phase === "loading");
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
    if (!reset && versionsRequest.phase === "loading") return;
    const token = Symbol("versions");
    versionsRequest = { phase: "loading", kind: reset ? "reset" : "more", token };
    const isCurrent = () => versionsRequest.phase === "loading" && versionsRequest.token === token;
    if (reset) error = null;
    try {
      const result = await listNotesProjectHistoryVersions({
        projectId,
        cursorTime: reset ? null : cursorTime,
        cursorId: reset ? null : cursorId,
        pageSize: 40,
      });
      if (!isCurrent()) return;
      versionsRequest = { phase: "idle" };
      versions = reset ? result.versions : [...versions, ...result.versions];
      cursorTime = result.nextCursorTime;
      cursorId = result.nextCursorId;
      if (reset && versions[0]) void selectVersion(versions[0].id, false);
    } catch (cause) {
      if (!isCurrent()) return;
      versionsRequest = { phase: "idle" };
      error = notesFailure(cause, t("notes.projectHistoryLoadFailed"));
    }
  }

  /** Starts a preview request that supersedes any preview in flight, returning a check for whether it is current. */
  function startPreviewRequest(kind: "tree" | "page"): () => boolean {
    const token = Symbol("preview");
    previewRequest = { phase: "loading", kind, token };
    return () => previewRequest.phase === "loading" && previewRequest.token === token;
  }

  async function selectVersion(versionId: string, shouldOpenCompactPreview = true): Promise<void> {
    selectedVersionId = versionId;
    historicalTree = null;
    selectedPage = null;
    const isCurrent = startPreviewRequest("tree");
    try {
      const tree = await loadNotesProjectHistoryTree(projectId, versionId);
      if (!isCurrent()) return;
      previewRequest = { phase: "idle" };
      if (selectedVersionId !== versionId) return;
      historicalTree = tree;
      if (!shouldOpenCompactPreview && compact) selectedVersionId = null;
    } catch (cause) {
      if (!isCurrent()) return;
      previewRequest = { phase: "idle" };
      error = notesFailure(cause, t("notes.projectHistoryLoadFailed"));
    }
  }

  async function openHistoricalPage(pageId: string): Promise<void> {
    if (!selectedVersionId) return;
    const versionId = selectedVersionId;
    const isCurrent = startPreviewRequest("page");
    try {
      const page = await loadNotesProjectHistoryPage(projectId, versionId, pageId);
      if (!isCurrent()) return;
      previewRequest = { phase: "idle" };
      if (versionId !== selectedVersionId) return;
      selectedPage = page;
    } catch (cause) {
      if (!isCurrent()) return;
      previewRequest = { phase: "idle" };
      error = notesFailure(cause, t("notes.projectHistoryLoadFailed"));
    }
  }

  async function requestRestore(): Promise<void> {
    const versionId = selectedVersionId;
    if (!versionId || restoring) return;
    restoring = true;
    error = null;
    try {
      const plan = await previewNotesProjectHistoryRestore(projectId, versionId);
      if (selectedVersionId === versionId) restorePlan = plan;
    } catch (cause) {
      error = notesFailure(cause, t("notes.projectHistoryRestoreFailed"));
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
      if (mounted) onClose();
    } catch (cause) {
      error = notesFailure(cause, t("notes.projectHistoryRestoreFailed"));
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
      mounted = false;
      versionsRequest = { phase: "idle" };
      previewRequest = { phase: "idle" };
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
  {versionsBusy}
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
          disabled={versionsBusy}
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
