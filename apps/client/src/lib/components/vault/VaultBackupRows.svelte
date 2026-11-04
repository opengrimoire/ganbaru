<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import BackupKitPrompt from "$lib/components/vault/BackupKitPrompt.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { createBackupKitPrompt } from "$lib/vault/backup-kit-prompt.svelte";
  import {
    backupRefusalMessage,
    backupSaveStatus,
    type BackupStatus,
  } from "$lib/vault/backup-restore";
  import { flushConfig } from "$lib/vault/config";
  import {
    backupActiveVault,
    backupVaultToFile,
    formatVaultError,
    type RestorePlacement,
  } from "$lib/vault/state";

  /**
   * The Back up and Restore rows of Settings > Data. Desktop writes the encrypted backup to a file
   * the user picks; Android writes it to Downloads and confirms before a restore replaces local data.
   */
  let {
    platform,
    buttonClass,
    onRestored,
  }: {
    platform: "desktop" | "android";
    /** Classes of the row buttons, matching the surrounding section. */
    buttonClass: string;
    /** Runs after a restore activated a vault, usually to restart or reload the app. */
    onRestored: (placement: RestorePlacement) => void;
  } = $props();

  const { t } = getLocalization();
  const kitPrompt = createBackupKitPrompt();
  let busy = $state<"backup" | "restore" | null>(null);
  let restoreConfirmationOpen = $state(false);
  let status = $state<BackupStatus>(null);

  async function backUp(): Promise<void> {
    if (busy) return;
    busy = "backup";
    status = null;
    try {
      await flushConfig();
      const outcome = platform === "desktop" ? await backupVaultToFile() : await backupActiveVault();
      status = backupSaveStatus(outcome, platform === "desktop" ? "file" : "downloads", t);
    } catch (cause: unknown) {
      status = { kind: "error", message: formatVaultError(cause, "backup", t) };
    } finally {
      busy = null;
    }
  }

  function requestRestore(): void {
    if (platform === "android") restoreConfirmationOpen = true;
    else void restore();
  }

  async function restore(): Promise<void> {
    if (busy) return;
    restoreConfirmationOpen = false;
    busy = "restore";
    status = null;
    try {
      await flushConfig();
      const outcome = await kitPrompt.restore();
      if (outcome.kind === "refused") {
        status = { kind: "error", message: backupRefusalMessage(outcome.reason, t) };
      } else if (outcome.kind === "restored") {
        status = {
          kind: "success",
          message: outcome.placement === "inPlace"
            ? t("settings.data.restoredInPlace")
            : t("settings.data.restoredCopy"),
        };
        onRestored(outcome.placement);
      }
    } catch (cause: unknown) {
      status = { kind: "error", message: formatVaultError(cause, "restore", t) };
    } finally {
      busy = null;
    }
  }
</script>

<div class="flex flex-col gap-3">
  <div class="flex items-start justify-between gap-4 px-1 py-1 max-[520px]:flex-col max-[520px]:items-stretch max-[520px]:gap-2">
    <div class="min-w-0 flex-1">
      <div class="text-[0.866667rem] text-foreground">{t("settings.data.backupNow")}</div>
      <div class="mt-0.5 text-[0.8rem] leading-5 text-muted-foreground">
        {platform === "desktop"
          ? t("settings.data.backupNowDescriptionDesktop")
          : t("settings.data.backupNowDescription")}
      </div>
    </div>
    <button type="button" class={buttonClass} disabled={busy !== null} onclick={() => void backUp()}>
      {#if busy === "backup"}
        <LoaderCircle size={14} strokeWidth={2.1} class="animate-spin" aria-hidden="true" />
      {:else}
        {t("settings.data.backupAction")}
      {/if}
    </button>
  </div>

  <div class="flex items-start justify-between gap-4 px-1 py-1 max-[520px]:flex-col max-[520px]:items-stretch max-[520px]:gap-2">
    <div class="min-w-0 flex-1">
      <div class="text-[0.866667rem] text-foreground">{t("settings.data.restoreBackup")}</div>
      <div class="mt-0.5 text-[0.8rem] leading-5 text-muted-foreground">
        {platform === "desktop"
          ? t("settings.data.restoreBackupDescriptionDesktop")
          : t("settings.data.restoreBackupDescription")}
      </div>
    </div>
    <button type="button" class={buttonClass} disabled={busy !== null} onclick={requestRestore}>
      {#if busy === "restore"}
        <LoaderCircle size={14} strokeWidth={2.1} class="animate-spin" aria-hidden="true" />
      {:else}
        {t("settings.data.restoreAction")}
      {/if}
    </button>
  </div>

  {#if status}
    <p
      role={status.kind === "error" ? "alert" : "status"}
      class={`px-1 text-[0.8rem] leading-5 ${status.kind === "error" ? "text-destructive" : "text-muted-foreground"}`}
    >
      {status.message}
    </p>
  {/if}
</div>

{#if restoreConfirmationOpen}
  <ConfirmDialog
    title={t("settings.data.restoreConfirmTitle")}
    message={t("settings.data.restoreConfirmMessage")}
    confirmLabel={t("settings.data.restoreConfirmAction")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => void restore()}
    onCancel={() => { restoreConfirmationOpen = false; }}
  />
{/if}

{#if kitPrompt.request}
  <BackupKitPrompt
    refusal={kitPrompt.request.refusal}
    onSubmit={(code) => kitPrompt.answer(code)}
    onCancel={() => kitPrompt.answer(null)}
  />
{/if}
