<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import {
    roundTripWarningCount,
    toRoundTripDiagnosticItem,
  } from "$lib/notes/round-trip-diagnostics";
  import type { NotesNotionExportImportResult } from "$lib/notes/types";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import NotesImportDestinationRow from "./NotesImportDestinationRow.svelte";
  import NotesRoundTripDiagnostics from "$lib/components/notes/NotesRoundTripDiagnostics.svelte";
  import NotesTransferFieldRow from "./NotesTransferFieldRow.svelte";

  let {
    onImport,
    onCancel,
    embedded = false,
    description = null,
  }: {
    onImport: (input: {
      sourceWorkspaceId: string | null;
      keepExternalFileReferences: boolean;
      copyLocalFileReferences: boolean;
      importMarkdown: boolean;
      importHtml: boolean;
      importCsv: boolean;
      projectId: string | null;
    }) => Promise<NotesNotionExportImportResult | null>;
    onCancel: () => void;
    embedded?: boolean;
    description?: string | null;
  } = $props();

  const { t } = getLocalization();
  let sourceWorkspaceId = $state("");
  let projectId = $state<string | null>(null);
  let keepExternalFileReferences = $state(false);
  let copyLocalFileReferences = $state(false);
  let importMarkdown = $state(true);
  let importHtml = $state(true);
  let importCsv = $state(true);
  let importing = $state(false);
  let error = $state<string | null>(null);
  let result = $state<NotesNotionExportImportResult | null>(null);

  const canImport = $derived(
    (importMarkdown || importHtml || importCsv)
      && projectId !== null
      && !importing,
  );

  const roundTripDiagnostics = $derived(
    result?.diagnostics.map((diagnostic) =>
      toRoundTripDiagnosticItem({
        code: diagnostic.code,
        severity: diagnostic.severity,
        message: diagnostic.message,
        sourceLabel: diagnostic.source_path
          ? t("notes.roundTripSourcePath", diagnostic.source_path)
          : null,
      }),
    ) ?? [],
  );
  const warningCount = $derived(roundTripWarningCount(roundTripDiagnostics));
  const roundTripCounts = $derived(
    result
      ? [
          {
            id: "pages",
            label: t("notes.roundTripCountPages"),
            value: result.imported_page_count,
          },
          {
            id: "blocks",
            label: t("notes.roundTripCountBlocks"),
            value: result.imported_block_count,
          },
          {
            id: "data-sources",
            label: t("notes.roundTripCountDataSources"),
            value: result.imported_data_source_count,
          },
          {
            id: "files",
            label: t("notes.roundTripCountFiles"),
            value: result.imported_file_count,
          },
          {
            id: "skipped-files",
            label: t("notes.roundTripCountSkippedFiles"),
            value: result.skipped_file_count,
          },
          {
            id: "unsupported-blocks",
            label: t("notes.roundTripCountUnsupportedBlocks"),
            value: result.unsupported_block_count,
          },
          {
            id: "warnings",
            label: t("notes.roundTripCountWarnings"),
            value: warningCount,
          },
        ]
      : [],
  );

  async function submit(): Promise<void> {
    if (!canImport) return;
    importing = true;
    error = null;
    result = null;
    try {
      result = await onImport({
        sourceWorkspaceId: sourceWorkspaceId.trim() || null,
        keepExternalFileReferences,
        copyLocalFileReferences,
        importMarkdown,
        importHtml,
        importCsv,
        projectId,
      });
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      importing = false;
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
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class={embedded
    ? "flex min-h-0 flex-col"
    : "fixed inset-0 z-90 flex items-center justify-center p-3"}
  onclick={(event) => {
    if (!embedded) {
      event.stopPropagation();
      onCancel();
    }
  }}
>
  {#if !embedded}
    <div class="surface-backdrop absolute inset-0"></div>
  {/if}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class={embedded
      ? "flex min-h-0 flex-col text-card-foreground outline-none"
      : "relative z-10 flex max-h-[min(92vh,44rem)] w-[min(42rem,100%)] flex-col surface-dialog outline-none"}
    role={embedded ? "region" : "dialog"}
    aria-modal={embedded ? undefined : "true"}
    aria-label={t("notes.notionExportImportDialogTitle")}
    tabindex="-1"
    onclick={(event) => event.stopPropagation()}
    onkeydown={embedded ? undefined : handleKeydown}
  >
    <div class="shrink-0 border-b border-border/70 px-4 py-3">
      <h2 class="text-[1rem] font-semibold text-foreground">
        {t("notes.notionExportImportDialogTitle")}
      </h2>
      {#if description}
        <p class="mt-1 max-w-2xl text-[0.866667rem] text-muted-foreground">{description}</p>
      {/if}
    </div>

    <div class={embedded ? "px-4 py-3" : "min-h-0 flex-1 overflow-auto px-4 py-3"} use:scrollEdgeFadeAction>
      <div class="grid gap-3">
        <NotesImportDestinationRow bind:projectId />

        <NotesTransferFieldRow
          label={t("notes.notionExportImportWorkspaceLabel")}
          description={t("notes.notionExportImportWorkspaceDescription")}
          forId="notes-notion-export-workspace"
        >
          <input
            id="notes-notion-export-workspace"
            class="field w-72 max-w-full text-panel text-foreground max-[560px]:w-full"
            bind:value={sourceWorkspaceId}
            spellcheck="false"
          />
        </NotesTransferFieldRow>

        <div class="grid gap-2">
          <NotesTransferFieldRow
            label={t("notes.notionExportImportMarkdown")}
            description={t("notes.notionExportImportMarkdownDescription")}
          >
            <Checkbox
              bind:checked={importMarkdown}
              label={t("notes.notionExportImportMarkdown")}
            />
          </NotesTransferFieldRow>
          <NotesTransferFieldRow
            label={t("notes.notionExportImportHtml")}
            description={t("notes.notionExportImportHtmlDescription")}
          >
            <Checkbox
              bind:checked={importHtml}
              label={t("notes.notionExportImportHtml")}
            />
          </NotesTransferFieldRow>
          <NotesTransferFieldRow
            label={t("notes.notionExportImportCsv")}
            description={t("notes.notionExportImportCsvDescription")}
          >
            <Checkbox
              bind:checked={importCsv}
              label={t("notes.notionExportImportCsv")}
            />
          </NotesTransferFieldRow>
          <NotesTransferFieldRow
            label={t("notes.notionExportImportCopyFiles")}
            description={t("notes.notionExportImportCopyFilesDescription")}
          >
            <Checkbox
              bind:checked={copyLocalFileReferences}
              label={t("notes.notionExportImportCopyFiles")}
            />
          </NotesTransferFieldRow>
          <NotesTransferFieldRow
            label={t("notes.notionExportImportKeepExternalFiles")}
            description={t("notes.notionExportImportKeepExternalFilesDescription")}
          >
            <Checkbox
              bind:checked={keepExternalFileReferences}
              label={t("notes.notionExportImportKeepExternalFiles")}
            />
          </NotesTransferFieldRow>
        </div>

        {#if error}
          <Notice class="text-[0.8rem]" message={t("notes.notionExportImportFailed", error)} copy={error} />
        {/if}

        {#if result}
          <div class="rounded-md border border-border/70 px-3 py-2">
            <div class="text-[0.8rem] font-medium text-foreground">
              {t(
                "notes.notionExportImportComplete",
                result.imported_page_count,
                result.imported_data_source_count,
                result.imported_file_count,
                warningCount,
              )}
            </div>
            <div class="mt-2">
              <NotesRoundTripDiagnostics counts={roundTripCounts} diagnostics={roundTripDiagnostics} />
            </div>
          </div>
        {/if}
      </div>
    </div>

    <div class="flex shrink-0 flex-wrap justify-end gap-2 border-t border-border/70 px-4 py-3">
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
        disabled={!canImport}
        onclick={() => {
          void submit();
        }}
      >
        {importing ? t("notes.notionExportImportImporting") : t("notes.notionExportImportSubmit")}
      </button>
    </div>
  </div>
</div>
