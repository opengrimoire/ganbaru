<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { formatDateTime, formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    dismissDatabaseRollback,
    formatVaultError,
    getVaultOwnershipStatus,
    listDatabaseRollbacks,
    pendingFlaggedRollbacks,
    restoreDatabaseRollback,
    type DatabaseRollback,
  } from "$lib/vault/state";

  /**
   * Lists device-local copies of earlier vault data and restores one.
   *
   * @param onRestored - Reloads the app after a restore replaced the active database.
   */
  let { onRestored }: { onRestored: () => void } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const BYTES_PER_MEGABYTE = 1_000_000;
  const buttonClass =
    "inline-flex h-7 shrink-0 items-center justify-center gap-1.5 rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent focus:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-55 dark:bg-transparent";

  let rollbacks = $state<DatabaseRollback[]>([]);
  let canRestore = $state(true);
  let pendingRollback = $state<DatabaseRollback | null>(null);
  let busy = $state<string | null>(null);
  let error = $state<string | null>(null);
  const flagged = $derived(pendingFlaggedRollbacks(rollbacks));

  async function loadRollbacks(): Promise<void> {
    try {
      rollbacks = await listDatabaseRollbacks();
    } catch (cause: unknown) {
      error = formatVaultError(cause, "general", t);
    }
    try {
      canRestore = (await getVaultOwnershipStatus()).canWrite;
    } catch {
      canRestore = true;
    }
  }

  function formatRollbackLabel(rollback: DatabaseRollback): string {
    const date = formatDateTime(localization.locale, new Date(rollback.createdAt), {
      dateStyle: "medium",
      timeStyle: "short",
    });
    const size = formatNumber(localization.locale, rollback.sizeBytes / BYTES_PER_MEGABYTE, {
      style: "unit",
      unit: "megabyte",
      maximumFractionDigits: 1,
    });
    return `${date} · ${size}`;
  }

  function rollbackOriginLabel(rollback: DatabaseRollback): string {
    return rollback.origin === "replacement"
      ? t("settings.data.rollbackOriginReplacement")
      : t("settings.data.rollbackOriginReset");
  }

  async function restoreRollback(rollback: DatabaseRollback): Promise<void> {
    pendingRollback = null;
    busy = rollback.id;
    error = null;
    try {
      await restoreDatabaseRollback(rollback.id);
      onRestored();
    } catch (cause: unknown) {
      error = formatVaultError(cause, "general", t);
      busy = null;
    }
  }

  async function dismissRollback(rollback: DatabaseRollback): Promise<void> {
    busy = rollback.id;
    error = null;
    try {
      await dismissDatabaseRollback(rollback.id);
      await loadRollbacks();
    } catch (cause: unknown) {
      error = formatVaultError(cause, "general", t);
    } finally {
      busy = null;
    }
  }

  $effect(() => {
    void loadRollbacks();
  });
</script>

{#if rollbacks.length > 0 || error}
  <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

  <section class="flex flex-col gap-4">
    <div class="px-1">
      <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("settings.data.rollbacksHeading")}</h2>
      <p class="mt-0.5 text-[0.8rem] leading-5 text-muted-foreground">{t("settings.data.rollbacksDescription")}</p>
      {#if !canRestore}
        <p class="mt-0.5 text-[0.8rem] leading-5 text-muted-foreground">{t("settings.data.rollbackNeedsOwnership")}</p>
      {/if}
    </div>

    {#each flagged as rollback (rollback.id)}
      <div role="status" class="flex flex-col gap-2 px-1">
        <div class="flex items-start gap-2 text-[0.8rem] leading-5 text-foreground">
          <TriangleAlert size={14} strokeWidth={1.9} class="mt-0.5 shrink-0 text-destructive" aria-hidden="true" />
          <span>{t("settings.data.rollbackFlagged", formatRollbackLabel(rollback))}</span>
        </div>
        <div class="flex flex-wrap justify-end gap-2">
          <button
            type="button"
            onclick={() => void dismissRollback(rollback)}
            disabled={busy !== null}
            class={buttonClass}
          >
            {t("settings.data.rollbackKeepCurrent")}
          </button>
          <button
            type="button"
            onclick={() => pendingRollback = rollback}
            disabled={busy !== null || !canRestore}
            class={buttonClass}
          >
            <RotateCcw size={14} strokeWidth={1.9} class="shrink-0" />
            <span>{t("settings.data.rollbackRestore")}</span>
          </button>
        </div>
      </div>
    {/each}

    <div class="flex flex-col gap-3">
      {#each rollbacks as rollback (rollback.id)}
        <div class="flex items-center justify-between gap-4 px-1 py-1">
          <div class="min-w-0 flex-1 text-[0.8rem] leading-5">
            <div class="text-foreground">{formatRollbackLabel(rollback)}</div>
            <div class="text-muted-foreground">{rollbackOriginLabel(rollback)}</div>
          </div>
          <button
            type="button"
            onclick={() => pendingRollback = rollback}
            disabled={busy !== null || !canRestore}
            class={buttonClass}
          >
            {#if busy === rollback.id}
              <LoaderCircle size={14} strokeWidth={2.1} class="shrink-0 animate-spin" />
            {:else}
              <RotateCcw size={14} strokeWidth={1.9} class="shrink-0" />
            {/if}
            <span>{t("settings.data.rollbackRestore")}</span>
          </button>
        </div>
      {/each}
    </div>

    {#if error}
      <Notice class="px-1 text-[0.8rem]" message={error} />
    {/if}
  </section>
{/if}

{#if pendingRollback}
  <ConfirmDialog
    title={t("settings.data.rollbackRestoreTitle")}
    message={t("settings.data.rollbackRestoreMessage")}
    confirmLabel={t("settings.data.rollbackRestore")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => { const rollback = pendingRollback; if (rollback) void restoreRollback(rollback); }}
    onCancel={() => pendingRollback = null}
  />
{/if}
