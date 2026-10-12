<script lang="ts">
  import Mail from "@lucide/svelte/icons/mail";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Plus from "@lucide/svelte/icons/plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Search from "@lucide/svelte/icons/search";
  import Star from "@lucide/svelte/icons/star";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import type {
    JsonValue,
    ProviderFamilyMetadataRead,
    ProviderInstanceRead,
    ProviderRefreshResult,
  } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import {
    compareCompanyModels,
    formatModelDisplayName,
    modelCompany,
    shouldShowModelId,
  } from "$lib/chat/composer/model-company";
  import ChatModelAvatar from "$lib/components/chat/identity/ChatModelAvatar.svelte";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
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
  let selectedFamilyId = $state<string | null>(null);
  let selectedProviderId = $state<string | null>(null);
  let search = $state("");
  let busyIds = $state<string[]>([]);
  let refreshingAll = $state(false);
  let refreshResult = $state<ProviderRefreshResult | null>(null);
  let refreshedOnOpen = $state(false);
  let customOpen = $state(false);
  let customId = $state("");
  let customLabel = $state("");
  let customError = $state<string | null>(null);
  let operationError = $state<string | null>(null);
  let removeId = $state<string | null>(null);
  let directoryScrollElement = $state<HTMLElement>();
  let detailScrollElement = $state<HTMLElement>();
  const families = $derived(chat.settings?.providerFamilies ?? []);
  const providers = $derived(chat.settings?.providerInstances ?? []);
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
    const favorites = new Set(selectedProvider?.configuration.favoriteModelIds ?? []);
    const collator = new Intl.Collator(localization.locale);
    const familyId = selectedProvider?.configuration.familyId ?? "";
    return [...(selectedProvider?.modelCatalog?.models ?? [])]
      .filter((model) => !query
        || formatModelDisplayName(model.displayName).toLocaleLowerCase(localization.locale).includes(query)
        || model.id.toLocaleLowerCase(localization.locale).includes(query))
      .sort((left, right) => {
        const favoriteOrder = Number(favorites.has(right.id)) - Number(favorites.has(left.id));
        if (favoriteOrder !== 0) return favoriteOrder;
        const leftCompany = modelCompany(familyId, left);
        const rightCompany = modelCompany(familyId, right);
        return leftCompany.order - rightCompany.order
          || (leftCompany.id === rightCompany.id ? compareCompanyModels(leftCompany.id, left, right) : 0)
          || collator.compare(left.displayName, right.displayName);
      });
  });
  const acceptsCustomModels = $derived(
    selectedProvider?.configuration.familyId === "codex" && providerConfigBoolean("allowCustomModels"),
  );

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
    customOpen = false;
    customError = null;
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

  async function refreshAll(): Promise<void> {
    if (refreshingAll) return;
    refreshingAll = true;
    refreshResult = null;
    operationError = null;
    try {
      refreshResult = await chat.refreshAllProviders();
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
        operationError = t("settings.chat.providers.credentialCleanupFailed");
      }
    });
  }

  async function setVisible(modelId: string, shouldBeVisible: boolean): Promise<void> {
    if (!selectedProvider) return;
    const allIds = selectedProvider.modelCatalog?.models.map((model) => model.id) ?? [];
    const configured = selectedProvider.configuration.visibleModelIds;
    const current = configured.length === 0 ? allIds : configured;
    const visible = shouldBeVisible
      ? [...new Set([...current, modelId])]
      : current.filter((id) => id !== modelId);
    await perform(selectedProvider.configuration.instanceId, () => (
      chat.updateModels(
        selectedProvider.configuration.instanceId,
        visible,
        selectedProvider.configuration.favoriteModelIds,
      )
    ));
  }

  async function toggleFavorite(modelId: string): Promise<void> {
    if (!selectedProvider) return;
    const favorites = selectedProvider.configuration.favoriteModelIds.includes(modelId)
      ? selectedProvider.configuration.favoriteModelIds.filter((id) => id !== modelId)
      : [...selectedProvider.configuration.favoriteModelIds, modelId];
    await perform(selectedProvider.configuration.instanceId, () => (
      chat.updateModels(
        selectedProvider.configuration.instanceId,
        selectedProvider.configuration.visibleModelIds,
        favorites,
      )
    ));
  }

  function isVisible(modelId: string): boolean {
    if (!selectedProvider) return false;
    return selectedProvider.configuration.visibleModelIds.length === 0
      || selectedProvider.configuration.visibleModelIds.includes(modelId);
  }

  function providerConfigBoolean(key: string): boolean {
    const value = selectedProvider?.configuration.providerConfig.value;
    return typeof value === "object" && value !== null && !Array.isArray(value) && value[key] === true;
  }

  function providerConfigRecord(): Record<string, JsonValue> {
    const value = selectedProvider?.configuration.providerConfig.value;
    return typeof value === "object" && value !== null && !Array.isArray(value) ? { ...value } : {};
  }

  function stringRecord(value: JsonValue | undefined): Record<string, string> {
    if (typeof value !== "object" || value === null || Array.isArray(value)) return {};
    return Object.fromEntries(
      Object.entries(value).filter((entry): entry is [string, string] => typeof entry[1] === "string"),
    );
  }

  async function saveCustomModel(): Promise<void> {
    if (!selectedProvider || !acceptsCustomModels) return;
    const id = customId.trim();
    const label = customLabel.trim();
    if (!id || id.length > 256 || [...id].some((character) => /\p{Control}/u.test(character))) {
      customError = t("settings.chat.models.customId");
      return;
    }
    const config = providerConfigRecord();
    const existingIds = Array.isArray(config.customModelIds)
      ? config.customModelIds.filter((value): value is string => typeof value === "string")
      : [];
    const existingLabels = stringRecord(config.customModelLabels);
    config.customModelIds = [...new Set([...existingIds, id])];
    config.customModelLabels = { ...existingLabels, ...(label ? { [id]: label } : {}) };
    customError = null;
    await perform(selectedProvider.configuration.instanceId, async () => {
      await chat.saveProvider({
        ...selectedProvider.configuration,
        providerConfig: { ...selectedProvider.configuration.providerConfig, value: config },
      });
      await chat.probeProvider(selectedProvider.configuration.instanceId);
      await chat.refreshModels(selectedProvider.configuration.instanceId);
      customId = "";
      customLabel = "";
      customOpen = false;
    });
  }

  async function removeCustomModel(modelId: string): Promise<void> {
    if (!selectedProvider || !acceptsCustomModels) return;
    const config = providerConfigRecord();
    const customIds = Array.isArray(config.customModelIds)
      ? config.customModelIds.filter((value): value is string => typeof value === "string" && value !== modelId)
      : [];
    const labels = stringRecord(config.customModelLabels);
    delete labels[modelId];
    config.customModelIds = customIds;
    config.customModelLabels = labels;
    await perform(selectedProvider.configuration.instanceId, async () => {
      await chat.saveProvider({
        ...selectedProvider.configuration,
        visibleModelIds: selectedProvider.configuration.visibleModelIds.filter((id) => id !== modelId),
        favoriteModelIds: selectedProvider.configuration.favoriteModelIds.filter((id) => id !== modelId),
        providerConfig: { ...selectedProvider.configuration.providerConfig, value: config },
      });
      await chat.probeProvider(selectedProvider.configuration.instanceId);
      await chat.refreshModels(selectedProvider.configuration.instanceId);
    });
  }

  function providerStateLabel(provider: ProviderInstanceRead): string {
    if (!provider.configuration.enabled) return t("settings.chat.providers.disabled");
    if (provider.lastProbe?.state === "healthy") return t("settings.chat.providers.healthy");
    if (provider.lastProbe?.state === "authentication_required") {
      return t("settings.chat.providers.authenticationRequired");
    }
    if (provider.lastProbe?.state === "unsupported_version") return t("settings.chat.providers.unsupported");
    if (!provider.lastProbe) return t("settings.chat.providers.neverChecked");
    return t("settings.chat.providers.unavailable");
  }

  function providerState(provider: ProviderInstanceRead): ProviderDisplayState {
    if (!provider.configuration.enabled) return "disabled";
    return provider.lastProbe?.state === "healthy" ? "healthy" : "attention";
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
    if (family.implementationStatus !== "available") return t("settings.chat.providers.notAvailable");
    const state = familyState(family);
    if (state === "unconfigured") return t("settings.chat.providers.notConfigured");
    if (state === "healthy") return t("settings.chat.providers.ready");
    if (state === "disabled") return t("settings.chat.providers.disabled");
    return t("settings.chat.providers.needsAttention");
  }

  function refreshSummary(result: ProviderRefreshResult): string {
    return t(
      "settings.chat.providers.refreshSummary",
      formatNumber(localization.locale, result.providersDiscovered),
      formatNumber(localization.locale, result.issues),
    );
  }

  function errorMessage(error: unknown): string {
    return chatErrorMessage(error, t("settings.chat.providers.operationFailed"));
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

  function providerCliSummary(
    family: ProviderFamilyMetadataRead,
    provider: ProviderInstanceRead,
  ): string {
    const cliName = providerCliName(family, provider);
    return provider.lastProbe?.version ? `${cliName} ${provider.lastProbe.version}` : cliName;
  }
</script>

<section class="provider-settings" data-chat-settings-subsection="providers">
  <header class="directory-header">
    <div>
      <h2>{t("settings.chat.providers.heading")}</h2>
    </div>
    <button type="button" class="settings-button" disabled={refreshingAll} onclick={() => void refreshAll()}>
      <RefreshCw size={13} class={refreshingAll ? "animate-spin" : undefined} />
      {refreshingAll ? t("settings.chat.providers.scanning") : t("settings.chat.providers.refreshAll")}
    </button>
  </header>

  {#if chat.loading}
    <p class="empty-copy" role="status">{t("common.loading")}</p>
  {:else if families.length === 0}
    <p class="empty-copy">{t("settings.chat.providers.noIntegrations")}</p>
  {:else}
    <div class="directory-layout">
      <aside class="directory-panel">
        <div class="scroll-frame">
          <div bind:this={directoryScrollElement} class="directory-scroll hide-scrollbar">
            <nav class="provider-directory" aria-label={t("settings.chat.providers.familyTabs")}>
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
            <div bind:this={detailScrollElement} class="detail-scroll hide-scrollbar">
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
                          aria-label={`${t("settings.chat.providers.account")}: ${selectedProvider.lastProbe.accountLabel}`}
                          data-app-tooltip={selectedProvider.lastProbe.accountLabel}
                          data-app-tooltip-keep-on-click="true"
                        ><Mail size={12} strokeWidth={1.8} /></button>
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
                          ? t("settings.chat.providers.disable")
                          : t("settings.chat.providers.enable")}
                        disabled={headerBusy}
                        onChange={(enabled) => void setEnabled(selectedProvider, enabled)}
                      />
                      <button
                        type="button"
                        class="icon-action"
                        aria-label={t("settings.chat.providers.configure")}
                        data-app-tooltip={t("settings.chat.providers.configure")}
                        onclick={() => onOpenProviderSetup({ mode: "edit", instanceId: headerInstanceId })}
                      >
                        <Pencil size={14} />
                      </button>
                      <button
                        type="button"
                        class="icon-action"
                        disabled={headerBusy}
                        aria-label={t("settings.chat.providers.refresh")}
                        data-app-tooltip={t("settings.chat.providers.refresh")}
                        onclick={() => void refreshProvider(selectedProvider)}
                      >
                        <RefreshCw size={14} class={headerBusy ? "animate-spin" : undefined} />
                      </button>
                      <button
                        type="button"
                        class="icon-action destructive"
                        disabled={headerBusy}
                        aria-label={t("settings.chat.providers.remove")}
                        data-app-tooltip={t("settings.chat.providers.remove")}
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
                <div class="editor-content">
                  {#if selectedProvider.lastProbe?.state === "authentication_required"}
                    <p class="provider-notice">{t("settings.chat.providers.loginHelp")}</p>
                  {/if}

                  <section class="editor-section catalog-section">
                    {#if acceptsCustomModels}
                      <div class="catalog-actions">
                        <button type="button" class="compact-action" disabled={selectedBusy} onclick={() => { customOpen = !customOpen; }}>
                          <Plus size={13} />
                          {t("settings.chat.models.addCustom")}
                        </button>
                      </div>
                    {/if}

                    <label class="model-search">
                      <Search size={14} />
                      <input
                        type="search"
                        bind:value={search}
                        aria-label={t("settings.chat.models.search")}
                        placeholder={t("settings.chat.models.search")}
                      />
                    </label>

                    {#if selectedProvider.modelCatalog?.stale}
                      <p class="catalog-warning">{t("settings.chat.models.catalogStale")}</p>
                    {/if}

                    {#if customOpen && acceptsCustomModels}
                      <section class="custom-model-editor">
                        <label>
                          <span>{t("settings.chat.models.customId")}</span>
                          <input bind:value={customId} />
                        </label>
                        <label>
                          <span>{t("settings.chat.models.customLabel")}</span>
                          <input bind:value={customLabel} />
                        </label>
                        <button type="button" class="compact-action" disabled={selectedBusy} onclick={() => void saveCustomModel()}>
                          {t("settings.chat.models.saveCustom")}
                        </button>
                        {#if customError}<p role="alert">{customError}</p>{/if}
                      </section>
                    {/if}

                    {#if models.length === 0}
                      <p class="models-empty">
                        {search.trim() ? t("settings.chat.models.noSearchResults") : t("settings.chat.models.empty")}
                      </p>
                    {:else}
                      <div class="model-list">
                        {#each models as model (model.id)}
                          {@const company = modelCompany(selectedProvider.configuration.familyId, model)}
                          {@const showModelId = shouldShowModelId(model)}
                          {@const showModelMetadata = showModelId
                            || Boolean(model.contextLimit)
                            || model.availability !== "available"
                            || model.custom}
                          <div class="model-row">
                            <ChatModelAvatar familyId={company.iconFamilyId} label={company.name} size={25} />
                            <div class="model-identity">
                              <span class="model-name">{formatModelDisplayName(model.displayName)}</span>
                              {#if showModelMetadata}
                                <div class="model-meta">
                                  {#if showModelId}<span class="model-id">{model.id}</span>{/if}
                                  {#if model.contextLimit}
                                    <span>{t("settings.chat.models.context", formatNumber(localization.locale, model.contextLimit))}</span>
                                  {/if}
                                  {#if model.availability !== "available"}
                                    <span class="text-status-tentative">
                                      {model.availability === "stale"
                                        ? t("settings.chat.models.stale")
                                        : model.availability === "deprecated"
                                          ? t("settings.chat.models.deprecated")
                                          : t("settings.chat.models.unavailable")}
                                    </span>
                                  {/if}
                                  {#if model.custom}<span>{t("settings.chat.models.custom")}</span>{/if}
                                </div>
                              {/if}
                            </div>
                            <div class="model-actions">
                              <Switch
                                checked={isVisible(model.id)}
                                ariaLabel={isVisible(model.id) ? t("settings.chat.models.hide") : t("settings.chat.models.show")}
                                disabled={selectedBusy}
                                onChange={(visible) => void setVisible(model.id, visible)}
                              />
                              <button
                                type="button"
                                class="favorite-action"
                                aria-label={t("settings.chat.models.favorite")}
                                aria-pressed={selectedProvider.configuration.favoriteModelIds.includes(model.id)}
                                onclick={() => void toggleFavorite(model.id)}
                              >
                                <Star
                                  size={14}
                                  fill={selectedProvider.configuration.favoriteModelIds.includes(model.id) ? "currentColor" : "none"}
                                />
                              </button>
                              {#if model.custom}
                                <button
                                  type="button"
                                  class="icon-action destructive"
                                  disabled={selectedBusy}
                                  aria-label={t("settings.chat.models.removeCustom")}
                                  data-app-tooltip={t("settings.chat.models.removeCustom")}
                                  onclick={() => void removeCustomModel(model.id)}
                                >
                                  <Trash2 size={14} />
                                </button>
                              {/if}
                            </div>
                          </div>
                        {/each}
                      </div>
                    {/if}

                    {#if selectedFamily.implementationStatus === "available"}
                      <button
                        type="button"
                        class="add-configuration-text"
                        onclick={() => openProviderSetup(selectedFamily.familyId)}
                      >{t("settings.chat.providers.addAnother", selectedFamily.displayName)}</button>
                    {/if}
                  </section>
                </div>
              {:else}
                <section class="empty-configuration">
                  <p>
                    {selectedFamily.implementationStatus === "available"
                      ? t("settings.chat.providers.noConfigurations", selectedFamily.displayName)
                      : selectedFamily.unavailableReason ?? t("settings.chat.providers.notAvailable")}
                  </p>
                  {#if selectedFamily.implementationStatus === "available"}
                    <button
                      type="button"
                      class="add-configuration-text"
                      onclick={() => openProviderSetup(selectedFamily.familyId)}
                    >{t("settings.chat.providers.setUp", selectedFamily.displayName)}</button>
                  {/if}
                </section>
              {/if}
            </div>
            <CalendarScrollbar scrollContainer={detailScrollElement} wheelPassthrough />
          </div>
        {/if}
      </div>
    </div>
  {/if}
</section>

{#if removeProvider}
  <ConfirmDialog
    title={t("settings.chat.providers.removeTitle")}
    message={t("settings.chat.providers.removeMessage", removeProvider.configuration.label)}
    confirmLabel={t("settings.chat.providers.remove")}
    cancelLabel={t("chat.cancel")}
    onConfirm={() => void confirmRemove()}
    onCancel={() => { removeId = null; }}
  />
{/if}

<style>
  .provider-settings { display:grid; height:100%; min-height:0; grid-template-rows:auto minmax(0,1fr); gap:0.8rem; }
  .directory-header { display:flex; flex-wrap:wrap; align-items:start; justify-content:space-between; gap:0.75rem; padding-inline:0.25rem; }
  .directory-header h2 { font-size:calc(0.866667rem * var(--type-scale)); font-weight:600; }
  .settings-button,.compact-action { display:inline-flex; min-height:1.9rem; align-items:center; justify-content:center; gap:0.35rem; border:1px solid var(--border); border-radius:0.42rem; background:var(--background); padding:0.3rem 0.65rem; color:var(--foreground); font-size:calc(0.733333rem * var(--type-scale)); font-weight:600; line-height:1; white-space:nowrap; }
  .settings-button:hover:not(:disabled),.compact-action:hover:not(:disabled) { background:var(--accent); }
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
  .directory-summary small[data-state="healthy"] i,.editor-state[data-state="healthy"] i { background:var(--action-confirm); }
  .directory-summary small[data-state="disabled"] i,.editor-state[data-state="disabled"] i { background:var(--muted-foreground); opacity:0.7; }
  .directory-summary small[data-state="unconfigured"] i,.editor-state[data-state="unconfigured"] i { border:1px solid currentColor; background:transparent; }
  .detail-panel { position:relative; z-index:2; min-width:0; min-height:0; border-left:1px solid var(--border); padding-left:1rem; }
  .detail-scroll { display:grid; align-content:start; gap:1rem; padding:0.2rem 0.75rem 1rem 0; }
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
  .refresh-summary { border-left:2px solid var(--action-confirm); padding:0.15rem 0.25rem 0.15rem 0.65rem; font-size:calc(0.68rem * var(--type-scale)); }
  .detail-scroll > :global(.operation-error) { padding:0.15rem 0.25rem 0.15rem 0.65rem; font-size:calc(0.68rem * var(--type-scale)); }
  .refresh-summary { color:var(--muted-foreground); }
  .refresh-summary.attention,.catalog-warning,.provider-notice { color:var(--status-tentative); }
  .refresh-summary.attention { border-left-color:var(--destructive); }
  .custom-model-editor p { color:var(--destructive); }
  .editor-content { display:grid; align-content:start; gap:1rem; }
  .editor-section { display:grid; min-width:0; gap:0.65rem; }
  .catalog-section { gap:0; }
  .catalog-actions { display:flex; justify-content:flex-end; padding:0 0.25rem 0.65rem; }
  .icon-action,.favorite-action { display:inline-grid; width:1.75rem; height:1.75rem; place-items:center; border:1px solid var(--border); border-radius:0.375rem; background:var(--background); color:var(--muted-foreground); }
  .icon-action:hover:not(:disabled),.favorite-action:hover { background:var(--accent); color:var(--foreground); }
  .icon-action.destructive { color:var(--destructive); }
  .provider-notice,.catalog-warning { padding:0.25rem; font-size:calc(0.68rem * var(--type-scale)); }
  .model-search { display:grid; grid-template-columns:auto minmax(0,1fr); align-items:center; gap:0.45rem; border-block:1px solid var(--border); padding:0.5rem 0.25rem; color:var(--muted-foreground); }
  .model-search input { min-width:0; background:transparent; color:var(--foreground); font-size:calc(0.75rem * var(--type-scale)); outline:none; }
  .model-search:focus-within { color:var(--foreground); }
  .custom-model-editor { display:grid; grid-template-columns:minmax(0,1fr) minmax(0,1fr) auto; align-items:end; gap:0.6rem; border-bottom:1px solid var(--border); padding:0.65rem 0.25rem; }
  .custom-model-editor label { display:grid; min-width:0; gap:0.3rem; color:var(--muted-foreground); font-size:calc(0.68rem * var(--type-scale)); }
  .custom-model-editor input { min-width:0; height:2rem; border:1px solid var(--border); border-radius:var(--floating-item-radius); background:transparent; padding:0.3rem 0.55rem; color:var(--foreground); outline:none; }
  .custom-model-editor p { grid-column:1/-1; font-size:calc(0.68rem * var(--type-scale)); }
  .model-list { display:grid; }
  .model-row { display:grid; grid-template-columns:auto minmax(0,1fr) auto; align-items:center; gap:0.65rem; border-bottom:1px solid var(--border); padding:0.62rem 0.25rem; }
  .model-identity { display:grid; min-width:0; align-content:center; gap:0.16rem; }
  .model-name { overflow:hidden; color:var(--foreground); font-size:calc(0.8rem * var(--type-scale)); font-weight:550; line-height:1.1rem; text-overflow:ellipsis; white-space:nowrap; }
  .model-meta { display:flex; min-width:0; flex-wrap:wrap; gap:0.15rem 0.42rem; color:var(--muted-foreground); font-size:calc(0.66rem * var(--type-scale)); line-height:1rem; }
  .model-meta > span + span::before { margin-right:0.42rem; content:"·"; }
  .model-id { overflow:hidden; max-width:22rem; font-family:var(--font-mono,monospace); text-overflow:ellipsis; white-space:nowrap; }
  .model-actions { display:flex; flex-shrink:0; align-items:center; gap:0.45rem; }
  .favorite-action[aria-pressed="true"] { color:var(--status-tentative); }
  .models-empty { padding:0.9rem 0.25rem; color:var(--muted-foreground); font-size:calc(0.75rem * var(--type-scale)); }
  .add-configuration-text { width:max-content; margin:0.6rem 0.25rem 0; color:var(--muted-foreground); font-size:calc(0.7rem * var(--type-scale)); font-weight:600; text-align:left; }
  .add-configuration-text:hover { color:var(--foreground); text-decoration:underline; text-underline-offset:0.18rem; }
  .empty-configuration { display:grid; justify-items:start; gap:0.7rem; padding:0.25rem; color:var(--muted-foreground); font-size:calc(0.72rem * var(--type-scale)); }
  .empty-configuration .add-configuration-text { margin:0; }
  button:disabled { cursor:not-allowed; opacity:0.5; }
  :global(.dark) .settings-button,:global(.dark) .compact-action,:global(.dark) .icon-action,:global(.dark) .favorite-action { background:transparent; }
  @media (max-width:700px) {
    .directory-layout { grid-template-columns:1fr; grid-template-rows:minmax(8rem,30%) minmax(0,1fr); }
    .directory-panel { border-bottom:1px solid var(--border); padding:0 0 0.75rem; }
    .detail-panel { border-left:0; padding:0.8rem 0 0; }
    .detail-scroll { padding-right:0.35rem; }
  }
  @media (max-width:540px) {
    .custom-model-editor { grid-template-columns:1fr; align-items:stretch; }
    .custom-model-editor p { grid-column:auto; }
    .model-actions { gap:0.2rem; }
    .model-id { max-width:10rem; }
  }
</style>
