<script lang="ts">
  import CircleQuestionMark from "@lucide/svelte/icons/circle-question-mark";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import SwitchField from "$lib/components/ui/SwitchField.svelte";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    AUTOMATIC_BACKUP_INTERVALS,
    automaticBackupSchedule,
    chooseAutomaticBackupFolder,
    configureAutomaticBackup,
    getAutomaticBackupStatus,
    resetAutomaticBackupFolder,
    runAutomaticBackupNow,
    type AutomaticBackupFailureKind,
    type AutomaticBackupRefusal,
    type AutomaticBackupStatus,
    type AutomaticBackupUpdate,
  } from "$lib/vault/automatic-backup";
  import { listenForAutomaticBackupChanges } from "$lib/vault/automatic-backup-events";
  import { formatVaultError } from "$lib/vault/state";

  /**
   * The automatic backup rows of Settings > Data: a switch, then the frequency, the folder, and the last and next
   * backups while it is on.
   */
  let {
    buttonClass,
  }: {
    /** Classes of the row buttons, matching the surrounding section. */
    buttonClass: string;
  } = $props();

  type Load =
    | { kind: "loading" }
    | { kind: "failed"; message: string }
    | { kind: "ready"; status: AutomaticBackupStatus; readAt: Date };
  type Action = "toggle" | "interval" | "folder" | "reset" | "run";

  const localization = getLocalization();
  const { t } = localization;
  let load = $state<Load>({ kind: "loading" });
  let busy = $state<Action | null>(null);
  let actionError = $state<string | null>(null);
  /** Names the latest status read or change, so an earlier response never replaces a later one. */
  let statusRequest = 0;

  const intervalOptions = $derived(
    AUTOMATIC_BACKUP_INTERVALS.map((interval) => ({
      value: interval,
      label: t(`settings.data.automaticBackupInterval.${interval}`),
    })),
  );

  function showStatus(status: AutomaticBackupStatus): void {
    statusRequest += 1;
    load = { kind: "ready", status, readAt: new Date() };
  }

  async function refresh(): Promise<void> {
    const request = ++statusRequest;
    try {
      const status = await getAutomaticBackupStatus();
      if (request === statusRequest) load = { kind: "ready", status, readAt: new Date() };
    } catch (cause: unknown) {
      if (request === statusRequest) load = { kind: "failed", message: formatVaultError(cause, "general", t) };
    }
  }

  /**
   * Reloads the status for the error notice retry action.
   *
   * @returns A promise that rejects when the status still cannot be read.
   */
  async function retryLoad(): Promise<void> {
    await refresh();
    if (load.kind === "failed") throw new Error(load.message);
  }

  $effect(() => {
    let disposed = false;
    let stopListening: (() => void) | null = null;
    void refresh();
    listenForAutomaticBackupChanges(() => void refresh()).then(
      (stop) => {
        if (disposed) stop();
        else stopListening = stop;
      },
      (cause: unknown) => console.error("Failed to listen for automatic backup changes:", cause),
    );
    return () => {
      disposed = true;
      stopListening?.();
    };
  });

  function refusalMessage(reason: AutomaticBackupRefusal): string {
    return reason === "noRecoveryKit"
      ? t("settings.data.backupRefusal.noRecoveryKit")
      : t("settings.data.automaticBackupInsideVault");
  }

  function failureMessage(kind: AutomaticBackupFailureKind): string {
    switch (kind) {
      case "noRecoveryKit":
        return t("settings.data.automaticBackupFailure.noRecoveryKit");
      case "folderUnavailable":
        return t("settings.data.automaticBackupFailure.folderUnavailable");
      case "failed":
        return t("settings.data.automaticBackupFailure.failed");
    }
  }

  async function perform(action: Action, change: () => Promise<AutomaticBackupUpdate>): Promise<void> {
    if (busy) return;
    busy = action;
    actionError = null;
    try {
      const update = await change();
      if (update.kind === "saved") showStatus(update.status);
      else actionError = refusalMessage(update.reason);
    } catch (cause: unknown) {
      actionError = formatVaultError(cause, "backup", t);
    } finally {
      busy = null;
    }
  }

  function setEnabled(status: AutomaticBackupStatus, enabled: boolean): void {
    void perform("toggle", () => configureAutomaticBackup(enabled, status.interval));
  }

  function setInterval(status: AutomaticBackupStatus, value: string): void {
    const interval = AUTOMATIC_BACKUP_INTERVALS.find((candidate) => candidate === value);
    if (!interval || interval === status.interval) return;
    void perform("interval", () => configureAutomaticBackup(status.enabled, interval));
  }

  function chooseFolder(): void {
    void perform("folder", chooseAutomaticBackupFolder);
  }

  function useDefaultFolder(): void {
    void perform("reset", async () => ({ kind: "saved", status: await resetAutomaticBackupFolder() }));
  }

  /**
   * Writes a backup now for the failure notice retry action.
   *
   * @returns A promise that rejects when the backup still fails.
   */
  async function retryBackup(): Promise<void> {
    await perform("run", async () => ({ kind: "saved", status: await runAutomaticBackupNow() }));
    if (actionError || (load.kind === "ready" && load.status.failure)) throw new Error("automatic backup failed");
  }

  function formatMoment(date: Date): string {
    return formatDateTime(localization.locale, date, { dateStyle: "medium", timeStyle: "short" });
  }

  function scheduleText(status: AutomaticBackupStatus, readAt: Date): string | null {
    const schedule = automaticBackupSchedule(status, readAt);
    switch (schedule.kind) {
      case "none":
        return null;
      case "running":
        return t("settings.data.automaticBackupRunning");
      case "last":
        return t("settings.data.automaticBackupLast", formatMoment(schedule.last));
      case "soon":
        return schedule.last === null
          ? t("settings.data.automaticBackupSoon")
          : t("settings.data.automaticBackupLastAndSoon", formatMoment(schedule.last));
      case "next":
        return schedule.last === null
          ? t("settings.data.automaticBackupNext", formatMoment(schedule.next))
          : t("settings.data.automaticBackupLastAndNext", formatMoment(schedule.last), formatMoment(schedule.next));
    }
  }
