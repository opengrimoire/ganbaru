<script lang="ts">
  import { onMount } from "svelte";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import * as chatApi from "$lib/api/chat";
  import type { ChatDiagnosticsRead } from "$lib/chat/contracts";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import SwitchField from "$lib/components/ui/SwitchField.svelte";

  const localization = getLocalization();
  const { t } = localization;
  const chat = getChat();
  let saving = $state(false);
  let error = $state<string | null>(null);
  let status = $state<string | null>(null);
  let diagnostics = $state<ChatDiagnosticsRead | null>(null);
  let maintenance = $state<"stop" | "rebuild" | null>(null);
  let confirmation = $state("");
  let advancedOpen = $state(false);
  const behavior = $derived(chat.settings?.configuration.behavior ?? null);
  const sendKeyOptions = $derived([
    { value: "enter", label: t("settings.chat.behavior.enter") },
    { value: "mod_enter", label: t("settings.chat.behavior.modEnter") },
  ]);
  const scrollbackOptions = $derived([1_000, 5_000, 10_000, 25_000, 50_000, 100_000].map((value) => ({
    value: String(value),
    label: formatNumber(localization.locale, value),
  })));
  const idleTimeoutOptions = $derived([
    { value: "60", label: t("settings.chat.behavior.oneMinute") },
    { value: "300", label: t("settings.chat.behavior.fiveMinutes") },
    { value: "900", label: t("settings.chat.behavior.fifteenMinutes") },
    { value: "1800", label: t("settings.chat.behavior.thirtyMinutes") },
    { value: "3600", label: t("settings.chat.behavior.oneHour") },
    { value: "7200", label: t("settings.chat.behavior.twoHours") },
  ]);
  const retentionOptions = $derived([1, 3, 7, 14, 30].map((value) => ({
    value: String(value),
    label: t("settings.chat.behavior.retentionValue", formatNumber(localization.locale, value)),
  })));

  async function update(patch: Partial<NonNullable<typeof behavior>>): Promise<void> {
    if (!behavior || saving) return;
    saving = true;
    error = null;
    try {
      await chat.updateBehavior({ ...behavior, ...patch });
    } catch (cause: unknown) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      saving = false;
    }
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
      error = cause instanceof Error ? cause.message : String(cause);
      throw cause;
    }
  }

  async function updateDiagnosticPreferences(patch: Partial<ChatDiagnosticsRead["preferences"]>): Promise<string> {
    const current = diagnostics;
    if (!current) throw new Error(t("settings.chat.behavior.diagnosticsUnavailable"));
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
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      saving = false;
    }
  }

  function expectedConfirmation(): string {
    return maintenance === "stop" ? "STOP ALL CHAT PROCESSES" : "REBUILD CHAT PROJECTIONS";
  }

  function openMaintenance(kind: "stop" | "rebuild"): void {
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
    return storage === "active_vault_sqlite"
      ? t("settings.chat.behavior.activeVaultSqlite")
      : storage;
  }

  onMount(() => {
    void loadDiagnostics().catch((cause: unknown) => {
      error = cause instanceof Error ? cause.message : String(cause);
    });
  });
</script>

