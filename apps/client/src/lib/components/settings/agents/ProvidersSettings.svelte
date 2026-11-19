<script lang="ts">
  import Mail from "@lucide/svelte/icons/mail";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import UserRound from "@lucide/svelte/icons/user-round";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Plus from "@lucide/svelte/icons/plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Search from "@lucide/svelte/icons/search";
  import CircleQuestionMark from "@lucide/svelte/icons/circle-question-mark";
  import Trash2 from "@lucide/svelte/icons/trash";
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import * as chatApi from "$lib/api/chat";
  import { newestModelOfLine } from "$lib/chat/agents/agent-names";
  import { familyAgentRoster, isRemovableAgent, listedCatalogModels, sortCatalogModels } from "$lib/chat/agents/agent-roster";
  import type {
    ChatAiAgentRead,
    ProviderFamilyMetadataRead,
    ProviderInstanceRead,
    ProviderModel,
    ProviderRefreshResult,
    ProviderUpdateRead,
  } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { providerFamilyDisplayName } from "$lib/chat/provider-setup";
  import { providerVersionNotice, type ProviderVersionNotice } from "$lib/chat/provider-version";
  import { formatModelDisplayName, modelCompany } from "$lib/chat/composer/model-company";
  import { familiesInPickerOrder, providersInPickerOrder } from "$lib/chat/composer/model-picker-model";
  import ChatModelAvatar from "$lib/components/chat/identity/ChatModelAvatar.svelte";
  import CustomAgentDialog from "./CustomAgentDialog.svelte";
  import ProviderInstallSteps from "./ProviderInstallSteps.svelte";
  import ProviderPlanLimitRings from "./ProviderPlanLimitRings.svelte";
  import UnconfiguredProviderFamily from "./UnconfiguredProviderFamily.svelte";
  import { unconfiguredFamilySituation } from "./provider-install-steps";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getProviderUpdates } from "$lib/stores/chat/provider-updates.svelte";
  import { SETTINGS_PRIMARY_BUTTON_CLASS, SETTINGS_SECONDARY_BUTTON_CLASS } from "$lib/components/settings/action-buttons";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Switch from "$lib/components/ui/Switch.svelte";
  import type { ChatProviderSetupTarget } from "$lib/settings/types";

  let {
    onOpenProviderSetup,
  }: {
    onOpenProviderSetup: (target: ChatProviderSetupTarget) => void;
  } = $props();

  type ProviderDisplayState = "healthy" | "attention" | "disabled" | "unconfigured";

  const localization = getLocalization();
  const { t } = localization;
  const chat = getChat();
  const providerUpdates = getProviderUpdates();
  let selectedFamilyId = $state<string | null>(null);
  let selectedProviderId = $state<string | null>(null);
  let search = $state("");
  /** Whether the list also shows the named agents a person archived. */
  let showArchived = $state(false);
  let busyIds = $state<string[]>([]);
  let refreshingAll = $state(false);
  let refreshResult = $state<ProviderRefreshResult | null>(null);
  let refreshedOnOpen = $state(false);
  let customAgentOpen = $state(false);
  let operationError = $state<string | null>(null);
  let updateFailure = $state<{ instanceId: string; output: string | null } | null>(null);
  let removeId = $state<string | null>(null);
  let directoryScrollElement = $state<HTMLElement>();
  let detailScrollElement = $state<HTMLElement>();
  // Providers and families follow the model picker order, so both lists read the same way.
  const providers = $derived(providersInPickerOrder(chat.settings?.providerInstances ?? [], Date.now()));
  const families = $derived(familiesInPickerOrder((chat.settings?.providerFamilies ?? []).map((family) => ({
    ...family,
    displayName: providerFamilyDisplayName(family, t("chat.providers.acp.name")),
  })), providers, Date.now()));
  const selectedFamily = $derived(
    families.find((family) => family.familyId === selectedFamilyId) ?? families[0] ?? null,
  );
  const familyProviders = $derived(
    selectedFamily
      ? providers.filter((provider) => provider.configuration.familyId === selectedFamily.familyId)
      : [],
  );
  const selectedProvider = $derived(
    familyProviders.find((entry) => entry.configuration.instanceId === selectedProviderId)
      ?? familyProviders[0]
      ?? null,
  );
  const removeProvider = $derived(
    providers.find((entry) => entry.configuration.instanceId === removeId) ?? null,
  );
  const models = $derived.by(() => {
    const query = search.trim().toLocaleLowerCase(localization.locale);
    const collator = new Intl.Collator(localization.locale);
    const familyId = selectedProvider?.configuration.familyId ?? "";
    const listed = listedCatalogModels(familyId, selectedProvider?.modelCatalog?.models ?? [])
      .filter((model) => !query
        || formatModelDisplayName(model.displayName).toLocaleLowerCase(localization.locale).includes(query)
        || model.id.toLocaleLowerCase(localization.locale).includes(query));
    return sortCatalogModels(familyId, listed, collator.compare);
  });
  /** Custom agents are created where providers run, so only a device with local execution offers the dialog. */
  const canCreateAgents = platformHasCapability(BUILD_PLATFORM_PROFILE, "chat.local-execution");
  const roster = $derived(familyAgentRoster(
    selectedProvider?.configuration.familyId ?? "",
    chat.agentIdentities,
    new Intl.Collator(localization.locale).compare,
  ));
  const hasArchivedAgents = $derived(roster.namedAgents.some((agent) => agent.hiddenAt !== null));
  /** The line and named agents whose name holds the search, listed above the catalog models. */
  const listedAgents = $derived.by(() => {
    const query = search.trim().toLocaleLowerCase(localization.locale);
    return [...roster.lineAgents, ...roster.namedAgents].filter((agent) => (
      (showArchived || agent.hiddenAt === null)
      && (!query || agent.participant.displayName.toLocaleLowerCase(localization.locale).includes(query))
    ));
  });

  $effect(() => {
    if (!hasArchivedAgents) showArchived = false;
  });

  $effect(() => {
    if (families.some((family) => family.familyId === selectedFamilyId)) return;
    const healthyFamily = families.find((family) => providers.some((provider) => (
      provider.configuration.familyId === family.familyId
      && provider.configuration.enabled
      && provider.lastProbe?.state === "healthy"
    )));
    const configuredFamily = families.find((family) => providers.some(
      (provider) => provider.configuration.familyId === family.familyId,
    ));
    selectedFamilyId = healthyFamily?.familyId
      ?? configuredFamily?.familyId
      ?? families.find((family) => family.implementationStatus === "available")?.familyId
      ?? families[0]?.familyId
      ?? null;
  });

  $effect(() => {
    if (familyProviders.some((provider) => provider.configuration.instanceId === selectedProviderId)) return;
    selectedProviderId = familyProviders[0]?.configuration.instanceId ?? null;
  });

  $effect(() => {
    void chat.ensureLoaded().catch((error: unknown) => {
      operationError = errorMessage(error);
    });
  });

  $effect(() => {
    if (!chat.settings || refreshedOnOpen) return;
    refreshedOnOpen = true;
    void refreshAll();
  });

  function selectFamily(familyId: string): void {
    if (familyId === selectedFamilyId) return;
    selectedFamilyId = familyId;
    const nextProvider = providers.find((provider) => provider.configuration.familyId === familyId);
    selectedProviderId = nextProvider?.configuration.instanceId ?? null;
    resetCatalogState();
  }

  function selectProvider(familyId: string, instanceId: string): void {
    if (familyId === selectedFamilyId && instanceId === selectedProviderId) return;
    selectedFamilyId = familyId;
    selectedProviderId = instanceId;
    resetCatalogState();
  }

  function resetCatalogState(): void {
    search = "";
    operationError = null;
  }

  function openProviderSetup(familyId: string): void {
    onOpenProviderSetup({ mode: "create", familyId });
  }

  function setBusy(instanceId: string, busy: boolean): void {
    busyIds = busy
      ? [...new Set([...busyIds, instanceId])]
      : busyIds.filter((id) => id !== instanceId);
  }

  async function perform(instanceId: string, action: () => Promise<void>): Promise<void> {
    setBusy(instanceId, true);
    operationError = null;
    try {
      await action();
    } catch (error: unknown) {
      operationError = errorMessage(error);
    } finally {
      setBusy(instanceId, false);
    }
  }

  async function confirmRuntime(): Promise<void> {
    if (!selectedProvider) return;
    const instanceId = selectedProvider.configuration.instanceId;
    await perform(instanceId, async () => {
      await chat.confirmProviderRuntime(instanceId);
    });
  }

  async function refreshAll(): Promise<void> {
    if (refreshingAll) return;
    refreshingAll = true;
    refreshResult = null;
    operationError = null;
    try {
      refreshResult = await chat.refreshAllProviders();
      void providerUpdates.check();
    } catch (error: unknown) {
      operationError = errorMessage(error);
    } finally {
      refreshingAll = false;
    }
  }

  async function refreshProvider(provider: ProviderInstanceRead): Promise<void> {
    const instanceId = provider.configuration.instanceId;
    await perform(instanceId, async () => {
      await chat.probeProvider(instanceId);
      await chat.refreshModels(instanceId);
    });
  }

  async function setEnabled(provider: ProviderInstanceRead, enabled: boolean): Promise<void> {
    const configuration = provider.configuration;
    await perform(configuration.instanceId, () => (
      chat.setProviderEnabled(configuration.instanceId, enabled)
    ));
  }

  async function confirmRemove(): Promise<void> {
    if (!removeProvider) return;
    const instanceId = removeProvider.configuration.instanceId;
    removeId = null;
    await perform(instanceId, async () => {
      const result = await chat.removeProvider(instanceId);
      if (result.credentialCleanupFailed) {
        operationError = t("settings.agents.providers.credentialCleanupFailed");
      }
    });
  }

  /** Turns an agent on or off for channels and mentions. */
  async function setAgentEnabled(agent: ChatAiAgentRead, enabled: boolean): Promise<void> {
    if (!selectedProvider || (agent.participant.archivedAt === null) === enabled) return;
    await perform(selectedProvider.configuration.instanceId, async () => {
      await chatApi.archiveChatAgent(agent.participant.id, agent.participant.revision, !enabled);
      await chat.refreshAgents();
    });
  }

  /** Deletes a named agent that was never used, and archives one that was, which hides it from the list. */
  async function removeAgent(agent: ChatAiAgentRead): Promise<void> {
    if (!selectedProvider) return;
    await perform(selectedProvider.configuration.instanceId, async () => {
      await chatApi.removeCustomChatAgent(agent.participant.id, agent.participant.revision);
      await chat.refreshAgents();
    });
  }

  /** Lists an archived named agent again; it stays off until a person turns it on. */
  async function restoreAgent(agent: ChatAiAgentRead): Promise<void> {
    if (!selectedProvider) return;
    await perform(selectedProvider.configuration.instanceId, async () => {
      await chatApi.restoreCustomChatAgent(agent.participant.id, agent.participant.revision);
      await chat.refreshAgents();
    });
  }

  /** The catalog model whose company icon an agent above the catalog shows. */
  function agentAvatarModel(agent: ChatAiAgentRead): ProviderModel | null {
    if (!selectedProvider) return null;
    const models = selectedProvider.modelCatalog?.models ?? [];
    if (agent.agentKind === "line") {
      return newestModelOfLine(selectedProvider.configuration.familyId, agent.participant.displayName.trim().toLowerCase(), models);
    }
    return models.find((model) => model.id === agent.latestPolicy?.modelId) ?? null;
  }

  function providerStateLabel(provider: ProviderInstanceRead): string {
    if (!provider.configuration.enabled) return t("settings.agents.providers.disabled");
    if (provider.duplicateOf) return t("settings.agents.providers.duplicateShort");
    if (provider.lastProbe?.state === "healthy") {
      if (provider.lastProbe.agentMinimumVersion) return t("settings.agents.providers.agentsNeedUpdate");
      if (availableUpdate(provider)) return t("settings.agents.providers.updateAvailableShort");
      return t("settings.agents.providers.healthy");
    }
    if (provider.lastProbe?.state === "authentication_required") {
      return t("settings.agents.providers.authenticationRequired");
    }
    if (provider.lastProbe?.state === "unsupported_version") return t("settings.agents.providers.unsupported");
    if (!provider.lastProbe) return t("settings.agents.providers.neverChecked");
    return t("settings.agents.providers.unavailable");
  }

  function providerState(provider: ProviderInstanceRead): ProviderDisplayState {
    if (!provider.configuration.enabled) return "disabled";
    if (provider.duplicateOf) return "attention";
    return provider.lastProbe?.state === "healthy" && !provider.lastProbe.agentMinimumVersion
      ? "healthy"
      : "attention";
  }

  /** The sentence explaining a version notice, naming the update command when the family has one. */
  function versionNoticeMessage(notice: ProviderVersionNotice): string {
    if (notice.kind === "too_new") return t("settings.agents.providers.newerThanSupported");
    if (notice.kind === "agents") {
      return notice.updateCommand
        ? t("settings.agents.providers.agentVersionCommand", notice.minimumVersion, notice.updateCommand)
        : t("settings.agents.providers.agentVersion", notice.minimumVersion);
    }
    if (!notice.minimumVersion) return t("settings.agents.providers.olderVersion");
    return notice.updateCommand
      ? t("settings.agents.providers.minimumVersionCommand", notice.minimumVersion, notice.updateCommand)
      : t("settings.agents.providers.minimumVersion", notice.minimumVersion);
  }

  function providersForFamily(familyId: string): ProviderInstanceRead[] {
    return providers.filter((provider) => provider.configuration.familyId === familyId);
  }

  function familyState(family: ProviderFamilyMetadataRead): ProviderDisplayState {
    if (family.implementationStatus !== "available") return "disabled";
    const instances = providersForFamily(family.familyId);
    if (instances.length === 0) return "unconfigured";
    if (instances.some((provider) => providerState(provider) === "healthy")) return "healthy";
    if (instances.every((provider) => providerState(provider) === "disabled")) return "disabled";
    return "attention";
  }

  function familyStateLabel(family: ProviderFamilyMetadataRead): string {
    if (family.implementationStatus !== "available") return t("settings.agents.providers.notAvailable");
    const state = familyState(family);
    if (state === "unconfigured") {
      return unconfiguredFamilySituation(chat.settings?.installedExecutables[family.familyId]).kind === "not-installed"
        ? t("settings.agents.providers.notInstalled")
        : t("settings.agents.providers.notConfigured");
    }
    if (state === "healthy") return t("settings.agents.providers.ready");
    if (state === "disabled") return t("settings.agents.providers.disabled");
    return t("settings.agents.providers.needsAttention");
  }

  function refreshSummary(result: ProviderRefreshResult): string {
    return t(
      "settings.agents.providers.refreshSummary",
      formatNumber(localization.locale, result.providersDiscovered),
      formatNumber(localization.locale, result.issues),
    );
  }

  function errorMessage(error: unknown): string {
    return chatErrorMessage(error, t("settings.agents.providers.operationFailed"));
  }

  function providerCliName(
    family: ProviderFamilyMetadataRead,
    provider: ProviderInstanceRead,
  ): string {
    const executable = provider.configuration.executable.trim().replaceAll("\\", "/");
    return executable.split("/").at(-1)
      || providerFamilyCliName(family);
  }

  function providerFamilyCliName(family: ProviderFamilyMetadataRead): string {
    return family.defaultExecutableCandidates[0] || family.familyId;
  }

  /** The newer release found for `provider`, while its last probe still reads the version the check compared. */
  function availableUpdate(provider: ProviderInstanceRead): ProviderUpdateRead | null {
    const update = providerUpdates.forInstance(provider.configuration.instanceId);
    return update && update.currentVersion === provider.lastProbe?.version ? update : null;
  }

  async function runProviderUpdate(instanceId: string): Promise<void> {
    operationError = null;
    updateFailure = null;
    await perform(instanceId, async () => {
      const result = await providerUpdates.run(instanceId);
      if (!result.successful) {
        updateFailure = { instanceId, output: result.output };
        return;
      }
      await chat.probeProvider(instanceId);
    });
  }

  function providerCliSummary(
    family: ProviderFamilyMetadataRead,
    provider: ProviderInstanceRead,
  ): string {
    const cliName = providerCliName(family, provider);
    return provider.lastProbe?.version ? `${cliName} ${provider.lastProbe.version}` : cliName;
  }
