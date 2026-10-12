<script lang="ts">
  import type { Component } from "svelte";
  import CloudDownload from "@lucide/svelte/icons/cloud-download";
  import DatabaseBackup from "@lucide/svelte/icons/database-backup";
  import FileText from "@lucide/svelte/icons/file-text";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Upload from "@lucide/svelte/icons/upload";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getNotesHistoryRetentionImpact } from "$lib/api/notes/project-history";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import {
    DEFAULT_NOTES_HISTORY_RETENTION_DAYS,
    isNotesHistoryRetentionDays,
    type NotesHistoryRetentionDays,
  } from "$lib/notes/history/retention";
  import {
    DEFAULT_NOTES_PAGE_OPEN_MODE,
    isNotesPageOpenMode,
  } from "$lib/notes/pages/open-mode";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getNotes } from "$lib/stores/notes.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import SwitchField from "$lib/components/ui/SwitchField.svelte";
  import type { NotesTransferOperation } from "$lib/settings/types";

  let {
    onOpenTransferPanel = () => {},
    transfersAvailable = true,
    notificationsAvailable = true,
  }: {
    onOpenTransferPanel?: (operation: NotesTransferOperation) => void;
    transfersAvailable?: boolean;
    notificationsAvailable?: boolean;
  } = $props();

  const preferences = getPreferences();
  const notes = getNotes();
  const localization = getLocalization();
  const { t } = localization;
  let pendingRetention = $state<NotesHistoryRetentionDays | null>(null);
  let pendingVersionCount = $state(0);
  let pendingStoredBytes = $state(0);
  let retentionError = $state<string | null>(null);
  const historyRetention = $derived(
    notes.pageHistorySettings?.retention_days ?? DEFAULT_NOTES_HISTORY_RETENTION_DAYS,
  );
  const notesDefaultOpenModeOptions = $derived([
    { value: "center", label: t("notes.centerPeek") },
    { value: "side", label: t("notes.sidePeek") },
    { value: "full", label: t("notes.fullPage") },
  ]);
  const historyRetentionOptions = $derived([
    { value: "0", label: t("settings.notesGeneral.historyRetentionOff") },
    { value: "7", label: t("settings.notesGeneral.historyRetention7") },
    { value: "30", label: t("settings.notesGeneral.historyRetention30") },
    { value: "90", label: t("settings.notesGeneral.historyRetention90") },
    { value: "180", label: t("settings.notesGeneral.historyRetention180") },
    { value: "365", label: t("settings.notesGeneral.historyRetention365") },
  ]);

  interface TransferAction {
    id: NotesTransferOperation;
    label: () => string;
    description: () => string;
    buttonLabel: () => string;
    icon: Component;
  }

  const importActions: readonly TransferAction[] = [
    {
      id: "html-import",
      label: () => t("notes.htmlImportOpen"),
      description: () => t("settings.notesTransfers.htmlImportSummary"),
      buttonLabel: () => t("notes.htmlImportSubmit"),
      icon: Upload,
    },
    {
      id: "notion-api-import",
      label: () => t("notes.notionApiImportOpen"),
      description: () => t("settings.notesTransfers.notionApiImportSummary"),
      buttonLabel: () => t("notes.notionApiImportSubmit"),
      icon: CloudDownload,
    },
    {
      id: "notion-export-import",
      label: () => t("notes.notionExportImportOpen"),
      description: () => t("settings.notesTransfers.notionExportImportSummary"),
      buttonLabel: () => t("notes.notionExportImportSubmit"),
      icon: FileText,
    },
  ];

  const exportActions: readonly TransferAction[] = [
    {
      id: "json-graph-export",
      label: () => t("notes.jsonGraphExportOpen"),
      description: () => t("settings.notesTransfers.jsonGraphExportSummary"),
      buttonLabel: () => t("notes.jsonGraphExportSubmit"),
      icon: DatabaseBackup,
    },
  ];

  function handleDefaultOpenModeChange(value: string): void {
    if (isNotesPageOpenMode(value)) preferences.setNotesDefaultOpenMode(value);
  }

  function formatStoredBytes(value: number): string {
    if (value < 1024) return `${value} B`;
    const units = ["KB", "MB", "GB", "TB"];
    let size = value / 1024;
    let unitIndex = 0;
    while (size >= 1024 && unitIndex < units.length - 1) {
      size /= 1024;
      unitIndex += 1;
    }
    return `${new Intl.NumberFormat(localization.locale, { maximumFractionDigits: 1 }).format(size)} ${units[unitIndex]}`;
  }

  async function applyHistoryRetention(value: NotesHistoryRetentionDays): Promise<void> {
    retentionError = null;
    await notes.updatePageHistoryRetention(value);
    if (notes.pageHistorySettingsError) {
      retentionError = t(
        "settings.notesGeneral.historyRetentionUpdateFailed",
        notes.pageHistorySettingsError,
      );
    }
  }

  async function selectHistoryRetention(value: string): Promise<void> {
    const parsed = Number.parseInt(value, 10);
    if (!isNotesHistoryRetentionDays(parsed)) return;
    const current = notes.pageHistorySettings?.retention_days
      ?? DEFAULT_NOTES_HISTORY_RETENTION_DAYS;
    if (parsed >= current) {
      await applyHistoryRetention(parsed);
      return;
    }
    retentionError = null;
    try {
      const impact = await getNotesHistoryRetentionImpact(parsed);
      if (impact.versionCount === 0) {
        await applyHistoryRetention(parsed);
        return;
      }
      pendingRetention = parsed;
      pendingVersionCount = impact.versionCount;
      pendingStoredBytes = impact.storedBytes;
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      retentionError = t("settings.notesGeneral.historyRetentionUpdateFailed", message);
    }
  }

  $effect(() => {
    if (notes.pageHistorySettings || notes.pageHistorySettingsLoading) return;
    void notes.loadPageHistorySettings();
  });
