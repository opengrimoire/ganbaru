<script lang="ts">
  import Select from "$lib/components/ui/Select.svelte";
  import { onMount, tick } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import type { Project } from "$lib/projects/types";
  import { getProjects } from "$lib/stores/projects.svelte";
  import {
    buildRoundTripNotesSourceHref,
    toRoundTripDiagnosticItem,
  } from "$lib/notes/round-trip-diagnostics";
  import type {
    NotesAgentBridgeExportDiagnostic,
    NotesAgentBridgeExportSaveResult,
  } from "$lib/notes/types";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import NotesRoundTripDiagnostics from "$lib/components/notes/NotesRoundTripDiagnostics.svelte";
  import NotesTransferFieldRow from "./NotesTransferFieldRow.svelte";

  let {
    pageTitle,
    onExport,
    onCancel,
  }: {
    pageTitle: string;
    onExport: (input: {
      includeDescendants: boolean;
      includeBacklinks: boolean;
      includeDatabaseViews: boolean;
      includeTaskContext: boolean;
      includePageComments: boolean;
      includeResolvedComments: boolean;
      projectIds: string[];
    }) => Promise<NotesAgentBridgeExportSaveResult>;
    onCancel: () => void;
  } = $props();

  const { t } = getLocalization();
  const projects = getProjects();
  let includeDescendants = $state(true);
  let includeBacklinks = $state(true);
  let includeDatabaseViews = $state(true);
  let includeTaskContext = $state(true);
  let includePageComments = $state(false);
  let includeResolvedComments = $state(false);
  let selectedProjectId = $state("");
  let exporting = $state(false);
  let error = $state<string | null>(null);
  let result = $state<NotesAgentBridgeExportSaveResult | null>(null);
  let dialogEl = $state<HTMLDivElement | null>(null);
  const activeProjects = $derived(
    [...projects.projects]
      .filter((project: Project) => project.status === "active")
      .sort((left, right) => left.name.localeCompare(right.name)),
  );
  const canExport = $derived(!exporting);
  const title = $derived(t("notes.agentBridgeExportDialogTitle", pageTitle));
  const roundTripDiagnostics = $derived(
    result?.export?.diagnostics.map((diagnostic) =>
      toRoundTripDiagnosticItem({
        code: diagnostic.code,
        severity: diagnostic.severity,
        message: diagnostic.message,
        sourceLabel: agentBridgeSourceLabel(diagnostic),
        sourceHref: agentBridgeSourceHref(diagnostic),
      }),
    ) ?? [],
  );
  const roundTripCounts = $derived(
    result?.export
      ? [
          {
            id: "pages",
            label: t("notes.roundTripCountPages"),
            value: result.export.exported_page_count,
          },
          {
            id: "projects",
            label: t("notes.roundTripCountProjects"),
            value: result.export.exported_project_count,
          },
          {
            id: "tasks",
            label: t("notes.roundTripCountTasks"),
            value: result.export.exported_task_count,
          },
          {
            id: "views",
            label: t("notes.roundTripCountDatabaseViews"),
            value: result.export.exported_database_view_count,
          },
          {
            id: "backlinks",
            label: t("notes.roundTripCountBacklinks"),
            value: result.export.exported_backlink_count,
          },
          {
            id: "warnings",
            label: t("notes.roundTripCountWarnings"),
            value: result.export.warning_count,
          },
        ]
      : [],
  );

  onMount(() => {
    void tick().then(() => {
      dialogEl?.focus();
    });
    void projects.ensureLoaded().catch(() => {
      // The project store records the failure in loadError, which this dialog renders.
    });
  });

  $effect(() => {
    if (!includePageComments) includeResolvedComments = false;
  });

  $effect(() => {
    if (selectedProjectId) return;
    const preferredProjectId = projects.selectedProjectId ?? "";
    if (activeProjects.some((project) => project.id === preferredProjectId)) {
      selectedProjectId = preferredProjectId;
    }
  });

  async function submit(): Promise<void> {
    if (!canExport) return;
    exporting = true;
    error = null;
    result = null;
    try {
      result = await onExport({
        includeDescendants,
        includeBacklinks,
        includeDatabaseViews,
        includeTaskContext,
        includePageComments,
        includeResolvedComments,
        projectIds: includeTaskContext && selectedProjectId ? [selectedProjectId] : [],
      });
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      exporting = false;
    }
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onCancel();
      return;
    }
    if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      void submit();
      return;
    }
    event.stopPropagation();
  }

  function agentBridgeSourceLabel(diagnostic: NotesAgentBridgeExportDiagnostic): string | null {
    if (diagnostic.source_type && diagnostic.source_id) {
      return t("notes.roundTripSourceTyped", diagnostic.source_type, diagnostic.source_id);
    }
    if (diagnostic.source_id) return t("notes.roundTripSourceObject", diagnostic.source_id);
    return null;
  }

  function agentBridgeSourceHref(diagnostic: NotesAgentBridgeExportDiagnostic): string | null {
    if (typeof window === "undefined" || diagnostic.source_type !== "page") return null;
    return buildRoundTripNotesSourceHref(window.location.href, {
      pageId: diagnostic.source_id,
    });
  }
</script>

