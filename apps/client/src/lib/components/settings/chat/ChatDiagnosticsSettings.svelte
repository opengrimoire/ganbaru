<script lang="ts">
  import { onMount } from "svelte";
  import * as chatApi from "$lib/api/chat";
  import { CHAT_DIAGNOSTIC_RETENTION_DEFAULT_DAYS, type ChatDiagnosticsRead } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { LocalizedError } from "$lib/i18n/user-errors";
  import { SETTINGS_SECONDARY_BUTTON_CLASS } from "$lib/components/settings/action-buttons";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import SwitchField from "$lib/components/ui/SwitchField.svelte";
  import ChatProcessList from "./ChatProcessList.svelte";
  import ChatScratchReclaimReport from "./ChatScratchReclaimReport.svelte";

  /** Chat health, redacted diagnostics, and device maintenance as plain Chat settings sections. */
  const localization = getLocalization();
  const { t } = localization;
  type Maintenance = "stop" | "rebuild";
  let saving = $state(false);
  let error = $state<string | null>(null);
  let status = $state<string | null>(null);
  let diagnostics = $state<ChatDiagnosticsRead | null>(null);
  let maintenance = $state<Maintenance | null>(null);
  let confirmation = $state("");
  let processRefreshKey = $state(0);
  const retentionOptions = $derived([1, 3, 7, 14, 30].map((value) => ({
    value: String(value),
    label: t("settings.chat.behavior.retentionValue", formatNumber(localization.locale, value)),
  })));

  function count(value: number): string {
    return formatNumber(localization.locale, value);
  }

  async function loadDiagnostics(): Promise<void> {
    diagnostics = await chatApi.readChatDiagnostics();
  }

  /** Retries loading Chat diagnostics and rejects when the load fails again. */
  async function retryDiagnostics(): Promise<void> {
    error = null;
    try {
      await loadDiagnostics();
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.settingsAction"));
      throw cause;
    }
  }

  async function updateDiagnosticPreferences(patch: Partial<ChatDiagnosticsRead["preferences"]>): Promise<string> {
    const current = diagnostics;
    if (!current) throw new LocalizedError(t("settings.chat.behavior.diagnosticsUnavailable"));
    await chatApi.updateChatDiagnosticPreferences({ ...current.preferences, ...patch });
    return t("settings.chat.behavior.preferencesSaved");
  }

  async function run(action: () => Promise<string | null>): Promise<void> {
    if (saving) return;
    saving = true;
    error = null;
    status = null;
    try {
      status = await action();
      await loadDiagnostics();
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.settingsAction"));
    } finally {
      saving = false;
    }
  }

  async function exportDiagnostics(): Promise<string> {
    await chatApi.exportRedactedChatDiagnostics(t("settings.chat.behavior.exportDiagnostics"));
    return t("settings.chat.behavior.exportComplete");
  }

  async function deleteDiagnostics(): Promise<string> {
    const deleted = await chatApi.deleteChatDiagnostics();
    return t("settings.chat.behavior.deletedDiagnostics", count(deleted));
  }

  async function retryCleanup(): Promise<string> {
    const completed = await chatApi.retryChatCheckpointCleanup();
    return t("settings.chat.behavior.cleanupRetried", count(completed));
  }

  /** Runs the confirmed stop or rebuild with the typed confirmation the backend checks again. */
  async function confirmMaintenance(): Promise<string> {
    if (maintenance === "stop") {
      const result = await chatApi.stopAllChatProcesses(confirmation);
      cancelMaintenance();
      processRefreshKey += 1;
      return t("settings.chat.behavior.stopped", count(result.providerProcessesStopped), count(result.terminalsStopped));
    }
    const result = await chatApi.rebuildChatProjections(confirmation);
    cancelMaintenance();
    return t("settings.chat.behavior.rebuilt", count(result.rebuiltThreads));
  }

  function expectedConfirmation(): string {
    return maintenance === "stop" ? "STOP ALL CHAT PROCESSES" : "REBUILD CHAT PROJECTIONS";
  }

  function openMaintenance(kind: Maintenance): void {
    maintenance = kind;
    confirmation = "";
    status = null;
  }

  function cancelMaintenance(): void {
    maintenance = null;
    confirmation = "";
  }

  function diagnosticFieldLabel(field: string): string {
    const labels: Record<string, string> = {
      provider_family: t("settings.chat.behavior.diagnosticProviderFamily"),
      provider_instance: t("settings.chat.behavior.diagnosticProviderInstance"),
      thread_id: t("settings.chat.behavior.diagnosticThreadId"),
      turn_id: t("settings.chat.behavior.diagnosticTurnId"),
      event_type: t("settings.chat.behavior.diagnosticEventType"),
      protocol_label: t("settings.chat.behavior.diagnosticProtocolLabel"),
      timestamp: t("settings.chat.behavior.diagnosticTimestamp"),
      prompts_responses: t("settings.chat.behavior.diagnosticPromptsResponses"),
      tool_content: t("settings.chat.behavior.diagnosticToolContent"),
      paths: t("settings.chat.behavior.diagnosticPaths"),
      environment_credentials: t("settings.chat.behavior.diagnosticEnvironmentCredentials"),
    };
    return labels[field] ?? field;
  }

  function diagnosticStorageLabel(storage: string): string {
    return storage === "active_vault_sqlite" ? t("settings.chat.behavior.activeVaultSqlite") : storage;
  }

  onMount(() => {
    void loadDiagnostics().catch((cause: unknown) => {
      error = chatErrorMessage(cause, t("chatErrors.actions.settingsAction"));
    });
  });
