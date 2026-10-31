<script lang="ts">
  import type { SyncLateChange } from "$lib/api/sync";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getSyncAuthorityState } from "$lib/stores/sync-authority.svelte";
  import { syncLateChangeKey, syncLateChangeOrigin } from "$lib/sync/authority";
  import { syncPresentations } from "$lib/sync/presentations";

  let {
    buttonClass,
    changes,
    total,
    canResolve,
    now,
  }: {
    buttonClass: string;
    changes: SyncLateChange[];
    /** Every late change waiting, including those past the listed bound. */
    total: number;
    /** Whether this admin desktop may keep or discard the changes. */
    canResolve: boolean;
    now: number;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const authorityState = getSyncAuthorityState();
  let busyKey = $state<string | null>(null);
  let message = $state("");
  let failed = $state(false);
  let pendingDiscard = $state<SyncLateChange | null>(null);

  function confirmDiscard(): void {
    const change = pendingDiscard;
    pendingDiscard = null;
    if (change) void act(change, false);
  }

  async function act(change: SyncLateChange, restore: boolean): Promise<void> {
    if (busyKey) return;
    busyKey = syncLateChangeKey(change);
    message = "";
    failed = false;
    try {
      if (restore) await authorityState.restoreLateChange(change);
      else await authorityState.discardLateChange(change);
      if (restore) syncPresentations.notifyChanged([change.table]);
      message = restore ? t("sync.late.restored") : t("sync.late.discarded");
    } catch (cause: unknown) {
      console.warn("late change decision failed", cause);
      failed = true;
      message = restore ? t("sync.late.restoreFailed") : t("sync.late.discardFailed");
    } finally {
      busyKey = null;
    }
  }
</script>

<div class="flex min-w-0 flex-col gap-2">
  <div class="px-1">
    <div class="text-[0.866667rem] text-foreground">{t("sync.late.heading")}</div>
    <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("sync.late.description")}</div>
  </div>
  {#each changes as change (syncLateChangeKey(change))}
    <div class="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-2 px-1">
      <div class="min-w-0 flex-1 basis-48">
        <div class="truncate text-[0.866667rem] font-medium text-foreground">{change.title || syncPresentations.recoveryUntitled(t, change.table)}</div>
        {#if change.preview}<div class="truncate text-[0.8rem] text-muted-foreground">{change.preview}</div>{/if}
        <div class="text-[0.75rem] text-muted-foreground">{syncLateChangeOrigin(t, localization.locale, change, now)}</div>
        {#if change.decisionPending}
          <div class="text-[0.75rem] text-muted-foreground">{t("sync.late.decisionPending")}</div>
        {/if}
      </div>
      {#if canResolve && !change.decisionPending}
        <div class="flex shrink-0 gap-2">
          <button type="button" class={buttonClass} disabled={busyKey !== null} onclick={() => void act(change, true)}>
            {t("sync.late.restore")}
          </button>
          <button type="button" class={buttonClass} disabled={busyKey !== null} onclick={() => { pendingDiscard = change; }}>
            {t("sync.late.discard")}
          </button>
        </div>
      {/if}
    </div>
  {/each}
  {#if total > changes.length}
    <p class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{t("sync.late.more", total - changes.length)}</p>
  {/if}
  {#if !canResolve}
    <p class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{t("sync.late.adminOnly")}</p>
  {/if}
  {#if message}
    {#if failed}
      <Notice class="px-1 text-[0.8rem] leading-5" {message} copy={false} />
    {:else}
      <p role="status" class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{message}</p>
    {/if}
  {/if}
</div>
{#if pendingDiscard}
  <ConfirmDialog
    title={t("sync.late.discardTitle")}
    message={t("sync.late.discardMessage")}
    confirmLabel={t("sync.late.discard")}
    cancelLabel={t("common.cancel")}
    onConfirm={confirmDiscard}
    onCancel={() => { pendingDiscard = null; }}
  />
{/if}
