<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import VaultHandoffPanel from "$lib/components/vault/handoff/VaultHandoffPanel.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { flushConfig } from "$lib/vault/config";
  import {
    backupActiveVault,
    formatVaultError,
    restoreVaultBackup,
  } from "$lib/vault/state";

  const { t } = getLocalization();
  const actionButtonClass =
    "inline-flex h-7 shrink-0 items-center justify-center rounded-md border border-primary bg-primary px-3 text-[0.8rem] font-medium text-primary-foreground transition-colors active:bg-primary/90 disabled:pointer-events-none disabled:opacity-55";

  let busy = $state<"backup" | "restore" | null>(null);
  let restoreConfirmationOpen = $state(false);
  let status = $state<{ kind: "success" | "error"; message: string } | null>(null);

  async function backUpData(): Promise<void> {
    if (busy) return;
    busy = "backup";
    status = null;
    try {
      await flushConfig();
      const outcome = await backupActiveVault();
      status = { kind: "success", message: t("settings.data.backupSaved", outcome.fileName) };
    } catch (cause: unknown) {
      status = { kind: "error", message: formatVaultError(cause, "backup", t) };
    } finally {
      busy = null;
    }
  }

  async function restoreData(): Promise<void> {
    if (busy) return;
    restoreConfirmationOpen = false;
    busy = "restore";
    status = null;
    try {
      const restored = await restoreVaultBackup();
      if (restored) window.location.reload();
    } catch (cause: unknown) {
      status = { kind: "error", message: formatVaultError(cause, "restore", t) };
    } finally {
      busy = null;
    }
  }
</script>

<div class="flex flex-col gap-6">
  <VaultHandoffPanel platform="android" onActivated={() => window.location.reload()} />

  <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">
      {t("settings.data.backupHeading")}
    </h2>

    <div class="flex flex-col gap-3">
      <div class="flex items-start justify-between gap-4 px-1 py-1">
        <div class="min-w-0 flex-1">
          <div class="text-[0.866667rem] text-foreground">{t("settings.data.backupNow")}</div>
          <div class="mt-0.5 text-[0.8rem] leading-5 text-muted-foreground">
            {t("settings.data.backupNowDescription")}
          </div>
        </div>
        <button
          type="button"
          class={actionButtonClass}
          disabled={busy !== null}
          onclick={() => void backUpData()}
        >
          {#if busy === "backup"}
            <LoaderCircle size={14} strokeWidth={2.1} class="animate-spin" aria-hidden="true" />
          {:else}
            {t("settings.data.backupAction")}
          {/if}
        </button>
      </div>

      <div class="flex items-start justify-between gap-4 px-1 py-1">
        <div class="min-w-0 flex-1">
          <div class="text-[0.866667rem] text-foreground">{t("settings.data.restoreBackup")}</div>
          <div class="mt-0.5 text-[0.8rem] leading-5 text-muted-foreground">
            {t("settings.data.restoreBackupDescription")}
          </div>
        </div>
        <button
          type="button"
          class={actionButtonClass}
          disabled={busy !== null}
          onclick={() => { restoreConfirmationOpen = true; }}
        >
          {#if busy === "restore"}
            <LoaderCircle size={14} strokeWidth={2.1} class="animate-spin" aria-hidden="true" />
          {:else}
            {t("settings.data.restoreAction")}
          {/if}
        </button>
      </div>
    </div>

    {#if status}
      <p
        role={status.kind === "error" ? "alert" : "status"}
        class={`px-1 text-[0.8rem] leading-5 ${status.kind === "error" ? "text-destructive" : "text-muted-foreground"}`}
      >
        {status.message}
      </p>
    {/if}
  </section>
</div>

{#if restoreConfirmationOpen}
  <ConfirmDialog
    title={t("settings.data.restoreConfirmTitle")}
    message={t("settings.data.restoreConfirmMessage")}
    confirmLabel={t("settings.data.restoreConfirmAction")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => void restoreData()}
    onCancel={() => { restoreConfirmationOpen = false; }}
  />
{/if}
