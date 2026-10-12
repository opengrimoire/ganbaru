<script lang="ts">
  import { onMount, tick } from "svelte";
  import Upload from "@lucide/svelte/icons/upload";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import {
    roundTripWarningCount,
    toRoundTripDiagnosticItem,
  } from "$lib/notes/round-trip-diagnostics";
  import type { NotesHtmlImportResult } from "$lib/notes/types";
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
      html: string;
      title: string | null;
      sourceName: string | null;
      keepExternalFileReferences: boolean;
      projectId: string | null;
    }) => Promise<NotesHtmlImportResult>;
    onCancel: () => void;
    embedded?: boolean;
    description?: string | null;
  } = $props();

  const { t } = getLocalization();
  let html = $state("");
  let title = $state("");
  let sourceName = $state("");
  let projectId = $state<string | null>(null);
  let keepExternalFileReferences = $state(false);
  let importing = $state(false);
  let error = $state<string | null>(null);
  let result = $state<NotesHtmlImportResult | null>(null);
  let dialogEl = $state<HTMLDivElement | null>(null);
  let textareaEl = $state<HTMLTextAreaElement | null>(null);
  let htmlFileInputEl = $state<HTMLInputElement | null>(null);

  const canImport = $derived(html.trim().length > 0 && projectId !== null && !importing);
  const roundTripDiagnostics = $derived(
    result?.diagnostics.map((diagnostic) =>
      toRoundTripDiagnosticItem({
        code: diagnostic.code,
        severity: diagnostic.severity,
        message: diagnostic.message,
        sourceLabel: diagnostic.line ? t("notes.roundTripSourceLine", diagnostic.line) : null,
      }),
    ) ?? [],
  );
  const warningCount = $derived(roundTripWarningCount(roundTripDiagnostics));
  const roundTripCounts = $derived(
    result
      ? [
          {
            id: "blocks",
            label: t("notes.roundTripCountBlocks"),
            value: result.imported_block_count,
          },
          {
            id: "warnings",
            label: t("notes.roundTripCountWarnings"),
            value: warningCount,
          },
        ]
      : [],
  );

  onMount(() => {
    void tick().then(() => {
      textareaEl?.focus();
    });
  });

  async function submit(): Promise<void> {
    if (!canImport) return;
    importing = true;
    error = null;
    result = null;
    try {
      result = await onImport({
        html,
        title: title.trim() || null,
        sourceName: sourceName.trim() || null,
        keepExternalFileReferences,
        projectId,
      });
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      importing = false;
    }
  }

  async function importHtmlFile(file: File): Promise<void> {
    error = null;
    result = null;
    try {
      html = await file.text();
      if (!sourceName.trim()) {
        sourceName = file.name;
      }
      await tick();
      textareaEl?.focus();
    } catch (caught) {
      error = t(
        "notes.htmlImportFileFailed",
        caught instanceof Error ? caught.message : String(caught),
      );
    }
  }

  function chooseHtmlFile(): void {
    htmlFileInputEl?.click();
  }

  function handleHtmlFileChange(event: Event): void {
    const input = event.currentTarget;
    if (!(input instanceof HTMLInputElement)) return;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    void importHtmlFile(file);
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

{#snippet dialogContent()}
  <div class="shrink-0 border-b border-border/70 px-4 py-3">
    <h2 class="text-[1rem] font-semibold text-foreground">
      {t("notes.htmlImportDialogTitle")}
    </h2>
    {#if description}
      <p class="mt-1 max-w-2xl text-[0.866667rem] text-muted-foreground">{description}</p>
    {/if}
  </div>

  <div class={embedded ? "px-4 py-3" : "min-h-0 flex-1 overflow-auto px-4 py-3"} use:scrollEdgeFadeAction>
    <div class="grid gap-3">
      <NotesImportDestinationRow bind:projectId />

      <NotesTransferFieldRow
        label={t("notes.htmlImportTitleLabel")}
        description={t("notes.htmlImportTitleDescription")}
        forId="notes-html-import-title"
      >
        <input
          id="notes-html-import-title"
          class="field w-72 max-w-full text-panel text-foreground max-[560px]:w-full"
          bind:value={title}
          placeholder={t("notes.htmlImportTitlePlaceholder")}
        />
      </NotesTransferFieldRow>

      <NotesTransferFieldRow
        label={t("notes.htmlImportSourceLabel")}
        description={t("notes.htmlImportSourceDescription")}
        forId="notes-html-import-source"
      >
        <input
          id="notes-html-import-source"
          class="field w-72 max-w-full text-panel text-foreground max-[560px]:w-full"
          bind:value={sourceName}
          placeholder={t("notes.htmlImportSourcePlaceholder")}
        />
      </NotesTransferFieldRow>

      <div class="grid gap-2 px-1 py-1">
        <div class="flex items-start justify-between gap-3 max-[560px]:flex-col max-[560px]:items-stretch max-[560px]:gap-2">
          <div class="min-w-0">
            <label for="notes-html-import-html" class="text-[0.866667rem] text-foreground">
              {t("notes.htmlImportHtmlLabel")}
            </label>
            <div class="mt-0.5 text-[0.8rem] leading-5 text-muted-foreground">
              {t("notes.htmlImportHtmlDescription")}
            </div>
          </div>
          <div class="flex shrink-0 items-center justify-end max-[560px]:justify-start">
            <input
              bind:this={htmlFileInputEl}
              class="sr-only"
              type="file"
              accept=".html,.htm,text/html"
              aria-label={t("notes.htmlImportFileInputLabel")}
              onchange={handleHtmlFileChange}
            />
            <button
              type="button"
              class="inline-flex h-7 items-center gap-1.5 rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50 dark:bg-transparent"
              disabled={importing}
              onclick={chooseHtmlFile}
            >
              <Upload size={13} strokeWidth={2} />
              <span>{t("notes.htmlImportFileButton")}</span>
            </button>
          </div>
        </div>
        <textarea
          id="notes-html-import-html"
          bind:this={textareaEl}
          class="field min-h-52 w-full resize-y py-2 font-mono text-panel leading-relaxed text-foreground"
          bind:value={html}
          spellcheck="false"
        ></textarea>
      </div>

      <NotesTransferFieldRow
        label={t("notes.htmlImportKeepExternalMedia")}
        description={t("notes.htmlImportKeepExternalMediaDescription")}
      >
        <Checkbox
          bind:checked={keepExternalFileReferences}
          label={t("notes.htmlImportKeepExternalMedia")}
        />
      </NotesTransferFieldRow>

      {#if error}
        <Notice class="text-[0.8rem]" message={t("notes.htmlImportFailed", error)} copy={error} />
      {/if}

      {#if result}
        <div class="rounded-md border border-border/70 px-3 py-2">
          <div class="text-[0.8rem] font-medium text-foreground">
            {t("notes.htmlImportComplete", result.imported_block_count, warningCount)}
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
      {importing ? t("notes.htmlImportImporting") : t("notes.htmlImportSubmit")}
    </button>
  </div>
{/snippet}

{#if embedded}
  <div
    bind:this={dialogEl}
    class="flex min-h-0 flex-col text-card-foreground outline-none"
    role="region"
    aria-label={t("notes.htmlImportDialogTitle")}
    tabindex="-1"
  >
    {@render dialogContent()}
  </div>
{:else}
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-90 flex items-center justify-center p-3"
    onclick={(event) => {
      event.stopPropagation();
      onCancel();
    }}
  >
    <div class="surface-backdrop absolute inset-0"></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      bind:this={dialogEl}
      class="relative z-10 flex max-h-[min(92vh,44rem)] w-[min(42rem,100%)] flex-col surface-dialog outline-none"
      role="dialog"
      aria-modal="true"
      aria-label={t("notes.htmlImportDialogTitle")}
      tabindex="-1"
      onclick={(event) => event.stopPropagation()}
      onkeydown={handleKeydown}
    >
      {@render dialogContent()}
    </div>
  </div>
{/if}
