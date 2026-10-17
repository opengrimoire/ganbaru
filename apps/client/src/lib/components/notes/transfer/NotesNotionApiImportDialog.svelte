<script lang="ts">
  import { onMount, tick } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { toRoundTripDiagnosticItem } from "$lib/notes/round-trip-diagnostics";
  import type { NotesNotionApiImportResult } from "$lib/notes/types";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import NotesImportDestinationRow from "./NotesImportDestinationRow.svelte";
  import NotesRoundTripDiagnostics from "$lib/components/notes/NotesRoundTripDiagnostics.svelte";
  import NotesTransferFieldRow from "./NotesTransferFieldRow.svelte";

  const NOTION_AUTHORIZATION_DOCS_URL = "https://developers.notion.com/guides/get-started/authorization";

  let {
    onImport,
    onCancel,
    embedded = false,
    description = null,
  }: {
    onImport: (input: {
      integrationToken: string;
      sourceWorkspaceId: string | null;
      pageIds: string[];
      dataSourceIds: string[];
      includeComments: boolean;
      includeUsers: boolean;
      keepExternalFileReferences: boolean;
      projectId: string | null;
    }) => Promise<NotesNotionApiImportResult>;
    onCancel: () => void;
    embedded?: boolean;
    description?: string | null;
  } = $props();

  const { t } = getLocalization();
  let integrationToken = $state("");
  let sourceWorkspaceId = $state("");
  let pageIdsText = $state("");
  let dataSourceIdsText = $state("");
  let projectId = $state<string | null>(null);
  let includeComments = $state(true);
  let includeUsers = $state(true);
  let keepExternalFileReferences = $state(false);
  let importing = $state(false);
  let error = $state<string | null>(null);
  let result = $state<NotesNotionApiImportResult | null>(null);
  let tokenInputEl = $state<HTMLInputElement | null>(null);

  const pageIds = $derived(splitSourceIds(pageIdsText));
  const dataSourceIds = $derived(splitSourceIds(dataSourceIdsText));
  const canImport = $derived(
    integrationToken.trim().length > 0
      && (pageIds.length > 0 || dataSourceIds.length > 0)
      && projectId !== null
      && !importing,
  );
  const roundTripDiagnostics = $derived(
    result?.diagnostics.map((diagnostic) =>
      toRoundTripDiagnosticItem({
        code: diagnostic.code,
        severity: diagnostic.severity,
        message: diagnostic.message,
        sourceLabel: diagnostic.source_object_id
          ? t("notes.roundTripSourceObject", diagnostic.source_object_id)
          : null,
      }),
    ) ?? [],
  );
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
            id: "comments",
            label: t("notes.roundTripCountComments"),
            value: result.imported_comment_count,
          },
          {
            id: "files",
            label: t("notes.roundTripCountFiles"),
            value: result.imported_file_count,
          },
          {
            id: "users",
            label: t("notes.roundTripCountUsers"),
            value: result.imported_user_count,
          },
          {
            id: "unsupported-blocks",
            label: t("notes.roundTripCountUnsupportedBlocks"),
            value: result.unsupported_block_count,
          },
          {
            id: "requests",
            label: t("notes.roundTripCountRequests"),
            value: result.request_count,
          },
          {
            id: "retries",
            label: t("notes.roundTripCountRetries"),
            value: result.retry_count,
          },
          {
            id: "rate-limits",
            label: t("notes.roundTripCountRateLimits"),
            value: result.rate_limit_count,
          },
        ]
      : [],
  );

  onMount(() => {
    void tick().then(() => {
      tokenInputEl?.focus();
    });
  });

  async function submit(): Promise<void> {
    if (!canImport) return;
    importing = true;
    error = null;
    result = null;
    try {
      result = await onImport({
        integrationToken,
        sourceWorkspaceId: sourceWorkspaceId.trim() || null,
        pageIds,
        dataSourceIds,
        includeComments,
        includeUsers,
        keepExternalFileReferences,
        projectId,
      });
      integrationToken = "";
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      importing = false;
    }
  }

  function splitSourceIds(value: string): string[] {
    const seen = new Set<string>();
    return value
      .split(/[\n,]/u)
      .map((item) => item.trim())
      .filter((item) => item.length > 0)
      .filter((item) => {
        if (seen.has(item)) return false;
        seen.add(item);
        return true;
      });
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
      : "relative z-10 flex max-h-[min(92vh,46rem)] w-[min(44rem,100%)] flex-col surface-dialog outline-none"}
    role={embedded ? "region" : "dialog"}
    aria-modal={embedded ? undefined : "true"}
    aria-label={t("notes.notionApiImportDialogTitle")}
    tabindex="-1"
    onclick={(event) => event.stopPropagation()}
    onkeydown={embedded ? undefined : handleKeydown}
  >
    <div class="shrink-0 border-b border-border/70 px-4 py-3">
      <h2 class="text-[1rem] font-semibold text-foreground">
        {t("notes.notionApiImportDialogTitle")}
      </h2>
      {#if description}
        <p class="mt-1 max-w-2xl text-[0.866667rem] text-muted-foreground">{description}</p>
      {/if}
    </div>

    <div class={embedded ? "px-4 py-3" : "min-h-0 flex-1 overflow-auto px-4 py-3"} use:scrollEdgeFadeAction>
      <div class="grid gap-3">
        <NotesImportDestinationRow bind:projectId />

        <NotesTransferFieldRow
          label={t("notes.notionApiImportTokenLabel")}
          description={t("notes.notionApiImportTokenDescription")}
          forId="notes-notion-api-token"
          labelUrl={NOTION_AUTHORIZATION_DOCS_URL}
        >
          <input
            id="notes-notion-api-token"
            aria-label={t("notes.notionApiImportTokenLabel")}
            bind:this={tokenInputEl}
            class="field w-72 max-w-full text-panel text-foreground max-[560px]:w-full"
            type="password"
            bind:value={integrationToken}
            autocomplete="off"
            spellcheck="false"
          />
        </NotesTransferFieldRow>

        <NotesTransferFieldRow
          label={t("notes.notionApiImportWorkspaceLabel")}
          description={t("notes.notionApiImportWorkspaceDescription")}
          forId="notes-notion-api-workspace"
        >
          <input
            id="notes-notion-api-workspace"
            class="field w-72 max-w-full text-panel text-foreground max-[560px]:w-full"
            bind:value={sourceWorkspaceId}
            spellcheck="false"
          />
        </NotesTransferFieldRow>

        <div class="grid gap-3 md:grid-cols-2">
          <NotesTransferFieldRow
            label={t("notes.notionApiImportPagesLabel")}
            description={t("notes.notionApiImportPagesDescription")}
            forId="notes-notion-api-page-ids"
            wide
          >
            <textarea
              id="notes-notion-api-page-ids"
              class="field min-h-28 w-full resize-y py-2 font-mono text-panel leading-relaxed text-foreground"
              bind:value={pageIdsText}
              spellcheck="false"
            ></textarea>
          </NotesTransferFieldRow>

          <NotesTransferFieldRow
            label={t("notes.notionApiImportDataSourcesLabel")}
            description={t("notes.notionApiImportDataSourcesDescription")}
            forId="notes-notion-api-data-source-ids"
            wide
          >
            <textarea
              id="notes-notion-api-data-source-ids"
              class="field min-h-28 w-full resize-y py-2 font-mono text-panel leading-relaxed text-foreground"
              bind:value={dataSourceIdsText}
              spellcheck="false"
            ></textarea>
          </NotesTransferFieldRow>
        </div>

        <div class="grid gap-2">
          <NotesTransferFieldRow
            label={t("notes.notionApiImportIncludeComments")}
            description={t("notes.notionApiImportIncludeCommentsDescription")}
          >
            <Checkbox
              bind:checked={includeComments}
              label={t("notes.notionApiImportIncludeComments")}
            />
          </NotesTransferFieldRow>
          <NotesTransferFieldRow
            label={t("notes.notionApiImportIncludeUsers")}
            description={t("notes.notionApiImportIncludeUsersDescription")}
          >
            <Checkbox
              bind:checked={includeUsers}
              label={t("notes.notionApiImportIncludeUsers")}
            />
          </NotesTransferFieldRow>
          <NotesTransferFieldRow
            label={t("notes.notionApiImportKeepExternalFiles")}
            description={t("notes.notionApiImportKeepExternalFilesDescription")}
          >
            <Checkbox
              bind:checked={keepExternalFileReferences}
              label={t("notes.notionApiImportKeepExternalFiles")}
            />
          </NotesTransferFieldRow>
        </div>

        {#if error}
          <Notice class="text-[0.8rem]" message={t("notes.notionApiImportFailed", error)} copy={error} />
        {/if}

        {#if result}
          <div class="rounded-md border border-border/70 px-3 py-2">
            <div class="text-[0.8rem] font-medium text-foreground">
              {t(
                "notes.notionApiImportComplete",
                result.imported_page_count,
                result.imported_block_count,
                result.imported_comment_count,
              )}
            </div>
            <div class="mt-1 text-[0.733333rem] text-muted-foreground">
              {t(
                "notes.notionApiImportRequests",
                result.request_count,
                result.retry_count,
                result.rate_limit_count,
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
        {importing ? t("notes.notionApiImportImporting") : t("notes.notionApiImportSubmit")}
      </button>
    </div>
  </div>
</div>