</script>

<section class="provider-settings" data-agents-settings-subsection="providers">
  {#if chat.loading}
    <Loading layout="rows" rows={3} />
  {:else if families.length === 0}
    <p class="empty-copy">{t("settings.agents.providers.noIntegrations")}</p>
  {:else}
    <div class="directory-layout">
      <aside class="directory-panel">
        <div class="scroll-frame">
          <div bind:this={directoryScrollElement} data-mobile-scroll-fade class="directory-scroll hide-scrollbar">
            <nav class="provider-directory" aria-label={t("settings.agents.providers.familyTabs")}>
              {#each families as family (family.familyId)}
                {@const instances = providersForFamily(family.familyId)}
                {#if instances.length === 0}
                  {@const active = family.familyId === selectedFamily?.familyId && !selectedProvider}
                  {@const state = familyState(family)}
                  <button
                    type="button"
                    class="provider-row"
                    class:active
                    aria-current={active ? "page" : undefined}
                    onclick={() => selectFamily(family.familyId)}
                  >
                    <ChatModelAvatar familyId={family.familyId} label={family.displayName} size={32} />
                    <span class="directory-summary">
                      <strong>{family.displayName}</strong>
                      <small data-state={state}><i></i>{familyStateLabel(family)}</small>
                    </span>
                  </button>
                {:else}
                  {#each instances as provider (provider.configuration.instanceId)}
                    {@const instanceId = provider.configuration.instanceId}
                    {@const active = instanceId === selectedProvider?.configuration.instanceId}
                    <button
                      type="button"
                      class="provider-row"
                      class:active
                      aria-current={active ? "page" : undefined}
                      onclick={() => selectProvider(family.familyId, instanceId)}
                    >
                      <ChatModelAvatar familyId={family.familyId} label={family.displayName} size={32} />
                      <span class="directory-summary">
                        <strong>{provider.configuration.label}</strong>
                        <small data-state={providerState(provider)}><i></i>{providerStateLabel(provider)}</small>
                      </span>
                    </button>
                  {/each}
                {/if}
              {/each}
            </nav>
          </div>
          <CalendarScrollbar scrollContainer={directoryScrollElement} wheelPassthrough />
        </div>
      </aside>

      <div class="detail-panel">
        {#if selectedFamily}
          <div class="detail-scroll-frame">
            <div bind:this={detailScrollElement} data-mobile-scroll-fade class="detail-scroll hide-scrollbar">
              <div class="editor-heading">
                <ChatModelAvatar
                  familyId={selectedFamily.familyId}
                  label={selectedFamily.displayName}
                  size={38}
                />
                <div class="editor-title">
                  <div class="editor-identity">
                    <div class="editor-label-line">
                      <h3>{selectedProvider?.configuration.label ?? selectedFamily.displayName}</h3>
                      <span
                        class="editor-state"
                        data-state={selectedProvider ? providerState(selectedProvider) : familyState(selectedFamily)}
                        aria-label={selectedProvider ? providerStateLabel(selectedProvider) : familyStateLabel(selectedFamily)}
                        data-app-tooltip={selectedProvider ? providerStateLabel(selectedProvider) : familyStateLabel(selectedFamily)}
                      ><i></i></span>
                    </div>
                    <div class="editor-subtitle-line">
                      <p>
                        {selectedProvider
                          ? providerCliSummary(selectedFamily, selectedProvider)
                          : providerFamilyCliName(selectedFamily)}
                      </p>
                      {#if selectedProvider?.lastProbe?.accountLabel}
                        <button
                          type="button"
                          class="editor-account"
                          aria-label={`${t("settings.agents.providers.account")}: ${selectedProvider.lastProbe.accountLabel}`}
                          data-app-tooltip={selectedProvider.lastProbe.accountLabel}
                          data-app-tooltip-keep-on-click="true"
                        ><Mail size={12} strokeWidth={1.8} /></button>
                      {/if}
                      {#if selectedProvider}<ProviderPlanLimitRings provider={selectedProvider} />{/if}
                      {#if selectedProvider?.lastProbe?.authSource}
                        {@const authSourceLabel = selectedProvider.lastProbe.authSource === "api_key"
                          ? t("settings.agents.providers.authSourceApiKey")
                          : t("settings.agents.providers.authSourceAccountLogin")}
                        <button
                          type="button"
                          class="editor-account"
                          aria-label={`${t("settings.agents.providers.authSource")}: ${authSourceLabel}`}
                          data-app-tooltip={authSourceLabel}
                          data-app-tooltip-keep-on-click="true"
                        >
                          {#if selectedProvider.lastProbe.authSource === "api_key"}
                            <KeyRound size={12} strokeWidth={1.8} />
                          {:else}
                            <UserRound size={12} strokeWidth={1.8} />
                          {/if}
                        </button>
                      {/if}
                    </div>
                  </div>
                  {#if selectedProvider}
                    {@const headerInstanceId = selectedProvider.configuration.instanceId}
                    {@const headerBusy = refreshingAll || busyIds.includes(headerInstanceId)}
                    <div class="header-actions">
                      <Switch
                        checked={selectedProvider.configuration.enabled}
                        ariaLabel={selectedProvider.configuration.enabled
                          ? t("settings.agents.providers.disable")
                          : t("settings.agents.providers.enable")}
                        disabled={headerBusy}
                        onChange={(enabled) => void setEnabled(selectedProvider, enabled)}
                      />
                      <button
                        type="button"
                        class="icon-action"
                        aria-label={t("settings.agents.providers.configure")}
                        data-app-tooltip={t("settings.agents.providers.configure")}
                        onclick={() => onOpenProviderSetup({ mode: "edit", instanceId: headerInstanceId })}
                      >
                        <Pencil size={14} />
                      </button>
                      <button
                        type="button"
                        class="icon-action"
                        disabled={headerBusy}
                        aria-label={t("settings.agents.providers.refresh")}
                        data-app-tooltip={t("settings.agents.providers.refresh")}
                        onclick={() => void refreshProvider(selectedProvider)}
                      >
                        <RefreshCw size={14} class={headerBusy ? "animate-spin" : undefined} />
                      </button>
                      <button
                        type="button"
                        class="icon-action destructive"
                        disabled={headerBusy}
                        aria-label={t("settings.agents.providers.remove")}
                        data-app-tooltip={t("settings.agents.providers.remove")}
                        onclick={() => { removeId = headerInstanceId; }}
                      >
                        <Trash2 size={14} />
                      </button>
                    </div>
                  {/if}
                </div>
              </div>

              {#if refreshResult && (refreshResult.providersDiscovered > 0 || refreshResult.issues > 0)}
                <p class="refresh-summary" class:attention={refreshResult.issues > 0} role="status">
                  {refreshSummary(refreshResult)}
                </p>
              {/if}
              {#if operationError}<Notice class="operation-error" message={operationError} />{/if}

              {#if selectedProvider}
                {@const instanceId = selectedProvider.configuration.instanceId}
                {@const selectedBusy = refreshingAll || busyIds.includes(instanceId)}
                {@const versionNotice = providerVersionNotice(selectedProvider.lastProbe, selectedFamily)}
                {@const update = availableUpdate(selectedProvider)}
                {@const duplicateLabel = selectedProvider.duplicateOf
                  ? providers.find((candidate) => candidate.configuration.instanceId === selectedProvider.duplicateOf)?.configuration.label
                  : undefined}
                <div class="editor-content">
                  {#if duplicateLabel}
                    <Notice
                      tone="warning"
                      role="status"
                      message={t("settings.agents.providers.duplicateNotice", duplicateLabel)}
                    />
                  {/if}
                  {#if versionNotice}
                    <Notice
                      tone="warning"
                      role="status"
                      message={versionNoticeMessage(versionNotice)}
                      copy={versionNotice.kind === "too_new" ? false : versionNotice.updateCommand ?? false}
                    />
                  {/if}
                  {#if update && !versionNotice}
                    <div class="update-notice">
                      {#if update.runsInApp}
                        <button
                          type="button"
                          class={SETTINGS_SECONDARY_BUTTON_CLASS}
                          disabled={selectedBusy || providerUpdates.running !== null}
                          onclick={() => void runProviderUpdate(instanceId)}
                        >{providerUpdates.running === instanceId
                          ? t("settings.agents.providers.updating")
                          : t("settings.agents.providers.updateAction")}</button>
                      {/if}
                      <Notice
                        class="min-w-0 flex-1"
                        tone="neutral"
                        role="status"
                        message={update.runsInApp
                          ? t("settings.agents.providers.updateAvailable", update.latestVersion)
                          : t("settings.agents.providers.updateAvailableManaged", update.latestVersion)}
                        copy={update.runsInApp ? update.updateCommand : false}
                      />
                    </div>
                  {/if}
                  {#if updateFailure?.instanceId === instanceId}
                    {@const command = update?.updateCommand ?? selectedFamily.updateCommand ?? ""}
                    <Notice
                      tone="warning"
                      message={t("settings.agents.providers.updateFailed", providerCliName(selectedFamily, selectedProvider), command)}
                      copy={updateFailure.output ?? command}
                      onDismiss={() => (updateFailure = null)}
                    />
                  {/if}
                  {#if selectedFamily.installGuide && selectedProvider.lastProbe?.state === "executable_missing"}
                    <ProviderInstallSteps
                      family={selectedFamily}
                      guide={selectedFamily.installGuide}
                      situation={{ kind: "missing-at", path: selectedProvider.configuration.executable }}
                      onRefresh={() => refreshProvider(selectedProvider)}
                    />
                  {:else if selectedFamily.installGuide && selectedProvider.lastProbe?.state === "authentication_required"}
                    <ProviderInstallSteps
                      family={selectedFamily}
                      guide={selectedFamily.installGuide}
                      situation={{ kind: "signed-out" }}
                      onRefresh={() => refreshProvider(selectedProvider)}
                    />
                  {/if}
                  {#if selectedProvider.runtimeReview.length > 0}
                    <section class="provider-notice runtime-review">
                      <p>{t("settings.agents.setup.runtimeReviewNotice")}</p>
                      <ul>
                        {#each selectedProvider.runtimeReview as item, index (index)}
                          <li>{item.label}</li>
                        {/each}
                      </ul>
                      <button type="button" class="compact-action" disabled={selectedBusy} onclick={() => void confirmRuntime()}>
                        {t("settings.agents.setup.runtimeReviewButton")}
                      </button>
                    </section>
                  {/if}

                  <section class="editor-section catalog-section">
                    <div class="model-search">
                      <label>
                        <Search size={14} />
                        <input
                          type="search"
                          bind:value={search}
                          aria-label={t("settings.agents.models.search")}
                          placeholder={t("settings.agents.models.search")}
                        />
                      </label>
                    </div>

                    {#if selectedProvider.modelCatalog?.stale}
                      <p class="catalog-warning">{t("settings.agents.models.catalogStale")}</p>
                    {/if}

                    {#if models.length === 0 && listedAgents.length === 0}
                      <p class="models-empty">
                        {search.trim() ? t("settings.agents.models.noSearchResults") : t("settings.agents.models.empty")}
                      </p>
                    {:else}
                      <div class="model-list">
                        {#each listedAgents as agent (agent.participant.id)}
                          {@const agentCompany = modelCompany(selectedProvider.configuration.familyId, agentAvatarModel(agent))}
                          <div class="model-row" class:archived={agent.hiddenAt !== null}>
                            <ChatModelAvatar familyId={agentCompany.iconFamilyId} label={agentCompany.name} size={25} />
                            <div class="model-identity">
                              <span class="model-name">@{agent.participant.displayName}</span>
                              {#if agent.agentKind === "line"}
                                <span
                                  class="latest-mark"
                                  role="img"
                                  aria-label={t("settings.agents.roster.latest")}
                                  data-app-tooltip={t("settings.agents.roster.latest")}
                                ><CircleQuestionMark size={14} strokeWidth={1.8} aria-hidden="true" /></span>
                              {/if}
                            </div>
                            <div class="model-actions">
                              <Switch
                                checked={agent.participant.archivedAt === null}
                                ariaLabel={`@${agent.participant.displayName}`}
                                disabled={selectedBusy}
                                onChange={(enabled) => void setAgentEnabled(agent, enabled)}
                              />
                              {@render agentRemoveAction(agent, selectedBusy)}
                            </div>
                          </div>
                        {/each}
                        {#each models as model (model.id)}
                          {@const modelAgent = roster.modelAgents.get(model.id)}
                          {@const modelName = modelAgent?.participant.displayName ?? formatModelDisplayName(model.displayName)}
                          {@const company = modelCompany(selectedProvider.configuration.familyId, model)}
                          <div class="model-row">
                            <ChatModelAvatar familyId={company.iconFamilyId} label={company.name} size={25} />
                            <div class="model-identity">
                              <span class="model-name">@{modelName}</span>
                            </div>
                            <div class="model-actions">
                              {#if modelAgent}
                                <Switch
                                  checked={modelAgent.participant.archivedAt === null}
                                  ariaLabel={`@${modelName}`}
                                  disabled={selectedBusy}
                                  onChange={(enabled) => void setAgentEnabled(modelAgent, enabled)}
                                />
                              {/if}
                              {@render agentRemoveAction(modelAgent, selectedBusy)}
                            </div>
                          </div>
                        {/each}
                      </div>
                    {/if}
                  </section>
                </div>
              {:else if selectedFamily.implementationStatus === "available"}
                <UnconfiguredProviderFamily
                  family={selectedFamily}
                  installedExecutables={chat.settings?.installedExecutables[selectedFamily.familyId]}
                  onSetUp={() => openProviderSetup(selectedFamily.familyId)}
                />
              {:else}
                <section class="empty-configuration">
                  <p>{selectedFamily.unavailableReason ?? t("settings.agents.providers.notAvailable")}</p>
                </section>
              {/if}
            </div>
            <CalendarScrollbar scrollContainer={detailScrollElement} wheelPassthrough />
          </div>
        {/if}
        <footer class="detail-footer">
          {#if selectedFamily && selectedProvider && selectedFamily.implementationStatus === "available"}
            {@const familyId = selectedFamily.familyId}
            <button type="button" class="add-configuration-text" onclick={() => openProviderSetup(familyId)}>
              {t("settings.agents.providers.addAnother")}
            </button>
          {/if}
          {#if hasArchivedAgents}
            {@const archivedLabel = showArchived
              ? t("settings.agents.roster.hideArchived")
              : t("settings.agents.roster.showArchived")}
            <button
              type="button"
              class="icon-action archived-toggle"
              aria-pressed={showArchived}
              aria-label={archivedLabel}
              data-app-tooltip={archivedLabel}
              onclick={() => { showArchived = !showArchived; }}
            >
              {#if showArchived}<Eye size={14} />{:else}<EyeOff size={14} />{/if}
            </button>
          {/if}
          {#if canCreateAgents && selectedProvider}
            <button
              type="button"
              class="custom-agent-action {SETTINGS_PRIMARY_BUTTON_CLASS}"
              disabled={refreshingAll || busyIds.includes(selectedProvider.configuration.instanceId)}
              onclick={() => { customAgentOpen = true; }}
            ><Plus size={14} />{t("settings.agents.roster.add")}</button>
          {/if}
        </footer>
      </div>
    </div>
  {/if}
</section>

{#snippet agentRemoveAction(agent: ChatAiAgentRead | undefined, busy: boolean)}
  {#if agent && isRemovableAgent(agent) && agent.hiddenAt !== null}
    <button
      type="button"
      class="icon-action"
      disabled={busy}
      aria-label={t("settings.agents.roster.restore")}
      data-app-tooltip={t("settings.agents.roster.restore")}
      onclick={() => void restoreAgent(agent)}
    >
      <ArchiveRestore size={14} />
    </button>
  {:else if agent && isRemovableAgent(agent)}
    <button
      type="button"
      class="icon-action destructive"
      disabled={busy}
      aria-label={t("settings.agents.roster.remove")}
      data-app-tooltip={t("settings.agents.roster.remove")}
      onclick={() => void removeAgent(agent)}
    >
      <Trash2 size={14} />
    </button>
  {:else}
    <!-- aria-disabled keeps hover events, so the built-in tooltip still shows. -->
    <button
      type="button"
      class="icon-action destructive"
      aria-disabled="true"
      aria-label={t("settings.agents.roster.builtIn")}
      data-app-tooltip={t("settings.agents.roster.builtIn")}
    >
      <Trash2 size={14} />
    </button>
  {/if}
{/snippet}

{#if removeProvider}
  <ConfirmDialog
    title={t("settings.agents.providers.removeTitle")}
    message={t("settings.agents.providers.removeMessage", removeProvider.configuration.label)}
    confirmLabel={t("settings.agents.providers.remove")}
    cancelLabel={t("chat.cancel")}
    onConfirm={() => void confirmRemove()}
    onCancel={() => { removeId = null; }}
  />
{/if}

{#if customAgentOpen && selectedProvider}
  <CustomAgentDialog provider={selectedProvider} onClose={() => { customAgentOpen = false; }} />
{/if}

<style>
  .provider-settings { display:grid; height:100%; min-height:0; grid-template-rows:minmax(0,1fr); }
  .compact-action { display:inline-flex; min-height:1.9rem; align-items:center; justify-content:center; gap:0.35rem; border:1px solid var(--border); border-radius:0.42rem; background:var(--background); padding:0.3rem 0.65rem; color:var(--foreground); font-size:calc(0.733333rem * var(--type-scale)); font-weight:600; line-height:1; white-space:nowrap; }
  .compact-action:hover:not(:disabled) { background:var(--accent); }
  .empty-copy { padding:0.7rem 0.25rem; color:var(--muted-foreground); font-size:calc(0.72rem * var(--type-scale)); }
  .directory-layout { display:grid; min-height:0; isolation:isolate; grid-template-columns:minmax(12.5rem,0.62fr) minmax(0,1.6fr); }
  .directory-panel { position:relative; z-index:1; min-width:0; min-height:0; padding-right:0.75rem; }
  .scroll-frame,.detail-scroll-frame { position:relative; height:100%; min-height:0; }
  .directory-scroll,.detail-scroll { height:100%; overflow-y:auto; overscroll-behavior:contain; }
  .provider-directory { display:grid; align-content:start; gap:0; padding:0.15rem 0.2rem 0.15rem 0; }
  .provider-row { display:grid; min-width:0; grid-template-columns:auto minmax(0,1fr); align-items:center; gap:0.55rem; margin-inline:0.2rem; border-radius:0.5rem; padding:0.6rem; text-align:left; }
  .provider-row:hover,.provider-row.active { background:var(--accent); color:var(--accent-foreground); }
  .directory-summary { display:grid; min-width:0; }
  .directory-summary strong,.directory-summary small { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .directory-summary strong { font-size:calc(0.8rem * var(--type-scale)); font-weight:600; }
  .directory-summary small { display:flex; min-width:0; align-items:center; gap:0.3rem; margin-top:0.05rem; color:var(--muted-foreground); font-size:calc(0.65rem * var(--type-scale)); }
  .directory-summary small i,.editor-state i { width:0.38rem; height:0.38rem; flex:0 0 auto; border-radius:50%; background:var(--status-tentative); }
  .directory-summary small[data-state="healthy"] i,.editor-state[data-state="healthy"] i { background:var(--action-confirm-signal); }
  .directory-summary small[data-state="disabled"] i,.editor-state[data-state="disabled"] i { background:var(--muted-foreground); opacity:0.7; }
  .directory-summary small[data-state="unconfigured"] i,.editor-state[data-state="unconfigured"] i { background:var(--destructive); }
  .detail-panel { position:relative; z-index:2; display:grid; min-width:0; min-height:0; grid-template-rows:minmax(0,1fr) auto; border-left:1px solid var(--border); padding-left:1rem; }
  .detail-footer { display:flex; min-width:0; flex-wrap:wrap; align-items:center; justify-content:space-between; gap:0.5rem 0.75rem; border-top:1px solid var(--border); padding:0.65rem 0.75rem 0.65rem 0; }
  .custom-agent-action,.archived-toggle { margin-left:auto; }
  .archived-toggle + .custom-agent-action { margin-left:0; }
  .model-row.archived .model-name { color:var(--muted-foreground); }
  .detail-scroll { display:grid; grid-template-columns:minmax(0,1fr); align-content:start; gap:1rem; padding:0.2rem 0.75rem 1rem 0; }
  .editor-heading { display:grid; min-width:0; grid-template-columns:auto minmax(0,1fr); align-items:center; gap:0.65rem; padding-inline:0.25rem; }
  .editor-title { display:grid; min-width:0; grid-template-columns:minmax(0,1fr) auto; align-items:center; gap:0.75rem; }
  .editor-identity { display:grid; min-width:0; }
  .editor-label-line,.editor-subtitle-line { display:flex; min-width:0; align-items:center; }
  .editor-label-line { gap:0.3rem; }
  .editor-subtitle-line { gap:0.65rem; }
  .editor-heading h3 { overflow:hidden; font-size:calc(0.833333rem * var(--type-scale)); font-weight:600; text-overflow:ellipsis; white-space:nowrap; }
  .editor-heading p { overflow:hidden; min-width:0; margin-top:0.08rem; color:var(--muted-foreground); font-size:calc(0.7rem * var(--type-scale)); text-overflow:ellipsis; white-space:nowrap; }
  .editor-state { display:inline-grid; width:0.7rem; height:0.7rem; flex:0 0 auto; place-items:center; }
  .editor-account { display:inline-grid; width:1rem; height:1rem; flex:0 0 auto; place-items:center; border-radius:0.2rem; color:var(--muted-foreground); }
  .editor-account:hover,.editor-account:focus-visible { color:var(--foreground); }
  .header-actions { display:flex; flex-shrink:0; align-items:center; gap:0.3rem; }
  .refresh-summary { border-left:2px solid var(--action-confirm-signal); padding:0.15rem 0.25rem 0.15rem 0.65rem; font-size:calc(0.68rem * var(--type-scale)); }
  .detail-scroll > :global(.operation-error) { padding:0.15rem 0.25rem 0.15rem 0.65rem; font-size:calc(0.68rem * var(--type-scale)); }
  .refresh-summary { color:var(--muted-foreground); }
  .refresh-summary.attention,.catalog-warning,.provider-notice { color:var(--status-tentative); }
  .refresh-summary.attention { border-left-color:var(--destructive); }
  .editor-content { display:grid; align-content:start; gap:1rem; }
  /* Notices match the size of the provider details around them. */
  .editor-content :global(.notice) { font-size:0.8rem; }
  /* The update action leads its notice, and both center on the button's height. */
  .update-notice { display:flex; min-width:0; align-items:center; gap:0.625rem; }
  .runtime-review { display:grid; gap:0.4rem; }
  .runtime-review ul { display:grid; gap:0.2rem; padding-left:1rem; list-style:disc; overflow-wrap:anywhere; }
  .runtime-review button { justify-self:start; }
  .editor-section { display:grid; min-width:0; gap:0.65rem; }
  .catalog-section { gap:0; }
  .icon-action { display:inline-grid; width:1.75rem; height:1.75rem; place-items:center; border:1px solid var(--border); border-radius:0.375rem; background:var(--background); color:var(--muted-foreground); }
  .icon-action:hover:not(:disabled, [aria-disabled="true"]) { background:var(--accent); color:var(--foreground); }
  .icon-action.destructive { color:var(--destructive); }
  .icon-action[aria-disabled="true"] { cursor:not-allowed; opacity:0.5; }
  .provider-notice,.catalog-warning { padding:0.25rem; font-size:calc(0.68rem * var(--type-scale)); }
  .model-search { display:flex; min-width:0; align-items:center; gap:0.45rem; border-block:1px solid var(--border); padding:0.5rem 0.25rem; color:var(--muted-foreground); }
  .model-search label { display:grid; min-width:0; flex:1; grid-template-columns:auto minmax(0,1fr); align-items:center; gap:0.45rem; }
  .model-search input { min-width:0; background:transparent; color:var(--foreground); font-size:calc(0.75rem * var(--type-scale)); outline:none; }
  .model-search:focus-within { color:var(--foreground); }
  .model-list { display:grid; }
  .model-row { display:grid; grid-template-columns:auto minmax(0,1fr) auto; align-items:center; gap:0.65rem; border-bottom:1px solid var(--border); padding:0.62rem 0.25rem; }
  .model-identity { display:flex; min-width:0; align-items:center; gap:0.35rem; }
  .latest-mark { display:inline-grid; flex:0 0 auto; place-items:center; color:var(--muted-foreground); }
  .model-name { overflow:hidden; color:var(--foreground); font-size:calc(0.8rem * var(--type-scale)); font-weight:550; line-height:1.1rem; text-overflow:ellipsis; white-space:nowrap; }
  .model-actions { display:flex; flex-shrink:0; align-items:center; gap:0.45rem; }
  .models-empty { padding:0.9rem 0.25rem; color:var(--muted-foreground); font-size:calc(0.75rem * var(--type-scale)); }
  .add-configuration-text { min-width:0; padding-inline:0.25rem; color:var(--muted-foreground); font-size:calc(0.7rem * var(--type-scale)); font-weight:600; text-align:left; }
  .add-configuration-text:hover { color:var(--foreground); text-decoration:underline; text-underline-offset:0.18rem; }
  .empty-configuration { display:grid; justify-items:start; gap:0.7rem; padding:0.25rem; color:var(--muted-foreground); font-size:calc(0.72rem * var(--type-scale)); }
  button:disabled { cursor:not-allowed; opacity:0.5; }
  :global(.dark) .compact-action,:global(.dark) .icon-action { background:transparent; }
  @media (max-width:700px) {
    .directory-layout { grid-template-columns:1fr; grid-template-rows:minmax(8rem,30%) minmax(0,1fr); }
    .directory-panel { border-bottom:1px solid var(--border); padding:0 0 0.75rem; }
    .detail-panel { border-left:0; padding:0.8rem 0 0; }
    .detail-scroll { padding-right:0.35rem; }
    .detail-footer { padding-right:0; }
  }
  @media (max-width:540px) {
    .model-actions { gap:0.2rem; }
  }
</style>