</script>

{#snippet separator()}
  <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>
{/snippet}

{#snippet statusItem(label: string, value: string, detail: string | null = null)}
  <div class="min-w-0">
    <div class="text-[0.8rem] text-muted-foreground">{label}</div>
    <div class="mt-0.5 text-[0.866667rem] text-foreground">{value}</div>
    {#if detail}<div class="mt-0.5 text-[0.733333rem] text-muted-foreground">{detail}</div>{/if}
  </div>
{/snippet}

{#snippet actionRow(label: string, description: string, action: string, onclick: () => void)}
  <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
    <div class="min-w-0 flex-1">
      <div class="text-[0.866667rem] text-foreground">{label}</div>
      <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{description}</div>
    </div>
    <button type="button" class={SETTINGS_SECONDARY_BUTTON_CLASS} disabled={saving} {onclick}>{action}</button>
  </div>
{/snippet}

<div class="flex flex-col gap-6">
  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.chat.behavior.diagnosticsHeading")}</h2>
    {#if error}<Notice class="px-1 text-[0.733333rem]" message={error} onRetry={diagnostics ? undefined : retryDiagnostics} />{/if}
    {#if status}<p role="status" class="px-1 text-[0.8rem] text-action-confirm-signal">{status}</p>{/if}
    {#if diagnostics}
      <div class="@container px-1">
        <div class="grid gap-3 @md:grid-cols-2">
          {@render statusItem(t("settings.chat.behavior.credentialStore"), diagnostics.credentialStoreAvailable ? t("settings.chat.behavior.credentialAvailable") : t("settings.chat.behavior.credentialUnavailable"))}
          {@render statusItem(t("settings.chat.behavior.projectionHealth"), diagnostics.projectionHealthy ? t("settings.chat.behavior.projectionHealthy") : t("settings.chat.behavior.projectionUnhealthy", count(diagnostics.inconsistentProjectionCount)))}
          {@render statusItem(t("settings.chat.behavior.processes"), t("settings.chat.behavior.processCounts", count(diagnostics.liveProviderProcesses), count(diagnostics.activeTurns), count(diagnostics.liveTerminals)))}
          {@render statusItem(t("settings.chat.behavior.probes"), t("settings.chat.behavior.probeCounts", count(diagnostics.healthyProviderProbes), count(diagnostics.unhealthyProviderProbes), count(diagnostics.unknownProviderProbes)))}
          {@render statusItem(t("settings.chat.behavior.attachments"), t("settings.chat.behavior.storageCounts", count(diagnostics.counts.attachmentCount), count(diagnostics.counts.attachmentBytes)), t("settings.chat.behavior.cleanupCounts", count(diagnostics.counts.pendingAttachmentCleanup), count(diagnostics.counts.failedAttachmentCleanup)))}
          {@render statusItem(t("settings.chat.behavior.commandArtifacts"), t("settings.chat.behavior.storageCounts", count(diagnostics.counts.commandOutputEvents), count(diagnostics.counts.commandOutputBytes)), t("settings.chat.behavior.outputBound"))}
        </div>
      </div>
      <div class="flex flex-col gap-3">
        <SwitchField
          label={t("settings.chat.behavior.captureDiagnostics")}
          description={t("settings.chat.behavior.captureFields", diagnostics.capturedFields.map(diagnosticFieldLabel).join(", "))}
          checked={diagnostics.preferences.captureEnabled}
          disabled={saving}
          onChange={(captureEnabled) => void run(() => updateDiagnosticPreferences({ captureEnabled }))}
        />
        <p class="px-1 text-[0.8rem] text-muted-foreground">{t("settings.chat.behavior.excludeFields", diagnostics.excludedFields.map(diagnosticFieldLabel).join(", "))}</p>
        <Select
          label={t("settings.chat.behavior.retentionDays")}
          description={t("settings.chat.behavior.diagnosticUsage", count(diagnostics.counts.retainedEvents), count(diagnostics.counts.retainedBytes), diagnosticStorageLabel(diagnostics.storageLocation))}
          value={String(diagnostics.preferences.retentionDays)}
          options={retentionOptions}
          onChange={(value) => void run(() => updateDiagnosticPreferences({ retentionDays: Number(value) }))}
          disabled={saving}
          canReset={diagnostics.preferences.retentionDays !== CHAT_DIAGNOSTIC_RETENTION_DEFAULT_DAYS}
          onReset={() => void run(() => updateDiagnosticPreferences({ retentionDays: CHAT_DIAGNOSTIC_RETENTION_DEFAULT_DAYS }))}
        />
        <div class="flex flex-wrap gap-2 px-1">
          <button type="button" class={SETTINGS_SECONDARY_BUTTON_CLASS} disabled={saving} onclick={() => void run(exportDiagnostics)}>{t("settings.chat.behavior.exportDiagnostics")}</button>
          <button type="button" class={SETTINGS_SECONDARY_BUTTON_CLASS} disabled={saving} onclick={() => void run(deleteDiagnostics)}>{t("settings.chat.behavior.deleteDiagnostics")}</button>
        </div>
      </div>
    {:else if !error}
      <Loading layout="rows" rows={3} />
    {/if}
  </section>

  {#if diagnostics}
    {@render separator()}

    <section class="flex flex-col gap-4">
      <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.chat.behavior.maintenance")}</h2>
      <div class="flex flex-col gap-3">
        {@render actionRow(t("settings.chat.behavior.stopAll"), t("settings.chat.behavior.stopAllDescription"), t("settings.chat.behavior.stopAllAction"), () => openMaintenance("stop"))}
        {@render actionRow(t("settings.chat.behavior.rebuild"), t("settings.chat.behavior.rebuildDescription"), t("settings.chat.behavior.rebuildAction"), () => openMaintenance("rebuild"))}
        {@render actionRow(
          t("settings.chat.behavior.retryCleanup"),
          t("settings.chat.behavior.checkpointCounts", count(diagnostics.counts.checkpointFailures), count(diagnostics.counts.pendingCheckpointCleanup), count(diagnostics.counts.failedCheckpointCleanup)),
          t("settings.chat.behavior.retryCleanupAction"),
          () => void run(retryCleanup),
        )}
        {#if maintenance}
          <div class="flex flex-col gap-2 border-l-2 border-status-tentative py-1 pl-3">
            <p class="text-[0.8rem] text-foreground">{maintenance === "stop" ? t("settings.chat.behavior.stopImpact") : t("settings.chat.behavior.rebuildImpact")}</p>
            <label class="flex flex-col gap-1.5 text-[0.8rem] text-muted-foreground">
              <span>{t("settings.chat.behavior.typeConfirmation", expectedConfirmation())}</span>
              <input class="field w-full px-2.5 text-[0.8rem] text-foreground" bind:value={confirmation} autocomplete="off" spellcheck="false" />
            </label>
            <div class="flex flex-wrap gap-2">
              <button type="button" class={SETTINGS_SECONDARY_BUTTON_CLASS} disabled={saving || confirmation !== expectedConfirmation()} onclick={() => void run(confirmMaintenance)}>{t("settings.chat.behavior.confirm")}</button>
              <button type="button" class={SETTINGS_SECONDARY_BUTTON_CLASS} disabled={saving} onclick={cancelMaintenance}>{t("common.cancel")}</button>
            </div>
          </div>
        {/if}
      </div>
    </section>

    {@render separator()}
    <ChatProcessList refreshKey={processRefreshKey} />
    {@render separator()}
    <ChatScratchReclaimReport />
  {/if}
</div>
