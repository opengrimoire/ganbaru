<script lang="ts">
  import Select from "$lib/components/ui/Select.svelte";
  import { saveNotesDataSourceCsv } from "$lib/api/notes";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    roundTripWarningCount,
    toRoundTripDiagnosticItem,
  } from "$lib/notes/round-trip-diagnostics";
  import type {
    NotesDataSourceCsvExportDiagnostic,
    NotesDataSourceCsvExportSaveResult,
    NotesDataSourceCsvExportScope,
  } from "$lib/notes/types";
  import Download from "@lucide/svelte/icons/download";
  import NotesRoundTripDiagnostics from "$lib/components/notes/NotesRoundTripDiagnostics.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  let {
    dataSourceId,
    databaseId = null,
    viewId = null,
    disabled = false,
  }: {
    dataSourceId: string;
    databaseId?: string | null;
    viewId?: string | null;
    disabled?: boolean;
  } = $props();

  const { t } = getLocalization();
  let scope = $state<NotesDataSourceCsvExportScope>("view");
  let exporting = $state(false);
  let error = $state<string | null>(null);
  let result = $state<NotesDataSourceCsvExportSaveResult | null>(null);

  const roundTripDiagnostics = $derived(
    result?.export?.diagnostics.map((diagnostic) =>
      toRoundTripDiagnosticItem({
        code: diagnostic.code,
        severity: diagnostic.severity,
        message: diagnostic.message,
        sourceLabel: csvExportSourceLabel(diagnostic),
      }),
    ) ?? [],
  );
  const warningCount = $derived(roundTripWarningCount(roundTripDiagnostics));
  const roundTripCounts = $derived(
    result?.export
      ? [
          {
            id: "rows",
            label: t("notes.roundTripCountRows"),
            value: result.export.exported_row_count,
          },
          {
            id: "properties",
            label: t("notes.roundTripCountProperties"),
            value: result.export.exported_property_count,
          },
          {
            id: "warnings",
            label: t("notes.roundTripCountWarnings"),
            value: warningCount,
          },
        ]
      : [],
  );

  function csvExportSourceLabel(diagnostic: NotesDataSourceCsvExportDiagnostic): string | null {
    if (diagnostic.property_name) {
      return t("notes.roundTripSourceProperty", diagnostic.property_name);
    }
    if (diagnostic.property_id) return t("notes.roundTripSourceProperty", diagnostic.property_id);
    return null;
  }

  async function runExport(): Promise<void> {
    if (disabled || exporting) return;
    exporting = true;
    error = null;
    result = null;
    try {
      result = await saveNotesDataSourceCsv(dataSourceId, {
        database_id: databaseId,
        view_id: viewId,
        scope,
      });
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      exporting = false;
    }
  }
</script>

<details class="rounded-md border border-border p-2 text-[0.8rem]">
  <summary class="cursor-pointer text-foreground">{t("notes.databaseCsvExportTitle")}</summary>
  <div class="mt-2 grid min-w-0 gap-2">
    <div class="flex min-w-0 flex-wrap items-center gap-2">
      <div class="min-w-0 text-muted-foreground">
        <span class="mb-1 block">{t("notes.databaseCsvExportScope")}</span>
        <Select textSize="collection"
          inline
          appearance="quiet"
          contentAlign="start"
          class="w-full min-w-0"
          ariaLabel={t("notes.databaseCsvExportScope")}
          value={String(scope ?? "")}
          disabled={disabled || exporting}
          options={[{ value: "view", label: t("notes.databaseCsvExportScopeView") },
            { value: "all", label: t("notes.databaseCsvExportScopeAll") }]}
          onChange={(nextValue) => {
            scope = nextValue === "all" ? "all" : "view";
            result = null;
          }}
          triggerProps={{ "onkeydown": (event) => event.stopPropagation() }}
        />
      </div>
      <button
        type="button"
        class="inline-flex h-8 items-center gap-1 rounded-md bg-primary px-2 text-primary-foreground disabled:pointer-events-none disabled:opacity-50"
        disabled={disabled || exporting}
        onclick={() => {
          void runExport();
        }}
      >
        <Download class="size-3.5" aria-hidden="true" />
        <span>{exporting ? t("notes.databaseCsvExportExporting") : t("notes.databaseCsvExportSubmit")}</span>
      </button>
    </div>

    {#if error}
      <Notice message={t("notes.databaseCsvExportFailed", error)} copy={error} />
    {/if}

    {#if result}
      <div class="grid gap-1 text-muted-foreground">
        <p>
          {#if result.saved && result.export}
            {t(
              "notes.databaseCsvExportComplete",
              result.export.exported_row_count,
              result.export.exported_property_count,
              warningCount,
            )}
          {:else}
            {t("notes.databaseCsvExportCanceled")}
          {/if}
        </p>
        {#if result.export}
          <NotesRoundTripDiagnostics counts={roundTripCounts} diagnostics={roundTripDiagnostics} />
        {/if}
      </div>
    {/if}
  </div>
</details>
