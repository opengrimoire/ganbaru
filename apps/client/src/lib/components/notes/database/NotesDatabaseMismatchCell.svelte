<script lang="ts">
  import { previewNotesMismatchedValue, settleNotesMismatchedValue } from "$lib/api/notes/conflicts";
  import SyncConflictChoice from "$lib/components/sync/SyncConflictChoice.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { portal } from "$lib/utils/portal";
  import { notesStoredPropertyTypeLabel } from "./property-kinds";

  /**
   * A row value stored under another type than its property, written on another device while the type changed. It
   * offers converting the value to the property type, warning first when that drops data, or discarding it after a
   * confirmation. Table cells and the row page both show it.
   */
  let {
    pageId,
    propertyId,
    storedType,
    disabled = false,
    onSettled,
  }: {
    pageId: string;
    propertyId: string;
    storedType: string;
    disabled?: boolean;
    /** Runs after the value changed so the view reads it again. */
    onSettled: () => Promise<unknown> | void;
  } = $props();

  const { t } = getLocalization();
  /** Whether Convert or Discard is running, so only that choice shows the spinner. */
  let pendingChoice = $state<"convert" | "discard" | null>(null);
  const busy = $derived(pendingChoice !== null);
  let failed = $state(false);
  let hintOpen = $state(false);
  let confirmingDiscard = $state(false);
  /** Convert stays disabled until the loss preview is read, so the warning always shows before converting. */
  let preview = $state<"loading" | "failed" | { conversionLosesData: boolean }>("loading");

  $effect(() => {
    const target = { pageId, propertyId, storedType };
    let current = true;
    preview = "loading";
    previewNotesMismatchedValue(target.pageId, target.propertyId)
      .then((result) => {
        if (current) preview = { conversionLosesData: result.conversionLosesData };
      })
      .catch((cause: unknown) => {
        console.warn("Notes mismatched value preview could not be read", cause);
        if (current) preview = "failed";
      });
    return () => {
      current = false;
    };
  });

  async function settle(convert: boolean): Promise<void> {
    if (busy) return;
    pendingChoice = convert ? "convert" : "discard";
    failed = false;
    try {
      await settleNotesMismatchedValue(pageId, propertyId, convert);
      await onSettled();
    } catch (cause: unknown) {
      console.warn("Notes mismatched value could not be changed", cause);
      failed = true;
    } finally {
      pendingChoice = null;
    }
  }
</script>

<div class="flex min-w-0 flex-col gap-0.5 px-2 py-1 text-xs">
  <div class="flex min-w-0 flex-wrap items-center gap-1">
    <button
      class="truncate rounded-md text-left text-muted-foreground hover:text-foreground"
      type="button"
      aria-expanded={hintOpen}
      onclick={() => { hintOpen = !hintOpen; }}
    >{t("notes.mismatch.label", notesStoredPropertyTypeLabel(storedType, t))}</button>
    <SyncConflictChoice
      disabled={disabled || busy || typeof preview !== "object"}
      pending={pendingChoice === "convert"}
      onclick={() => void settle(true)}
    >{t("notes.mismatch.convert")}</SyncConflictChoice>
    <SyncConflictChoice
      disabled={disabled || busy}
      pending={pendingChoice === "discard"}
      onclick={() => { confirmingDiscard = true; }}
    >{t("notes.mismatch.discard")}</SyncConflictChoice>
  </div>
  {#if typeof preview === "object" && preview.conversionLosesData}
    <span class="text-muted-foreground">{t("notes.mismatch.lossy")}</span>
  {:else if preview === "failed"}
    <span class="text-muted-foreground">{t("notes.mismatch.previewFailed")}</span>
  {/if}
  {#if hintOpen}<span class="text-muted-foreground">{t("notes.mismatch.hint")}</span>{/if}
  {#if failed}<Notice message={t("notes.mismatch.failed")} copy={false} />{/if}
</div>

{#if confirmingDiscard}
  <div class="contents" use:portal>
    <ConfirmDialog
      title={t("notes.mismatch.discardTitle")}
      message={t("notes.mismatch.discardBody")}
      confirmLabel={t("notes.mismatch.discard")}
      cancelLabel={t("common.cancel")}
      onConfirm={() => { confirmingDiscard = false; void settle(false); }}
      onCancel={() => { confirmingDiscard = false; }}
    />
  </div>
{/if}