</script>

<div class="flex flex-col gap-6">
  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.notesGeneral.heading")}</h2>

    <div class="flex flex-col gap-3">
      <Select
        label={t("settings.notesGeneral.defaultOpenMode")}
        description={t("settings.notesGeneral.defaultOpenModeDescription")}
        value={preferences.notesDefaultOpenMode}
        options={notesDefaultOpenModeOptions}
        onChange={handleDefaultOpenModeChange}
        canReset={preferences.notesDefaultOpenMode !== DEFAULT_NOTES_PAGE_OPEN_MODE}
        onReset={() => preferences.resetNotesDefaultOpenMode()}
      />
      <Select
        label={t("settings.notesGeneral.historyRetention")}
        description={t("settings.notesGeneral.historyRetentionDescription")}
        value={String(historyRetention)}
        options={historyRetentionOptions}
        onChange={(value) => { void selectHistoryRetention(value); }}
        canReset={historyRetention !== DEFAULT_NOTES_HISTORY_RETENTION_DAYS}
        onReset={() => { void selectHistoryRetention(String(DEFAULT_NOTES_HISTORY_RETENTION_DAYS)); }}
      />
      {#if historyRetention === 365}
        <div class="flex items-center gap-1.5 px-1 text-[0.733333rem] text-warning">
          <TriangleAlert class="size-3.5 shrink-0" />
          <span>{t("settings.notesGeneral.historyRetention365Warning")}</span>
        </div>
      {/if}
      {#if retentionError}
        <Notice class="px-1 text-[0.8rem]" message={retentionError} />
      {/if}
      <SwitchField
        label={t("settings.notesGeneral.showPropertyIcons")}
        description={t("settings.notesGeneral.showPropertyIconsDescription")}
        checked={preferences.notesShowPropertyIcons}
        onChange={preferences.setNotesShowPropertyIcons}
      />
    </div>
  </section>

  {#if notificationsAvailable}
    <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

    <section class="flex flex-col gap-4">
      <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.notesNotifications.heading")}</h2>

      <div class="flex flex-col gap-3">
        <SwitchField
          label={t("settings.notesNotifications.enable")}
          description={t("settings.notesNotifications.enableDescription")}
          checked={preferences.notesMentionNotificationsEnabled}
          onChange={preferences.setNotesMentionNotificationsEnabled}
        />
        <SwitchField
          label={t("settings.notesNotifications.reminders")}
          description={t("settings.notesNotifications.remindersDescription")}
          checked={preferences.notesReminderNotificationsEnabled}
          disabled={!preferences.notesMentionNotificationsEnabled}
          onChange={preferences.setNotesReminderNotificationsEnabled}
        />
        <SwitchField
          label={t("settings.notesNotifications.userMentions")}
          description={t("settings.notesNotifications.userMentionsDescription")}
          checked={preferences.notesUserMentionNotificationsEnabled}
          disabled={!preferences.notesMentionNotificationsEnabled}
          onChange={preferences.setNotesUserMentionNotificationsEnabled}
        />
        <SwitchField
          label={t("settings.notesNotifications.taskMentions")}
          description={t("settings.notesNotifications.taskMentionsDescription")}
          checked={preferences.notesTaskMentionNotificationsEnabled}
          disabled={!preferences.notesMentionNotificationsEnabled}
          onChange={preferences.setNotesTaskMentionNotificationsEnabled}
        />
        <SwitchField
          label={t("settings.notesNotifications.includeContent")}
          description={t("settings.notesNotifications.includeContentDescription")}
          checked={preferences.notesNotificationIncludeContent}
          disabled={!preferences.notesMentionNotificationsEnabled}
          onChange={preferences.setNotesNotificationIncludeContent}
        />
      </div>
    </section>
  {/if}

  {#if transfersAvailable}
    <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

    <section class="flex flex-col gap-4">
      <div class="min-w-0 px-1">
        <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("settings.notesTransfers.importHeading")}</h2>
      </div>

      <div class="flex flex-col gap-3">
        {#each importActions as action (action.id)}
          {@const Icon = action.icon}
          <div class="flex items-center justify-between gap-4 px-1 py-1 max-[520px]:flex-col max-[520px]:items-stretch max-[520px]:gap-2">
            <div class="min-w-0 flex-1">
              <div class="text-[0.866667rem] text-foreground">{action.label()}</div>
              <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{action.description()}</div>
            </div>
            <button
              type="button"
              class="flex h-7 shrink-0 items-center justify-center gap-1.5 rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent dark:bg-transparent max-[520px]:w-fit"
              onclick={() => onOpenTransferPanel(action.id)}
            >
              <Icon size={13} strokeWidth={2.25} />
              <span>{action.buttonLabel()}</span>
            </button>
          </div>
        {/each}
      </div>
    </section>

    <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

    <section class="flex flex-col gap-4">
      <div class="min-w-0 px-1">
        <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("settings.notesTransfers.exportHeading")}</h2>
      </div>

      <div class="flex flex-col gap-3">
        {#each exportActions as action (action.id)}
          {@const Icon = action.icon}
          <div class="flex items-center justify-between gap-4 px-1 py-1 max-[520px]:flex-col max-[520px]:items-stretch max-[520px]:gap-2">
            <div class="min-w-0 flex-1">
              <div class="text-[0.866667rem] text-foreground">{action.label()}</div>
              <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{action.description()}</div>
            </div>
            <button
              type="button"
              class="flex h-7 shrink-0 items-center justify-center gap-1.5 rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent dark:bg-transparent max-[520px]:w-fit"
              onclick={() => onOpenTransferPanel(action.id)}
            >
              <Icon size={13} strokeWidth={2.25} />
              <span>{action.buttonLabel()}</span>
            </button>
          </div>
        {/each}
      </div>
    </section>
  {/if}
</div>

{#if pendingRetention !== null}
  <ConfirmDialog
    title={t("settings.notesGeneral.historyRetentionPruneTitle")}
    message={t(
      "settings.notesGeneral.historyRetentionPruneMessage",
      pendingVersionCount,
      formatStoredBytes(pendingStoredBytes),
    )}
    confirmLabel={t("projects.settings.save")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => {
      const value = pendingRetention;
      pendingRetention = null;
      if (value !== null) void applyHistoryRetention(value);
    }}
    onCancel={() => {
      pendingRetention = null;
    }}
  />
{/if}
