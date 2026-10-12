<script lang="ts">
  import { importNotesDataSourceCsv } from "$lib/api/notes";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    browserFileExceedsLimit,
    NOTES_DATABASE_CSV_MAX_BYTES,
    NOTES_DATABASE_CSV_MAX_KIBIBYTES,
  } from "$lib/browser-file-policy";
  import {
    roundTripWarningCount,
    toRoundTripDiagnosticItem,
  } from "$lib/notes/round-trip-diagnostics";
  import type {
    NotesDataSourceCsvImportDiagnostic,
    NotesDataSourceCsvImportResult,
  } from "$lib/notes/types";
  import Check from "@lucide/svelte/icons/check";
  import Eye from "@lucide/svelte/icons/eye";
  import Upload from "@lucide/svelte/icons/upload";
  import NotesRoundTripDiagnostics from "$lib/components/notes/NotesRoundTripDiagnostics.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  let {
    dataSourceId,
    disabled = false,
    onImported,
  }: {
    dataSourceId: string;
    disabled?: boolean;
    onImported: () => Promise<void> | void;
  } = $props();

  const { t } = getLocalization();
  let csvText = $state("");
  let hasHeader = $state(true);
  let runningAction = $state<"preview" | "import" | null>(null);
  let error = $state<string | null>(null);
  let result = $state<NotesDataSourceCsvImportResult | null>(null);

  const canRun = $derived(!disabled && !runningAction && csvText.trim().length > 0);
  const roundTripDiagnostics = $derived(
    result?.diagnostics.map((diagnostic) =>
      toRoundTripDiagnosticItem({
        code: diagnostic.code,
        severity: diagnostic.severity,
        message: diagnostic.message,
        sourceLabel: csvImportSourceLabel(diagnostic),
      }),
    ) ?? [],
  );
  const warningCount = $derived(roundTripWarningCount(roundTripDiagnostics));
  const roundTripCounts = $derived(
    result
      ? [
          {
            id: "rows",
            label: t("notes.roundTripCountRows"),
            value: result.total_row_count,
          },
          {
            id: "valid-rows",
            label: t("notes.roundTripCountValidRows"),
            value: result.valid_row_count,
          },
          {
            id: "skipped-rows",
            label: t("notes.roundTripCountSkippedRows"),
            value: result.skipped_row_count,
          },
          {
            id: "imported-rows",
            label: t("notes.roundTripCountImportedRows"),
            value: result.imported_row_count,
          },
          {
            id: "warnings",
            label: t("notes.roundTripCountWarnings"),
            value: warningCount,
          },
        ]
      : [],
  );

  function csvImportSourceLabel(diagnostic: NotesDataSourceCsvImportDiagnostic): string | null {
    if (diagnostic.row_number) return t("notes.roundTripSourceCsvRow", diagnostic.row_number);
    if (diagnostic.column_name) return diagnostic.column_name;
    if (diagnostic.property_id) return t("notes.roundTripSourceProperty", diagnostic.property_id);
    return null;
  }

  async function loadFile(event: Event): Promise<void> {
    const input = event.currentTarget;
    if (!(input instanceof HTMLInputElement)) return;
    const file = input.files?.[0] ?? null;
    if (!file) return;
    input.value = "";
    error = null;
    result = null;
    csvText = "";
    if (browserFileExceedsLimit(file, NOTES_DATABASE_CSV_MAX_BYTES)) {
      error = t("notes.databaseCsvImportTooLarge", NOTES_DATABASE_CSV_MAX_KIBIBYTES);
      return;
    }
    try {
      csvText = await file.text();
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    }
  }

  async function runImport(dryRun: boolean): Promise<void> {
    if (!canRun) return;
    runningAction = dryRun ? "preview" : "import";
    error = null;
    try {
      const next = await importNotesDataSourceCsv(dataSourceId, {
        csv: csvText,
        has_header: hasHeader,
        dry_run: dryRun,
      });
      result = next;
      if (!dryRun && next.imported_row_count > 0) {
        await onImported();
      }
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      runningAction = null;
    }
  }
</script>

