<script lang="ts">
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import BackupKitPrompt from "$lib/components/vault/BackupKitPrompt.svelte";
  import DatabaseRollbacks from "$lib/components/vault/DatabaseRollbacks.svelte";
  import KeptVaultFolders from "$lib/components/vault/KeptVaultFolders.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { createBackupKitPrompt } from "$lib/vault/backup-kit-prompt.svelte";
  import { backupRefusalMessage } from "$lib/vault/backup-restore";
  import { formatVaultError, startNewVaultAfterLostDatabase } from "$lib/vault/state";

  /**
   * The screen shown when the active vault cannot open at startup.
   *
   * @param message - Localized guidance for the failure.
   * @param onRetry - Opens the app again.
   * @param offerRollbacks - Offers the vault's rollback copies, the kept vault folders to put back, a backup file to restore, and a new vault that keeps the lost one aside, for a vault whose database is gone.
   */
  let {
    message,
    onRetry,
    offerRollbacks = false,
  }: { message: string; onRetry: () => unknown; offerRollbacks?: boolean } = $props();
  const { t } = getLocalization();
  const kitPrompt = createBackupKitPrompt();
  let restoring = $state(false);
  let restoreError = $state<string | null>(null);
  let confirmingNewVault = $state(false);

  /** Restores a picked backup file, which keeps the lost vault's folder, then opens the app again. */
  async function restoreBackup(): Promise<void> {
    if (restoring) return;
    restoring = true;
    restoreError = null;
    try {
      const outcome = await kitPrompt.restore();
      if (outcome.kind === "restored") {
        onRetry();
      } else if (outcome.kind === "refused") {
        restoreError = backupRefusalMessage(outcome.reason, t);
      }
    } catch (cause) {
      console.warn("Could not restore the backup:", cause);
      restoreError = formatVaultError(cause, "restore", t);
    } finally {
      restoring = false;
    }
  }

  /** Sets the lost vault aside whole, starts a new empty vault, then opens the app again. */
  async function startNewVault(): Promise<void> {
    confirmingNewVault = false;
    if (restoring) return;
    restoring = true;
    restoreError = null;
    try {
      await startNewVaultAfterLostDatabase();
      onRetry();
    } catch (cause) {
      console.warn("Could not start a new vault:", cause);
      restoreError = formatVaultError(cause, "general", t);
    } finally {
      restoring = false;
    }
  }
</script>

{#snippet restoreAction()}
  <button type="button" class="notice-action-text" disabled={restoring} onclick={() => void restoreBackup()}>
    {t("mobile.vaultSetup.restoreBackupFile")}
  </button>
  <button type="button" class="notice-action-text" disabled={restoring} onclick={() => confirmingNewVault = true}>
    {t("mobile.vaultSetup.startNewVault")}
  </button>
{/snippet}

<main
  class="mobile-viewport-height flex w-screen bg-background pb-(--safe-area-bottom) pt-(--safe-area-top) text-foreground"
  style="padding-left: calc(var(--safe-area-left) + 1.5rem); padding-right: calc(var(--safe-area-right) + 1.5rem);"
>
  {#if offerRollbacks}
    <div class="mx-auto flex min-h-0 w-full max-w-md flex-col gap-6 overflow-y-auto py-8">
      <Notice
        class="text-base"
        icon={HardDrive}
        title={t("mobile.startupFailed")}
        {message}
        {onRetry}
        actions={restoreAction}
      />
      {#if restoreError}
        <Notice message={restoreError} />
      {/if}
      <DatabaseRollbacks onRestored={onRetry} />
      <KeptVaultFolders onRestored={onRetry} />
    </div>
  {:else}
    <Notice layout="fill" class="text-base" icon={HardDrive} title={t("mobile.startupFailed")} {message} {onRetry} />
  {/if}
</main>

{#if confirmingNewVault}
  <ConfirmDialog
    title={t("mobile.vaultSetup.startNewVaultTitle")}
    message={t("mobile.vaultSetup.startNewVaultMessage")}
    confirmLabel={t("mobile.vaultSetup.startNewVault")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => void startNewVault()}
    onCancel={() => confirmingNewVault = false}
  />
{/if}

{#if kitPrompt.request}
  <BackupKitPrompt
    refusal={kitPrompt.request.refusal}
    onSubmit={(code) => kitPrompt.answer(code)}
    onCancel={() => kitPrompt.answer(null)}
  />
{/if}
