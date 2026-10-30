<script lang="ts">
  import Download from "@lucide/svelte/icons/download";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { formatDateTime, formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { replacedVaultRestoreError } from "$lib/vault/backup-restore";
  import {
    deleteReplacedVaultBackup,
    exportReplacedVaultBackup,
    formatVaultError,
    listReplacedVaultBackups,
    restoreReplacedVaultBackup,
    type ReplacedVaultBackup,
  } from "$lib/vault/state";

  /**
   * Lists the Android backups of vaults that a first link replaced, with restore, export, and
   * delete.
   *
   * @param onRestored - Reloads the app after a restore replaced the active vault.
   */
  let { onRestored }: { onRestored: () => void } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const BYTES_PER_MEGABYTE = 1_000_000;
  const buttonClass =
    "inline-flex h-7 shrink-0 items-center justify-center gap-1.5 rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent focus:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-55 dark:bg-transparent";

  let backups = $state<ReplacedVaultBackup[]>([]);
  let pendingDelete = $state<ReplacedVaultBackup | null>(null);
  let pendingRestore = $state<ReplacedVaultBackup | null>(null);
  let busy = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let error = $state<string | null>(null);

  async function loadBackups(): Promise<void> {
    try {
      backups = await listReplacedVaultBackups();
    } catch (cause: unknown) {
      error = formatVaultError(cause, "general", t);
    }
  }

  function backupName(backup: ReplacedVaultBackup): string {
    return backup.displayName ?? t("settings.data.replacedVaultUnnamed");
  }

  function backupDetails(backup: ReplacedVaultBackup): string {
    const date = formatDateTime(localization.locale, new Date(backup.replacedAtMs), {
      dateStyle: "medium",
      timeStyle: "short",
    });
    const size = formatNumber(localization.locale, backup.sizeBytes / BYTES_PER_MEGABYTE, {
      style: "unit",
      unit: "megabyte",
      maximumFractionDigits: 1,
    });
    return `${date} · ${size}`;
  }

  async function exportBackup(backup: ReplacedVaultBackup): Promise<void> {
    busy = backup.id;
    error = null;
    notice = null;
    try {
      const outcome = await exportReplacedVaultBackup(backup.id);
      notice = t("settings.data.backupSaved", outcome.fileName);
    } catch (cause: unknown) {
      error = formatVaultError(cause, "backup", t);
    } finally {
      busy = null;
    }
  }

  async function restoreBackup(backup: ReplacedVaultBackup): Promise<void> {
    pendingRestore = null;
    busy = backup.id;
    error = null;
    notice = null;
    try {
      const outcome = await restoreReplacedVaultBackup(backup.id);
      if (outcome.kind === "restored") {
        onRestored();
        return;
      }
      error = replacedVaultRestoreError(outcome, t);
    } catch (cause: unknown) {
      error = formatVaultError(cause, "restore", t);
    } finally {
      busy = null;
    }
  }

  async function deleteBackup(backup: ReplacedVaultBackup): Promise<void> {
    pendingDelete = null;
    busy = backup.id;
    error = null;
    notice = null;
    try {
      await deleteReplacedVaultBackup(backup.id);
      await loadBackups();
    } catch (cause: unknown) {
      error = formatVaultError(cause, "general", t);
    } finally {
      busy = null;
    }
  }

  $effect(() => {
    void loadBackups();
  });
</script>

{#if backups.length > 0 || error}
  <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

  <section class="flex flex-col gap-4">
    <div class="px-1">
      <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("settings.data.replacedVaultsHeading")}</h2>
      <p class="mt-0.5 text-[0.8rem] leading-5 text-muted-foreground">{t("settings.data.replacedVaultsDescription")}</p>
    </div>

    <div class="flex flex-col gap-3">
      {#each backups as backup (backup.id)}
        <div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 px-1 py-1">
          <div class="min-w-0 flex-1 text-[0.8rem] leading-5">
            <div class="truncate text-foreground">{backupName(backup)}</div>
            <div class="text-muted-foreground">{backupDetails(backup)}</div>
          </div>
          <div class="flex shrink-0 gap-2">
            <button
              type="button"
              onclick={() => pendingRestore = backup}
              disabled={busy !== null}
              class={buttonClass}
            >
              <RotateCcw size={14} strokeWidth={1.9} class="shrink-0" />
              <span>{t("settings.data.replacedVaultRestore")}</span>
            </button>
            <button
              type="button"
              onclick={() => void exportBackup(backup)}
              disabled={busy !== null}
              class={buttonClass}
            >
              {#if busy === backup.id}
                <LoaderCircle size={14} strokeWidth={2.1} class="shrink-0 animate-spin" />
              {:else}
                <Download size={14} strokeWidth={1.9} class="shrink-0" />
              {/if}
              <span>{t("settings.data.replacedVaultExport")}</span>
            </button>
            <button
              type="button"
              onclick={() => pendingDelete = backup}
              disabled={busy !== null}
              class={buttonClass}
            >
              <Trash2 size={14} strokeWidth={1.9} class="shrink-0" />
              <span>{t("settings.data.replacedVaultDelete")}</span>
            </button>
          </div>
        </div>
      {/each}
    </div>

    {#if notice}
      <p role="status" class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{notice}</p>
    {/if}
    {#if error}
      <Notice class="px-1 text-[0.8rem]" message={error} />
    {/if}
  </section>
{/if}

{#if pendingRestore}
  <ConfirmDialog
    title={t("settings.data.replacedVaultRestoreTitle")}
    message={t("settings.data.replacedVaultRestoreMessage")}
    confirmLabel={t("settings.data.replacedVaultRestore")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => { const backup = pendingRestore; if (backup) void restoreBackup(backup); }}
    onCancel={() => pendingRestore = null}
  />
{/if}

{#if pendingDelete}
  <ConfirmDialog
    title={t("settings.data.replacedVaultDeleteTitle")}
    message={t("settings.data.replacedVaultDeleteMessage")}
    confirmLabel={t("settings.data.replacedVaultDelete")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => { const backup = pendingDelete; if (backup) void deleteBackup(backup); }}
    onCancel={() => pendingDelete = null}
  />
{/if}