<section class="flex flex-col gap-4">
  <div class="px-1">
    <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("settings.chat.behavior.heading")}</h2>
  </div>
  {#if error}<Notice class="text-sm" message={error} onRetry={diagnostics ? undefined : retryDiagnostics} />{/if}
  {#if status}<p role="status" class="text-sm text-action-confirm">{status}</p>{/if}
  {#if behavior}
    <div class="flex flex-col gap-3">
      <Select
        label={t("settings.chat.behavior.sendKey")}
        description={t("settings.chat.behavior.sendKeyDescription")}
        value={behavior.sendKey}
        options={sendKeyOptions}
        onChange={(value) => void update({ sendKey: value === "mod_enter" ? "mod_enter" : "enter" })}
        disabled={saving}
      />
      <Select label={t("settings.chat.behavior.terminalScrollback")} description={t("settings.chat.behavior.terminalScrollbackDescription")} value={String(behavior.terminalScrollbackLines)} options={scrollbackOptions} onChange={(value) => void update({ terminalScrollbackLines: Number(value) })} disabled={saving} />
      <Select label={t("settings.chat.behavior.idleTimeout")} description={t("settings.chat.behavior.idleTimeoutDescription")} value={String(behavior.idleSessionTimeoutSeconds)} options={idleTimeoutOptions} onChange={(value) => void update({ idleSessionTimeoutSeconds: Number(value) })} disabled={saving} />
    </div>
    <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>
    <div class="flex flex-col gap-3">
      <SwitchField label={t("settings.chat.behavior.restoreThread")} description={t("settings.chat.behavior.restoreThreadDescription")} checked={behavior.restoreLastSelectedThread} disabled={saving} onChange={(value) => void update({ restoreLastSelectedThread: value })} />
      <SwitchField label={t("settings.chat.behavior.reasoning")} description={t("settings.chat.behavior.reasoningDescription")} checked={behavior.showReasoningSummaries} disabled={saving} onChange={(value) => void update({ showReasoningSummaries: value })} />
      <SwitchField label={t("settings.chat.behavior.foldWork")} description={t("settings.chat.behavior.foldWorkDescription")} checked={behavior.automaticallyFoldSettledWork} disabled={saving} onChange={(value) => void update({ automaticallyFoldSettledWork: value })} />
      <SwitchField label={t("settings.chat.behavior.confirmPaste")} description={t("settings.chat.behavior.confirmPasteDescription")} checked={behavior.confirmMultilineTerminalPaste} disabled={saving} onChange={(value) => void update({ confirmMultilineTerminalPaste: value })} />
    </div>
  {/if}
  {#if diagnostics}
    <div class="border-y border-border">
      <button
        type="button"
        class="flex w-full items-center gap-2 px-3 py-2.5 text-left text-[0.8rem] font-medium text-foreground hover:bg-accent/40"
        aria-expanded={advancedOpen}
        onclick={() => { advancedOpen = !advancedOpen; }}
      >
        <ChevronRight size={13} class="transition-transform {advancedOpen ? 'rotate-90' : ''}" />
        <span>{t("settings.chat.behavior.advanced")}</span>
      </button>
      {#if advancedOpen}
      <div class="flex flex-col gap-4 border-t border-border p-3">
    <div class="diagnostic-grid">
      <div><div class="text-xs text-muted-foreground">{t("settings.chat.behavior.credentialStore")}</div><div class="mt-1 text-sm font-medium">{diagnostics.credentialStoreAvailable ? t("settings.chat.behavior.credentialAvailable") : t("settings.chat.behavior.credentialUnavailable")}</div></div>
      <div><div class="text-xs text-muted-foreground">{t("settings.chat.behavior.projectionHealth")}</div><div class="mt-1 text-sm font-medium">{diagnostics.projectionHealthy ? t("settings.chat.behavior.projectionHealthy") : t("settings.chat.behavior.projectionUnhealthy", formatNumber(localization.locale, diagnostics.inconsistentProjectionCount))}</div></div>
      <div><div class="text-xs text-muted-foreground">{t("settings.chat.behavior.processes")}</div><div class="mt-1 text-sm">{t("settings.chat.behavior.processCounts", formatNumber(localization.locale, diagnostics.liveProviderProcesses), formatNumber(localization.locale, diagnostics.activeTurns), formatNumber(localization.locale, diagnostics.liveTerminals))}</div></div>
      <div><div class="text-xs text-muted-foreground">{t("settings.chat.behavior.probes")}</div><div class="mt-1 text-sm">{t("settings.chat.behavior.probeCounts", formatNumber(localization.locale, diagnostics.healthyProviderProbes), formatNumber(localization.locale, diagnostics.unhealthyProviderProbes), formatNumber(localization.locale, diagnostics.unknownProviderProbes))}</div></div>
      <div><div class="text-xs text-muted-foreground">{t("settings.chat.behavior.attachments")}</div><div class="mt-1 text-sm">{t("settings.chat.behavior.storageCounts", formatNumber(localization.locale, diagnostics.counts.attachmentCount), formatNumber(localization.locale, diagnostics.counts.attachmentBytes))}</div><div class="text-xs text-muted-foreground">{t("settings.chat.behavior.cleanupCounts", formatNumber(localization.locale, diagnostics.counts.pendingAttachmentCleanup), formatNumber(localization.locale, diagnostics.counts.failedAttachmentCleanup))}</div></div>
      <div><div class="text-xs text-muted-foreground">{t("settings.chat.behavior.commandArtifacts")}</div><div class="mt-1 text-sm">{t("settings.chat.behavior.storageCounts", formatNumber(localization.locale, diagnostics.counts.commandOutputEvents), formatNumber(localization.locale, diagnostics.counts.commandOutputBytes))}</div><div class="text-xs text-muted-foreground">{t("settings.chat.behavior.outputBound")}</div></div>
      <div class="sm:col-span-2"><div class="text-xs text-muted-foreground">{t("settings.chat.behavior.checkpoints")}</div><div class="mt-1 text-sm">{t("settings.chat.behavior.checkpointCounts", formatNumber(localization.locale, diagnostics.counts.checkpointFailures), formatNumber(localization.locale, diagnostics.counts.pendingCheckpointCleanup), formatNumber(localization.locale, diagnostics.counts.failedCheckpointCleanup))}</div></div>
    </div>

    <div class="space-y-3 border-t border-border pt-4">
      <SwitchField label={t("settings.chat.behavior.captureDiagnostics")} checked={diagnostics.preferences.captureEnabled} disabled={saving} onChange={(captureEnabled) => void run(() => updateDiagnosticPreferences({ captureEnabled }))} />
      <Select label={t("settings.chat.behavior.retentionDays")} value={String(diagnostics.preferences.retentionDays)} options={retentionOptions} onChange={(value) => void run(() => updateDiagnosticPreferences({ retentionDays: Number(value) }))} disabled={saving} />
      <p class="text-xs text-muted-foreground">{t("settings.chat.behavior.captureFields", diagnostics.capturedFields.map(diagnosticFieldLabel).join(", "))}</p>
      <p class="text-xs text-muted-foreground">{t("settings.chat.behavior.excludeFields", diagnostics.excludedFields.map(diagnosticFieldLabel).join(", "))}</p>
      <p class="text-xs text-muted-foreground">{t("settings.chat.behavior.diagnosticStorage", diagnosticStorageLabel(diagnostics.storageLocation))}</p>
      <p class="text-xs text-muted-foreground">{t("settings.chat.behavior.diagnosticUsage", formatNumber(localization.locale, diagnostics.counts.retainedEvents), formatNumber(localization.locale, diagnostics.counts.retainedBytes))}</p>
      <div class="flex flex-wrap gap-2"><button type="button" class="chat-settings-action" disabled={saving} onclick={() => void run(async () => { await chatApi.exportRedactedChatDiagnostics(t("settings.chat.behavior.exportDiagnostics")); return t("settings.chat.behavior.exportComplete"); })}>{t("settings.chat.behavior.exportDiagnostics")}</button><button type="button" class="chat-settings-action" disabled={saving} onclick={() => void run(async () => { const count = await chatApi.deleteChatDiagnostics(); return t("settings.chat.behavior.deletedDiagnostics", formatNumber(localization.locale, count)); })}>{t("settings.chat.behavior.deleteDiagnostics")}</button></div>
    </div>

    <div class="space-y-3 border-t border-border pt-4">
      <h3 class="text-sm font-medium">{t("settings.chat.behavior.maintenance")}</h3>
      <p class="text-xs text-muted-foreground">{t("settings.chat.behavior.maintenanceDescription")}</p>
      <div class="flex flex-wrap gap-2"><button type="button" class="chat-settings-action" disabled={saving} onclick={() => openMaintenance("stop")}>{t("settings.chat.behavior.stopAll")}</button><button type="button" class="chat-settings-action" disabled={saving} onclick={() => openMaintenance("rebuild")}>{t("settings.chat.behavior.rebuild")}</button><button type="button" class="chat-settings-action" disabled={saving} onclick={() => void run(async () => { const count = await chatApi.retryChatCheckpointCleanup(); return t("settings.chat.behavior.cleanupRetried", formatNumber(localization.locale, count)); })}>{t("settings.chat.behavior.retryCleanup")}</button></div>
      {#if maintenance}
        <div class="space-y-2 border-l-2 border-status-tentative pl-3">
          <p class="text-xs">{maintenance === "stop" ? t("settings.chat.behavior.stopImpact") : t("settings.chat.behavior.rebuildImpact")}</p>
          <label class="setup-field"><span>{t("settings.chat.behavior.typeConfirmation", expectedConfirmation())}</span><input bind:value={confirmation} autocomplete="off" spellcheck="false" /></label>
          <div class="flex gap-2"><button type="button" class="chat-settings-action" disabled={saving || confirmation !== expectedConfirmation()} onclick={() => void run(async () => { if (maintenance === "stop") { const result = await chatApi.stopAllChatProcesses(confirmation); cancelMaintenance(); return t("settings.chat.behavior.stopped", formatNumber(localization.locale, result.providerProcessesStopped), formatNumber(localization.locale, result.terminalsStopped)); } const result = await chatApi.rebuildChatProjections(confirmation); cancelMaintenance(); return t("settings.chat.behavior.rebuilt", formatNumber(localization.locale, result.rebuiltThreads)); })}>{t("settings.chat.behavior.confirm")}</button><button type="button" class="chat-settings-action" disabled={saving} onclick={cancelMaintenance}>{t("common.cancel")}</button></div>
        </div>
      {/if}
    </div>
      </div>
      {/if}
    </div>
  {:else}
    <Loading layout="fill" class="min-h-40" />
  {/if}
</section>

<style>
  .chat-settings-action { border: 1px solid var(--border); border-radius: 0.375rem; padding: 0.35rem 0.65rem; font-size: calc(0.733333rem * var(--type-scale)); }
  .chat-settings-action:disabled { cursor: not-allowed; opacity: 0.5; }
  .diagnostic-grid { display:grid; gap:1rem; padding-inline:0.25rem; }
  @media (min-width:640px) { .diagnostic-grid { grid-template-columns:repeat(2,minmax(0,1fr)); } }
</style>