<details class="rounded-md border border-border p-2 text-[0.8rem]">
  <summary class="cursor-pointer text-foreground">{t("notes.databaseCsvImportTitle")}</summary>
  <div class="mt-2 grid min-w-0 gap-2">
    <div class="flex min-w-0 flex-wrap items-center gap-2">
      <label class="inline-flex h-8 cursor-pointer items-center gap-1 rounded-md px-2 hover:bg-accent">
        <Upload class="size-3.5" aria-hidden="true" />
        <span>{t("notes.databaseCsvImportFile")}</span>
        <input
          class="sr-only"
          type="file"
          accept=".csv,text/csv"
          disabled={disabled || runningAction !== null}
          onchange={(event) => {
            void loadFile(event);
          }}
        />
      </label>
      <label class="inline-flex min-w-0 items-center gap-2 text-muted-foreground">
        <Checkbox
          checked={hasHeader}
          disabled={disabled || runningAction !== null}
          onChange={(checked) => {
            hasHeader = checked;
            result = null;
          }}
          onkeydown={(event) => event.stopPropagation()}
        />
        <span>{t("notes.databaseCsvImportHasHeader")}</span>
      </label>
    </div>

    <label class="min-w-0 text-muted-foreground">
      <span class="mb-1 block">{t("notes.databaseCsvImportText")}</span>
      <textarea
        class="field min-h-28 w-full min-w-0 resize-y font-mono text-foreground"
        value={csvText}
        placeholder={t("notes.databaseCsvImportPlaceholder")}
        disabled={disabled || runningAction !== null}
        oninput={(event) => {
          csvText = event.currentTarget.value;
          result = null;
        }}
        onkeydown={(event) => event.stopPropagation()}
      ></textarea>
    </label>

    <div class="flex min-w-0 flex-wrap items-center gap-1">
      <button
        type="button"
        class="inline-flex h-8 items-center gap-1 rounded-md px-2 hover:bg-accent disabled:pointer-events-none disabled:opacity-50"
        disabled={!canRun}
        onclick={() => {
          void runImport(true);
        }}
      >
        <Eye class="size-3.5" aria-hidden="true" />
        <span>{runningAction === "preview" ? t("notes.databaseCsvImportPreviewing") : t("notes.databaseCsvImportPreview")}</span>
      </button>
      <button
        type="button"
        class="inline-flex h-8 items-center gap-1 rounded-md bg-primary px-2 text-primary-foreground disabled:pointer-events-none disabled:opacity-50"
        disabled={!canRun || (result !== null && result.valid_row_count === 0)}
        onclick={() => {
          void runImport(false);
        }}
      >
        <Check class="size-3.5" aria-hidden="true" />
        <span>{runningAction === "import" ? t("notes.databaseCsvImportImporting") : t("notes.databaseCsvImportImport")}</span>
      </button>
    </div>

    {#if error}
      <Notice message={t("notes.databaseCsvImportFailed", error)} copy={error} />
    {/if}

    {#if result}
      <div class="grid gap-2">
        <p class="text-muted-foreground">
          {result.imported_row_count > 0
            ? t("notes.databaseCsvImportComplete", result.imported_row_count)
            : t("notes.databaseCsvImportSummary", result.valid_row_count, result.total_row_count, result.skipped_row_count)}
        </p>
        <div class="grid gap-1">
          <h4 class="text-[0.8rem] font-medium text-foreground">{t("notes.databaseCsvImportColumns")}</h4>
          <ul class="grid max-h-32 gap-1 overflow-auto text-muted-foreground" use:scrollEdgeFadeAction>
            {#each result.columns as column (column.source_index)}
              <li class="flex min-w-0 items-center gap-2">
                <span class="min-w-0 flex-1 truncate">
                  {column.source_name}
                  {#if column.property_name}
                    -> {column.property_name}
                  {:else}
                    -> {t("notes.databaseCsvImportColumnSkipped")}
                  {/if}
                </span>
                {#if column.warning}
                  <span class="shrink-0 text-muted-foreground">{column.warning}</span>
                {/if}
              </li>
            {/each}
          </ul>
        </div>
        <NotesRoundTripDiagnostics counts={roundTripCounts} diagnostics={roundTripDiagnostics} />
      </div>
    {:else if !csvText.trim()}
      <p class="text-muted-foreground">{t("notes.databaseCsvImportNoCsv")}</p>
    {/if}
  </div>
</details>