<div class="fixed inset-0 z-90 flex items-center justify-center p-3">
  <button
    class="absolute inset-0 border-0 surface-backdrop p-0"
    type="button"
    aria-label={t("common.close")}
    onclick={onCancel}
  ></button>
  <div
    bind:this={dialogEl}
    class="relative z-10 flex max-h-[min(92vh,40rem)] w-[min(36rem,100%)] flex-col surface-dialog outline-none"
    role="dialog"
    aria-modal="true"
    aria-label={title}
    tabindex="-1"
    onkeydown={handleKeydown}
  >
    <div class="shrink-0 border-b border-border px-4 py-3">
      <h2 class="text-[1rem] font-semibold text-foreground">
        {title}
      </h2>
    </div>

    <div class="min-h-0 flex-1 overflow-auto px-4 py-3" use:scrollEdgeFadeAction>
      <div class="grid gap-2">
        <NotesTransferFieldRow
          label={t("notes.agentBridgeExportIncludeDescendants")}
          description={t("notes.agentBridgeExportIncludeDescendantsDescription")}
        >
          <Checkbox
            bind:checked={includeDescendants}
            label={t("notes.agentBridgeExportIncludeDescendants")}
          />
        </NotesTransferFieldRow>
        <NotesTransferFieldRow
          label={t("notes.agentBridgeExportIncludeBacklinks")}
          description={t("notes.agentBridgeExportIncludeBacklinksDescription")}
        >
          <Checkbox
            bind:checked={includeBacklinks}
            label={t("notes.agentBridgeExportIncludeBacklinks")}
          />
        </NotesTransferFieldRow>
        <NotesTransferFieldRow
          label={t("notes.agentBridgeExportIncludeDatabaseViews")}
          description={t("notes.agentBridgeExportIncludeDatabaseViewsDescription")}
        >
          <Checkbox
            bind:checked={includeDatabaseViews}
            label={t("notes.agentBridgeExportIncludeDatabaseViews")}
          />
        </NotesTransferFieldRow>
        <NotesTransferFieldRow
          label={t("notes.agentBridgeExportIncludeTaskContext")}
          description={t("notes.agentBridgeExportIncludeTaskContextDescription")}
        >
          <Checkbox
            bind:checked={includeTaskContext}
            label={t("notes.agentBridgeExportIncludeTaskContext")}
          />
        </NotesTransferFieldRow>
        <NotesTransferFieldRow
          label={t("notes.agentBridgeExportIncludePageComments")}
          description={t("notes.agentBridgeExportIncludePageCommentsDescription")}
        >
          <Checkbox
            bind:checked={includePageComments}
            label={t("notes.agentBridgeExportIncludePageComments")}
          />
        </NotesTransferFieldRow>
        <NotesTransferFieldRow
          label={t("notes.agentBridgeExportIncludeResolvedComments")}
          description={t("notes.agentBridgeExportIncludeResolvedCommentsDescription")}
        >
          <Checkbox
            bind:checked={includeResolvedComments}
            disabled={!includePageComments}
            label={t("notes.agentBridgeExportIncludeResolvedComments")}
          />
        </NotesTransferFieldRow>

        <NotesTransferFieldRow
          label={t("notes.agentBridgeExportProjectLabel")}
          description={t("notes.agentBridgeExportProjectDescription")}
          forId="notes-agent-bridge-project"
        >
          <Select
            inline
            appearance="quiet"
            contentAlign="start"
            class="w-full min-w-0"
            ariaLabel={t("notes.agentBridgeExportProjectLabel")}
            value={String(selectedProjectId ?? "")}
            disabled={!includeTaskContext || projects.loading}
            options={[{ value: "", label: t("notes.agentBridgeExportNoProject") },
              ...(activeProjects).map((project) => ({ value: String(project.id), label: String(project.name) }))]}
            onChange={(nextValue) => { selectedProjectId = nextValue; }}
            triggerProps={{ "id": "notes-agent-bridge-project" }}
          />
        </NotesTransferFieldRow>

        {#if projects.loading}
          <div class="rounded-md border border-border/70 px-3 py-2 text-[0.8rem] text-muted-foreground">
            {t("notes.agentBridgeExportProjectLoading")}
          </div>
        {:else if projects.loadError}
          <Notice class="text-[0.8rem]" message={t("notes.agentBridgeExportProjectLoadFailed", projects.loadError)} copy={projects.loadError} onRetry={() => projects.load()} />
        {/if}

        {#if error}
          <Notice class="text-[0.8rem]" message={t("notes.agentBridgeExportFailed", error)} copy={error} />
        {/if}

        {#if result}
          <div class="rounded-md border border-border/70 px-3 py-2">
            <div class="text-[0.8rem] font-medium text-foreground">
              {#if result.saved && result.export}
                {t(
                  "notes.agentBridgeExportComplete",
                  result.export.exported_page_count,
                  result.export.exported_project_count,
                  result.export.exported_task_count,
                  result.export.exported_database_view_count,
                  result.export.exported_backlink_count,
                  result.export.warning_count,
                )}
              {:else}
                {t("notes.agentBridgeExportCanceled")}
              {/if}
            </div>
            {#if result.export}
              <div class="mt-2">
                <NotesRoundTripDiagnostics counts={roundTripCounts} diagnostics={roundTripDiagnostics} />
              </div>
            {/if}
          </div>
        {/if}
      </div>
    </div>

    <div class="flex shrink-0 flex-wrap justify-end gap-2 border-t border-border px-4 py-3">
      <button
        class="rounded-md border border-border bg-card px-3 py-1.5 text-[0.866667rem] font-medium text-foreground hover:bg-accent"
        type="button"
        onclick={onCancel}
      >
        {t("common.close")}
      </button>
      <button
        class="rounded-md bg-primary px-3 py-1.5 text-[0.866667rem] font-medium text-primary-foreground hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-45"
        type="button"
        disabled={!canExport}
        onclick={() => {
          void submit();
        }}
      >
        {exporting ? t("notes.agentBridgeExportExporting") : t("notes.agentBridgeExportSubmit")}
      </button>
    </div>
  </div>
</div>
