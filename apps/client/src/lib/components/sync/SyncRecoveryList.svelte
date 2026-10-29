<script lang="ts">
  import {
    discardSyncRecovery,
    listSyncRecovery,
    restoreSyncRecovery,
    type SyncRecoveryEntry,
    type SyncRecoveryText,
  } from "$lib/api/sync";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { syncPresentations } from "$lib/sync/presentations";
  import { formatSyncTime, syncDeviceName } from "$lib/sync/status";
  import { writeTextToClipboard } from "$lib/utils/clipboard";

  let {
    buttonClass,
    recoveryCount,
    now,
    mobileLayout = false,
  }: {
    buttonClass: string;
    /** Count from the sync status; a change reloads the list. */
    recoveryCount: number;
    now: number;
    mobileLayout?: boolean;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  let entries = $state<SyncRecoveryEntry[]>([]);
  let busyKey = $state<string | null>(null);
  let message = $state("");
  let failed = $state(false);
  let loadFailed = $state(false);
  let loadGeneration = 0;

  const actionClass = $derived(mobileLayout ? `${buttonClass} min-h-12` : buttonClass);

  function entryKey(entry: SyncRecoveryEntry): string {
    return `${entry.table}:${entry.rowKey}`;
  }

  async function load(): Promise<void> {
    const expected = ++loadGeneration;
    try {
      const next = await listSyncRecovery();
      if (expected !== loadGeneration) return;
      entries = next;
      loadFailed = false;
    } catch (cause: unknown) {
      if (expected !== loadGeneration) return;
      console.warn("sync recovery entries could not be loaded", cause);
      failed = true;
      loadFailed = true;
      message = t("sync.recovery.loadFailed");
    }
  }

  function retryLoad(): Promise<void> {
    message = "";
    failed = false;
    return load();
  }

  async function act(entry: SyncRecoveryEntry, restore: boolean): Promise<void> {
    if (busyKey) return;
    busyKey = entryKey(entry);
    message = "";
    failed = false;
    const target = { table: entry.table, rowKey: entry.rowKey };
    try {
      if (restore) await restoreSyncRecovery(target);
      else await discardSyncRecovery(target);
      if (restore) syncPresentations.notifyChanged([entry.table]);
      message = restore ? syncPresentations.recoveryRestored(t, entry.table) : t("sync.recovery.discarded");
      await load();
    } catch (cause: unknown) {
      console.warn("sync recovery action failed", cause);
      failed = true;
      message = restore ? syncPresentations.recoveryRestoreFailed(t, entry.table) : t("sync.recovery.discardFailed");
      await load();
    } finally {
      busyKey = null;
    }
  }

  /** Copies the text of a deleted row the offer may not bring back. */
  async function copyUnseen(text: SyncRecoveryText): Promise<void> {
    message = "";
    failed = false;
    loadFailed = false;
    try {
      await writeTextToClipboard(text.title && text.preview ? `${text.title}\n\n${text.preview}` : text.title || text.preview);
      message = t("sync.recovery.copied");
    } catch (cause: unknown) {
      console.warn("sync recovery text could not be copied", cause);
      failed = true;
      message = t("sync.recovery.copyFailed");
    }
  }

  $effect(() => {
    void recoveryCount;
    void load();
  });
</script>

<div class="flex min-w-0 flex-col gap-2">
  <div class="px-1">
    <div class="text-[0.866667rem] text-foreground">{t("sync.recovery.heading")}</div>
    <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("sync.recovery.description")}</div>
  </div>
  {#each entries as entry (entryKey(entry))}
    <div class="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-2 px-1">
      <div class="min-w-0 flex-1 basis-48">
        <div class="truncate text-[0.866667rem] font-medium text-foreground">{entry.title || syncPresentations.recoveryUntitled(t, entry.table)}</div>
        {#if entry.preview}<div class="truncate text-[0.8rem] text-muted-foreground">{entry.preview}</div>{/if}
        {#if syncPresentations.recoveryNote(t, entry.table)}
          <div class="text-[0.75rem] text-muted-foreground">{syncPresentations.recoveryNote(t, entry.table)}</div>
        {/if}
        {#if entry.descendants > 0}
          <div class="text-[0.75rem] text-muted-foreground">{t("sync.recovery.descendants", entry.descendants)}</div>
        {/if}
        <div class="text-[0.75rem] text-muted-foreground">
          {t(
            "sync.recovery.editedBy",
            syncDeviceName(entry.device, t("sync.recovery.thisDevice"), t("sync.recovery.otherDevice")),
            formatSyncTime(t, localization.locale, entry.editedAtMs, now),
          )}
        </div>
        {#if entry.unseen.length > 0}
          <div class="mt-1 text-[0.75rem] text-muted-foreground">{t("sync.recovery.unseen")}</div>
          <ul class="mt-1 flex min-w-0 flex-col gap-1">
            {#each entry.unseen as text, index (index)}
              <li class="flex min-w-0 items-start gap-2">
                <div class="min-w-0 flex-1 text-[0.8rem] break-words whitespace-pre-wrap text-foreground">
                  {#if text.title}<div class="font-medium">{text.title}</div>{/if}
                  {#if text.preview}<div class="line-clamp-4">{text.preview}</div>{/if}
                </div>
                <button type="button" class={actionClass} onclick={() => void copyUnseen(text)}>
                  {t("sync.recovery.copy")}
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
      <div class="flex shrink-0 gap-2">
        {#if syncPresentations.recoveryRestorable(entry.table)}
          <button type="button" class={actionClass} disabled={busyKey !== null} onclick={() => void act(entry, true)}>
            {t("sync.recovery.restore")}
          </button>
        {/if}
        <button type="button" class={actionClass} disabled={busyKey !== null} onclick={() => void act(entry, false)}>
          {t("sync.recovery.discard")}
        </button>
      </div>
    </div>
  {/each}
  {#if message}
    {#if failed}
      <Notice class="px-1 text-[0.8rem] leading-5" {message} copy={false} onRetry={loadFailed ? retryLoad : undefined} />
    {:else}
      <p role="status" class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{message}</p>
    {/if}
  {/if}
</div>