</script>

{#if load.kind === "ready"}
  {@const status = load.status}
  {@const schedule = scheduleText(status, load.readAt)}
  <div class="flex flex-col gap-3">
    <SwitchField
      label={t("settings.data.automaticBackup")}
      description={t("settings.data.automaticBackupDescription")}
      checked={status.enabled}
      disabled={busy !== null}
      onChange={(enabled) => setEnabled(status, enabled)}
    />

    {#if status.enabled}
      <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
        <div class="flex min-w-0 flex-1 items-center gap-1.5">
          <span class="text-[0.866667rem] text-foreground">{t("settings.data.automaticBackupFrequency")}</span>
          <span
            class="inline-flex text-muted-foreground"
            role="img"
            aria-label={t("settings.data.automaticBackupFrequencyTooltip")}
            data-app-tooltip={t("settings.data.automaticBackupFrequencyTooltip")}
          ><CircleQuestionMark size={14} strokeWidth={1.8} aria-hidden="true" /></span>
        </div>
        <Select
          inline
          value={status.interval}
          options={intervalOptions}
          ariaLabel={t("settings.data.automaticBackupFrequency")}
          disabled={busy !== null}
          onChange={(value) => setInterval(status, value)}
        />
      </div>

      <div class="flex items-start justify-between gap-4 px-1 py-1 max-[520px]:flex-col max-[520px]:items-stretch max-[520px]:gap-2">
        <div class="min-w-0 flex-1">
          <div class="text-[0.866667rem] text-foreground">{t("settings.data.automaticBackupFolder")}</div>
          <div class="mt-0.5 text-[0.8rem] leading-5 wrap-anywhere text-muted-foreground">
            {status.folder.custom
              ? status.folder.label
              : t("settings.data.automaticBackupFolderDefault", status.folder.label)}
          </div>
        </div>
        <div class="flex shrink-0 gap-2 max-[520px]:flex-col">
          {#if status.folder.custom}
            <button type="button" class={buttonClass} disabled={busy !== null} onclick={useDefaultFolder}>
              {#if busy === "reset"}
                <LoaderCircle size={14} strokeWidth={2.1} class="animate-spin" aria-hidden="true" />
              {:else}
                {t("settings.data.automaticBackupUseDefaultFolder")}
              {/if}
            </button>
          {/if}
          <button type="button" class={buttonClass} disabled={busy !== null} onclick={chooseFolder}>
            {#if busy === "folder"}
              <LoaderCircle size={14} strokeWidth={2.1} class="animate-spin" aria-hidden="true" />
            {:else}
              {t("settings.data.automaticBackupChangeFolder")}
            {/if}
          </button>
        </div>
      </div>
    {/if}

    {#if status.failure && !status.running}
      <Notice
        class="px-1 text-[0.8rem]"
        message={failureMessage(status.failure.kind)}
        retryLabel={t("settings.data.automaticBackupRetry")}
        onRetry={busy === null ? retryBackup : undefined}
      />
    {:else if schedule}
      <p role="status" class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{schedule}</p>
    {/if}

    {#if actionError}
      <p role="alert" class="px-1 text-[0.8rem] leading-5 text-destructive">{actionError}</p>
    {/if}
  </div>
{:else if load.kind === "failed"}
  <Notice
    class="px-1 text-[0.8rem]"
    message={load.message}
    retryLabel={t("settings.data.reloadFolderStatus")}
    onRetry={retryLoad}
  />
{/if}
