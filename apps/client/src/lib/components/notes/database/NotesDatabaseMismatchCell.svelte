<script lang="ts">
  import { settleNotesMismatchedValue } from "$lib/api/notes/conflicts";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /**
   * A row value stored under another type than its column, written on another device while the type changed. It
   * offers converting the value to the column type or discarding it.
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
  let busy = $state(false);
  let failed = $state(false);

  async function settle(convert: boolean): Promise<void> {
    if (busy) return;
    busy = true;
    failed = false;
    try {
      await settleNotesMismatchedValue(pageId, propertyId, convert);
      await onSettled();
    } catch (cause: unknown) {
      console.warn("Notes mismatched value could not be changed", cause);
      failed = true;
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex min-w-0 flex-wrap items-center gap-1 px-2 py-1 text-[0.8rem]" title={t("notes.mismatch.hint")}>
  <span class="truncate text-muted-foreground">{t("notes.mismatch.label", storedType)}</span>
  <button
    class="rounded-md px-1.5 py-0.5 font-medium underline hover:bg-black/10 disabled:opacity-40 dark:hover:bg-white/10"
    type="button"
    disabled={disabled || busy}
    onclick={() => void settle(true)}
  >{t("notes.mismatch.convert")}</button>
  <button
    class="rounded-md px-1.5 py-0.5 font-medium underline hover:bg-black/10 disabled:opacity-40 dark:hover:bg-white/10"
    type="button"
    disabled={disabled || busy}
    onclick={() => void settle(false)}
  >{t("notes.mismatch.discard")}</button>
  {#if failed}<span class="text-destructive" role="alert">{t("notes.mismatch.failed")}</span>{/if}
</div>
