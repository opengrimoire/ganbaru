<script lang="ts">
  import { onMount, tick } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import Plus from "@lucide/svelte/icons/plus";
  import Save from "@lucide/svelte/icons/save";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import * as chatApi from "$lib/api/chat";
  import type { JsonValue, ProviderInstanceConfig, ProviderSetupTestRead } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import {
    createProviderSetupDraft,
    providerConfigurationFromDraft,
    providerInstanceIdFromLabel,
    validateProviderSetup,
    type ProviderEnvironmentDraft,
    type ProviderEnvironmentValueType,
    type ProviderSetupDraft,
  } from "$lib/chat/provider-setup";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import type { ChatProviderSetupTarget } from "$lib/settings/types";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import SwitchField from "$lib/components/ui/SwitchField.svelte";

  let {
    target,
    onCancel,
    compactLayout = false,
    iconRailLayout = false,
    onScrollContainerChange = () => {},
    onScrollbarInsetsChange = () => {},
  }: {
    target: ChatProviderSetupTarget;
    onCancel: () => void;
    compactLayout?: boolean;
    iconRailLayout?: boolean;
    onScrollContainerChange?: (element: HTMLElement | undefined) => void;
    onScrollbarInsetsChange?: (insets: { top: number; bottom: number }) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const chat = getChat();
  let draft = $state<ProviderSetupDraft>(createProviderSetupDraft());
  let editorRootEl: HTMLElement | undefined = $state();
  let scrollElement: HTMLElement | undefined = $state();
  let initialized = $state(false);
  let saving = $state(false);
  let testing = $state(false);
  let operationError = $state<string | null>(null);
  let testResult = $state<ProviderSetupTestRead | null>(null);
  let testSucceeded = $state(false);
  let revealSecrets = $state<Record<string, boolean>>({});
  let pendingSecrets = $state<Record<string, string>>({});
  let nextRowId = 1;
  const contentPaddingX = $derived(compactLayout ? "0.75rem" : iconRailLayout ? "1.25rem" : "2rem");
  const contentPaddingTop = $derived(compactLayout ? "1rem" : iconRailLayout ? "1.25rem" : "2rem");
  const contentPaddingBottom = $derived(compactLayout ? "1rem" : iconRailLayout ? "1.25rem" : "2rem");
  const editingId = $derived(target.mode === "edit" ? target.instanceId : null);
  const existingProvider = $derived(
    target.mode === "edit"
      ? chat.settings?.providerInstances.find((entry) => entry.configuration.instanceId === target.instanceId) ?? null
      : null,
  );
  const existingIds = $derived(
    new Set(chat.settings?.providerInstances.map((entry) => entry.configuration.instanceId) ?? []),
  );
  const validation = $derived(validateProviderSetup(draft, existingIds, editingId));
  const family = $derived(
    chat.settings?.providerFamilies.find((entry) => entry.familyId === draft.familyId) ?? null,
  );
  const openCodePassword = $derived(
    draft.environment.find((row) => row.name === "OPENCODE_SERVER_PASSWORD") ?? null,
  );

  onMount(() => {
    void initialize();
  });

  $effect(() => {
    onScrollContainerChange(scrollElement);
    reportScrollbarInsets();

    const contentEl = editorRootEl?.closest<HTMLElement>("[data-settings-content]");
    if (!editorRootEl || !scrollElement || !contentEl) {
      return () => {
        onScrollContainerChange(undefined);
        onScrollbarInsetsChange({ top: 0, bottom: 0 });
      };
    }

    const observer = new ResizeObserver(reportScrollbarInsets);
    observer.observe(editorRootEl);
    observer.observe(scrollElement);
    observer.observe(contentEl);
    window.addEventListener("resize", reportScrollbarInsets);

    return () => {
      observer.disconnect();
      window.removeEventListener("resize", reportScrollbarInsets);
      onScrollContainerChange(undefined);
      onScrollbarInsetsChange({ top: 0, bottom: 0 });
    };
  });

  function reportScrollbarInsets(): void {
    const contentEl = editorRootEl?.closest<HTMLElement>("[data-settings-content]");
    if (!scrollElement || !contentEl) {
      onScrollbarInsetsChange({ top: 0, bottom: 0 });
      return;
    }
    const contentRect = contentEl.getBoundingClientRect();
    const scrollRect = scrollElement.getBoundingClientRect();
    onScrollbarInsetsChange({
      top: Math.max(0, scrollRect.top - contentRect.top),
      bottom: Math.max(0, contentRect.bottom - scrollRect.bottom),
    });
  }

  async function initialize(): Promise<void> {
    try {
      await chat.ensureLoaded();
      if (target.mode === "edit") {
        const provider = chat.settings?.providerInstances.find(
          (entry) => entry.configuration.instanceId === target.instanceId,
        );
        if (provider) {
          draft = draftFromConfiguration(provider.configuration);
        } else {
          operationError = t("settings.chat.setup.configurationMissing");
        }
      } else {
        const providerFamily = chat.settings?.providerFamilies.find(
          (entry) => entry.familyId === target.familyId,
        );
        if (providerFamily?.implementationStatus === "available") {
          initializeFamily(
            providerFamily.familyId,
            providerFamily.displayName,
            providerFamily.defaultExecutableCandidates[0] ?? "",
          );
        } else {
          operationError = t("settings.chat.setup.integrationUnavailable");
        }
      }
    } catch (error: unknown) {
      operationError = errorMessage(error);
    } finally {
      initialized = true;
      await tick();
      scrollElement?.focus();
    }
  }

  /** Retries preparing the setup form and rejects when it still cannot be prepared. */
  async function retryInitialize(): Promise<void> {
    operationError = null;
    await initialize();
    if (operationError) throw new Error(operationError);
  }

  function initializeFamily(familyId: string, displayName: string, executable: string): void {
    draft.familyId = familyId;
    draft.executable = executable;
    draft.providerHome = "";
    draft.providerConfig = defaultProviderConfig(familyId);
    draft.label = displayName;
    draft.instanceId = uniqueInstanceId(displayName);
    testResult = null;
    testSucceeded = false;
    operationError = null;
  }

  function uniqueInstanceId(label: string): string {
    const base = providerInstanceIdFromLabel(label) || "provider";
    if (!existingIds.has(base)) return base;
    let suffix = 2;
    while (existingIds.has(`${base}-${suffix}`)) suffix += 1;
    return `${base}-${suffix}`;
  }

  function updateLabel(value: string): void {
    const previousGenerated = providerInstanceIdFromLabel(draft.label);
    draft.label = value;
    const secretBound = draft.environment.some((row) => row.credentialReference);
    if (!secretBound && (!draft.instanceId || draft.instanceId === previousGenerated)) {
      draft.instanceId = uniqueInstanceId(value);
    }
  }

  function addArgument(): void {
    draft.launchArguments = [...draft.launchArguments, ""];
  }

  function addEnvironment(): void {
    const row: ProviderEnvironmentDraft = {
      key: `row-${nextRowId++}`,
      name: "",
      valueType: "text",
      value: "",
      credentialReference: "",
    };
    draft.environment = [...draft.environment, row];
  }

  function setEnvironmentValueType(
    row: ProviderEnvironmentDraft,
    valueType: ProviderEnvironmentValueType,
  ): void {
    row.valueType = valueType;
  }

  function removeEnvironment(key: string): void {
    draft.environment = draft.environment.filter((row) => row.key !== key);
    delete pendingSecrets[key];
  }

  function setOpenCodeMode(mode: "local" | "external"): void {
    const current = draft.providerConfig.value;
    const value: { [key: string]: JsonValue } = typeof current === "object"
      && current !== null
      && !Array.isArray(current)
      ? { ...current, mode }
      : { mode };
    if (mode === "local") {
      delete value.serverUrl;
      delete value.allowInsecureExternalHttp;
      delete value.confirmExternalWorkspaceAccess;
    }
    draft.providerConfig = { schemaVersion: 1, value };
    if (mode === "local" && openCodePassword) {
      const row = openCodePassword;
      if (row.credentialReference) {
        void removeSecret(row).catch((error: unknown) => { operationError = errorMessage(error); });
      }
      removeEnvironment(row.key);
    }
  }

  function addOpenCodePassword(): void {
    if (openCodePassword) return;
    draft.environment = [...draft.environment, {
      key: `row-${nextRowId++}`,
      name: "OPENCODE_SERVER_PASSWORD",
      valueType: "secret",
      value: "",
      credentialReference: "",
    }];
  }

  async function storeSecret(row: ProviderEnvironmentDraft): Promise<void> {
    const secret = pendingSecrets[row.key]?.trim();
    if (!secret || !row.name) return;
    try {
      row.credentialReference = await chatApi.replaceChatCredential(draft.instanceId, row.name, secret);
      pendingSecrets[row.key] = "";
      operationError = null;
    } catch (error: unknown) {
      operationError = errorMessage(error);
    }
  }

  async function removeSecret(row: ProviderEnvironmentDraft): Promise<void> {
    try {
      if (row.credentialReference) await chatApi.removeChatCredential(draft.instanceId, row.name);
      row.credentialReference = "";
      operationError = null;
    } catch (error: unknown) {
      operationError = errorMessage(error);
    }
  }

  async function pickExecutable(): Promise<void> {
    try {
      const selected = await chatApi.pickChatProviderExecutable(t("settings.chat.setup.chooseExecutable"));
      if (selected) draft.executable = selected;
    } catch (error: unknown) {
      operationError = errorMessage(error);
    }
  }

  async function pickHome(): Promise<void> {
    try {
      const selected = await chatApi.pickChatProviderHome(t("settings.chat.setup.chooseProviderHome"));
      if (selected) draft.providerHome = selected;
    } catch (error: unknown) {
      operationError = errorMessage(error);
    }
  }

  async function testSetup(): Promise<void> {
    operationError = null;
    testResult = null;
    testSucceeded = false;
    if (!validation.valid) return;
    testing = true;
    try {
      const configuration = configurationFromDraft();
      const result = await chat.testProvider(configuration);
      if (!result) return;
      testResult = result;
      testSucceeded = result.probe.state === "healthy"
        || testResult.probe.state === "authentication_required";
    } catch (error: unknown) {
      operationError = errorMessage(error);
    } finally {
      testing = false;
    }
  }

  async function save(): Promise<void> {
    operationError = null;
    if (!validation.valid) return;
    saving = true;
    try {
      if (await chat.saveProvider(configurationFromDraft())) onCancel();
    } catch (error: unknown) {
      operationError = errorMessage(error);
    } finally {
      saving = false;
    }
  }

  function configurationFromDraft(): ProviderInstanceConfig {
    return providerConfigurationFromDraft(draft, existingProvider?.configuration);
  }

  function providerConfigValue(key: string): string {
    const value = draft.providerConfig.value;
    return typeof value === "object"
      && value !== null
      && !Array.isArray(value)
      && typeof value[key] === "string"
      ? value[key]
      : "";
  }

  function openCodeMode(): "local" | "external" {
    const mode = providerConfigValue("mode");
    return mode === "external" ? "external" : "local";
  }

  function openCodeServerUrl(): string {
    return providerConfigValue("serverUrl");
  }

  function openCodeUsesInsecureExternalHttp(): boolean {
    try {
      const url = new URL(openCodeServerUrl());
      const hostname = url.hostname.toLowerCase();
      const loopback = hostname === "localhost"
        || hostname === "[::1]"
        || /^127(?:\.\d{1,3}){3}$/.test(hostname);
      return url.protocol === "http:" && !loopback;
    } catch {
      return false;
    }
  }

  function providerConfigBoolean(key: string): boolean {
    const value = draft.providerConfig.value;
    return typeof value === "object"
      && value !== null
      && !Array.isArray(value)
      && value[key] === true;
  }

  function setProviderConfigValue(key: string, value: string): void {
    const current = draft.providerConfig.value;
    draft.providerConfig = {
      schemaVersion: 1,
      value: {
        ...(typeof current === "object" && current !== null && !Array.isArray(current) ? current : {}),
        [key]: value || null,
      },
    };
  }

  function setProviderConfigBoolean(key: string, value: boolean): void {
    const current = draft.providerConfig.value;
    draft.providerConfig = {
      schemaVersion: 1,
      value: {
        ...(typeof current === "object" && current !== null && !Array.isArray(current) ? current : {}),
        [key]: value,
      },
    };
  }

  function fieldError(field: string): string | null {
    const error = validation.fields[field];
    if (!error) return null;
    if (field === "familyId") return t("settings.chat.setup.validation.provider");
    if (field === "label") return t("settings.chat.setup.validation.label");
    if (field === "instanceId") {
      return error.includes("already")
        ? t("settings.chat.setup.validation.duplicateInstanceId")
        : t("settings.chat.setup.validation.instanceId");
    }
    if (field === "executable") return t("settings.chat.setup.validation.executable");
    if (field.startsWith("launchArguments")) return t("settings.chat.setup.validation.arguments");
    if (field.endsWith(".name")) return t("settings.chat.setup.validation.environmentName");
    if (field.endsWith(".value")) {
      return error.includes("secret")
        ? t("settings.chat.setup.validation.secret")
        : t("settings.chat.setup.validation.environmentValue");
    }
    if (field === "providerConfig.endpoint" || field === "providerConfig.serverUrl") {
      return t("settings.chat.setup.validation.endpoint");
    }
    if (field === "providerConfig.allowInsecureExternalHttp") {
      return t("settings.chat.setup.validation.insecureExternalHttp");
    }
    if (field === "providerConfig.confirmExternalWorkspaceAccess") {
      return t("settings.chat.setup.validation.externalWorkspaceAccess");
    }
    return t("settings.chat.setup.validation.providerConfig");
  }

  function draftFromConfiguration(configuration: ProviderInstanceConfig): ProviderSetupDraft {
    const environment: ProviderEnvironmentDraft[] = [];
    for (const [name, value] of Object.entries(configuration.environment)) {
      environment.push({
        key: `row-${nextRowId++}`,
        name,
        valueType: value === `inherit:${name}` ? "inherit" : "text",
        value,
        credentialReference: "",
      });
    }
    for (const [name, reference] of Object.entries(configuration.credentialReferences)) {
      environment.push({
        key: `row-${nextRowId++}`,
        name,
        valueType: "secret",
        value: "",
        credentialReference: reference,
      });
    }
    return {
      familyId: configuration.familyId,
      label: configuration.label,
      instanceId: configuration.instanceId,
      executable: configuration.executable,
      providerHome: configuration.providerHome ?? "",
      launchArguments: [...configuration.launchArguments],
      environment,
      providerConfig: configuration.providerConfig,
    };
  }

  function defaultProviderConfig(familyId: string): ProviderSetupDraft["providerConfig"] {
    if (familyId === "codex") {
      return {
        schemaVersion: 1,
        value: {
          refreshMcpBeforeTurn: false,
          allowCustomModels: false,
          customModelIds: [],
          customModelLabels: {},
        },
      };
    }
    if (familyId === "opencode") return { schemaVersion: 1, value: { mode: "local" } };
    return { schemaVersion: 1, value: {} };
  }

  function errorMessage(error: unknown): string {
    return chatErrorMessage(error, t("settings.chat.setup.operationFailed"));
  }

  function editorTitle(): string {
    if (!family) return target.mode === "edit" ? t("settings.chat.setup.editTitle") : t("settings.chat.setup.title");
    return target.mode === "edit"
      ? t("settings.chat.setup.editConfigurationTitle", family.displayName)
      : t("settings.chat.setup.addConfigurationTitle", family.displayName);
  }
</script>

<div bind:this={editorRootEl} class="flex h-full min-h-0 flex-col">
  <main bind:this={scrollElement} tabindex="-1" class="hide-scrollbar min-h-0 flex-1 overflow-y-auto outline-none">
    <header
      class="shrink-0"
      style="padding-left: {contentPaddingX}; padding-right: {contentPaddingX}; padding-top: {contentPaddingTop};"
    >
      <div class="border-b border-border/70 pb-4">
        <h1 class="truncate text-[1rem] font-semibold text-foreground">
          {editorTitle()}
        </h1>
        <p class="mt-1 max-w-2xl text-[0.866667rem] text-muted-foreground">
          {t("settings.chat.setup.intro")}
        </p>
      </div>
    </header>

    <div
      class="flex flex-col gap-6 py-6"
      style="padding-left: {contentPaddingX}; padding-right: {contentPaddingX};"
    >
      {#if !initialized}
        <Loading layout="fill" class="min-h-28" />
      {:else}
        {#if family}
          <section class="setup-section">
            <h2>{t("settings.chat.setup.connectionSection")}</h2>
            <div class="form-row">
              <label for="provider-label">{t("settings.chat.setup.label")}</label>
              <div class="field-control">
                <input
                  id="provider-label"
                  value={draft.label}
                  oninput={(event) => updateLabel(event.currentTarget.value)}
                />
                {#if fieldError("label")}<small>{fieldError("label")}</small>{/if}
              </div>
            </div>
            <div class="form-row">
              <label for="provider-executable">{t("settings.chat.setup.executable")}</label>
              <div class="field-control">
                <div class="input-with-action">
                  <input id="provider-executable" class="min-w-0 flex-1" bind:value={draft.executable} />
                  <button type="button" aria-label={t("settings.chat.setup.chooseExecutable")} onclick={() => void pickExecutable()}>
                    <FolderOpen size={14} />
                  </button>
                </div>
                {#if fieldError("executable")}<small>{fieldError("executable")}</small>{/if}
              </div>
            </div>

            {#if draft.familyId !== "cursor"}
              <div class="form-row">
                <label for="provider-home">{t("settings.chat.setup.providerHome")}</label>
                <div class="field-control">
                  <div class="input-with-action">
                    <input id="provider-home" class="min-w-0 flex-1" bind:value={draft.providerHome} />
                    <button type="button" aria-label={t("settings.chat.setup.chooseProviderHome")} onclick={() => void pickHome()}>
                      <FolderOpen size={14} />
                    </button>
                  </div>
                </div>
              </div>
            {/if}

            {#if draft.familyId === "codex"}
              <div class="form-row">
                <label for="codex-shadow-home">{t("settings.chat.setup.codexShadowHome")}</label>
                <div class="field-control">
                  <input
                    id="codex-shadow-home"
                    value={providerConfigValue("shadowHomePath")}
                    oninput={(event) => setProviderConfigValue("shadowHomePath", event.currentTarget.value)}
                  />
                </div>
              </div>
              <SwitchField
                label={t("settings.chat.setup.allowCustomModels")}
                checked={providerConfigBoolean("allowCustomModels")}
                onChange={(checked) => setProviderConfigBoolean("allowCustomModels", checked)}
              />
            {:else if draft.familyId === "cursor"}
              <div class="form-row">
                <label for="cursor-endpoint">{t("settings.chat.setup.endpoint")}</label>
                <div class="field-control">
                  <input
                    id="cursor-endpoint"
                    value={providerConfigValue("endpoint")}
                    oninput={(event) => setProviderConfigValue("endpoint", event.currentTarget.value)}
                  />
                  {#if fieldError("providerConfig.endpoint")}<small>{fieldError("providerConfig.endpoint")}</small>{/if}
                </div>
              </div>
            {:else if draft.familyId === "opencode"}
              <div class="form-row">
                <span>{t("settings.chat.setup.openCodeMode")}</span>
                <div class="segmented-control" role="radiogroup" aria-label={t("settings.chat.setup.openCodeMode") }>
                  <button
                    type="button"
                    role="radio"
                    aria-checked={openCodeMode() === "local"}
                    class:active={openCodeMode() === "local"}
                    onclick={() => setOpenCodeMode("local")}
                  >{t("settings.chat.setup.localMode")}</button>
                  <button
                    type="button"
                    role="radio"
                    aria-checked={openCodeMode() === "external"}
                    class:active={openCodeMode() === "external"}
                    onclick={() => setOpenCodeMode("external")}
                  >{t("settings.chat.setup.externalMode")}</button>
                </div>
              </div>
              {#if openCodeMode() === "external"}
                <div class="form-row">
                  <label for="opencode-endpoint">{t("settings.chat.setup.endpoint")}</label>
                  <div class="field-control">
                    <input
                      id="opencode-endpoint"
                      value={openCodeServerUrl()}
                      oninput={(event) => setProviderConfigValue("serverUrl", event.currentTarget.value)}
                    />
                    {#if fieldError("providerConfig.serverUrl")}<small>{fieldError("providerConfig.serverUrl")}</small>{/if}
                  </div>
                </div>
                <SwitchField
                  label={t("settings.chat.setup.confirmExternalWorkspaceAccess")}
                  checked={providerConfigBoolean("confirmExternalWorkspaceAccess")}
                  onChange={(checked) => setProviderConfigBoolean("confirmExternalWorkspaceAccess", checked)}
                />
                {#if fieldError("providerConfig.confirmExternalWorkspaceAccess")}
                  <p class="field-error">{fieldError("providerConfig.confirmExternalWorkspaceAccess")}</p>
                {/if}
                {#if openCodeUsesInsecureExternalHttp()}
                  <div class="security-warning">
                    <p>{t("settings.chat.setup.insecureExternalHttpWarning")}</p>
                    <SwitchField
                      label={t("settings.chat.setup.allowInsecureExternalHttp")}
                      checked={providerConfigBoolean("allowInsecureExternalHttp")}
                      onChange={(checked) => setProviderConfigBoolean("allowInsecureExternalHttp", checked)}
                    />
                    {#if fieldError("providerConfig.allowInsecureExternalHttp")}
                      <p class="field-error">{fieldError("providerConfig.allowInsecureExternalHttp")}</p>
                    {/if}
                  </div>
                {/if}
                {#if openCodePassword}
                  <div class="form-row">
                    <span>{t("settings.chat.setup.externalPassword")}</span>
                    <div class="field-control">
                      <div class="input-with-actions">
                        <input
                          type={revealSecrets[openCodePassword.key] ? "text" : "password"}
                          value={pendingSecrets[openCodePassword.key] ?? ""}
                          placeholder={openCodePassword.credentialReference
                            ? t("settings.chat.setup.stored")
                            : t("settings.chat.setup.missing")}
                          oninput={(event) => { pendingSecrets[openCodePassword.key] = event.currentTarget.value; }}
                        />
                        <button
                          type="button"
                          aria-label={t("settings.chat.providers.revealAccount")}
                          onclick={() => { revealSecrets[openCodePassword.key] = !revealSecrets[openCodePassword.key]; }}
                        >
                          {#if revealSecrets[openCodePassword.key]}<EyeOff size={13} />{:else}<Eye size={13} />{/if}
                        </button>
                        <button type="button" aria-label={t("settings.chat.setup.storeSecret")} onclick={() => void storeSecret(openCodePassword)}>
                          <Check size={13} />
                        </button>
                        <button
                          type="button"
                          aria-label={t("settings.chat.setup.removeSecret")}
                          onclick={() => {
                            if (openCodePassword.credentialReference) void removeSecret(openCodePassword);
                            removeEnvironment(openCodePassword.key);
                          }}
                        ><Trash2 size={13} /></button>
                      </div>
                      {#if fieldError(`environment.${openCodePassword.key}.value`)}
                        <small>{fieldError(`environment.${openCodePassword.key}.value`)}</small>
                      {/if}
                    </div>
                  </div>
                {:else}
                  <div class="row-action">
                    <button type="button" onclick={addOpenCodePassword}>
                      <Plus size={13} />{t("settings.chat.setup.addExternalPassword")}
                    </button>
                  </div>
                {/if}
              {/if}
            {/if}
          </section>

          <div class="section-divider" aria-hidden="true"></div>

          <section class="setup-section">
            <h2>{t("settings.chat.setup.advanced")}</h2>
            <div class="advanced-content">
                <div class="form-row">
                  <label for="provider-instance-id">{t("settings.chat.setup.instanceId")}</label>
                  <div class="field-control">
                    <input id="provider-instance-id" bind:value={draft.instanceId} disabled={target.mode === "edit"} />
                    {#if fieldError("instanceId")}<small>{fieldError("instanceId")}</small>{/if}
                  </div>
                </div>

                <div class="advanced-group">
                  <div class="form-row">
                    <span>{t("settings.chat.setup.launchArguments")}</span>
                    <div class="collection-action">
                      <button type="button" onclick={addArgument}><Plus size={13} />{t("settings.chat.setup.addArgument")}</button>
                    </div>
                  </div>
                  {#each draft.launchArguments as argument, index}
                    <div class="argument-row">
                      <input
                        aria-label={t("settings.chat.setup.launchArgument", index + 1)}
                        value={argument}
                        oninput={(event) => { draft.launchArguments[index] = event.currentTarget.value; }}
                      />
                      <button
                        type="button"
                        aria-label={t("settings.chat.setup.removeArgument", index + 1)}
                        onclick={() => {
                          draft.launchArguments = draft.launchArguments.filter((_, candidate) => candidate !== index);
                        }}
                      ><Trash2 size={13} /></button>
                      {#if fieldError(`launchArguments.${index}`)}
                        <small>{fieldError(`launchArguments.${index}`)}</small>
                      {/if}
                    </div>
                  {/each}
                  {#if draft.launchArguments.length === 0}
                    <p class="advanced-empty">{t("settings.chat.setup.noArguments")}</p>
                  {/if}
                </div>

                <div class="advanced-group">
                  <div class="form-row">
                    <span>{t("settings.chat.setup.environment")}</span>
                    <div class="collection-action">
                      <button type="button" onclick={addEnvironment}><Plus size={13} />{t("settings.chat.setup.addEnvironment")}</button>
                    </div>
                  </div>
                  {#each draft.environment.filter((row) => row.name !== "OPENCODE_SERVER_PASSWORD") as row (row.key)}
                    <div class="environment-row">
                      <input aria-label={t("settings.chat.setup.variableName")} bind:value={row.name} readonly={row.credentialReference !== ""} />
                      <div class="environment-kind" aria-label={t("settings.chat.setup.valueType") }>
                        {#each ["text", "secret", "inherit"] as valueType}
                          <button
                            type="button"
                            class:active={row.valueType === valueType}
                            onclick={() => setEnvironmentValueType(row, valueType as ProviderEnvironmentValueType)}
                          >
                            {valueType === "text"
                              ? t("settings.chat.setup.textValue")
                              : valueType === "secret"
                                ? t("settings.chat.setup.secretValue")
                                : t("settings.chat.setup.inheritedValue")}
                          </button>
                        {/each}
                      </div>
                      {#if row.valueType === "text"}
                        <input aria-label={t("settings.chat.setup.environmentValue")} bind:value={row.value} />
                      {:else if row.valueType === "secret"}
                        <div class="input-with-actions">
                          <input
                            type={revealSecrets[row.key] ? "text" : "password"}
                            value={pendingSecrets[row.key] ?? ""}
                            placeholder={row.credentialReference ? t("settings.chat.setup.stored") : t("settings.chat.setup.missing")}
                            oninput={(event) => { pendingSecrets[row.key] = event.currentTarget.value; }}
                          />
                          <button type="button" onclick={() => { revealSecrets[row.key] = !revealSecrets[row.key]; }}>
                            {#if revealSecrets[row.key]}<EyeOff size={13} />{:else}<Eye size={13} />{/if}
                          </button>
                          <button type="button" aria-label={t("settings.chat.setup.storeSecret")} onclick={() => void storeSecret(row)}>
                            <Check size={13} />
                          </button>
                        </div>
                      {:else}
                        <span class="inherit-value">{row.name || t("common.none")}</span>
                      {/if}
                      <button
                        type="button"
                        class="remove-row"
                        aria-label={t("settings.chat.setup.removeEnvironment")}
                        onclick={() => {
                          if (row.valueType === "secret" && row.credentialReference) void removeSecret(row);
                          removeEnvironment(row.key);
                        }}
                      ><Trash2 size={13} /></button>
                      {#if fieldError(`environment.${row.key}.name`) || fieldError(`environment.${row.key}.value`)}
                        <small class="environment-error">
                          {fieldError(`environment.${row.key}.name`) ?? fieldError(`environment.${row.key}.value`)}
                        </small>
                      {/if}
                    </div>
                  {/each}
                  {#if draft.environment.filter((row) => row.name !== "OPENCODE_SERVER_PASSWORD").length === 0}
                    <p class="advanced-empty">{t("settings.chat.setup.noEnvironment")}</p>
                  {/if}
                </div>
            </div>
          </section>

          {#if testResult}
            <div class="test-result" data-state={testSucceeded ? "success" : "failure"}>
              <i>{#if testSucceeded}<Check size={13} />{/if}</i>
              <span>
                {testSucceeded
                  ? t("settings.chat.setup.testPassed", formatNumber(localization.locale, testResult.modelCatalog?.models.length ?? 0))
                  : t("settings.chat.setup.testFailed", testResult.probe.detail ?? testResult.probe.state)}
              </span>
              {#if testResult.probe.version}<small>{testResult.probe.version}</small>{/if}
              {#if testResult.probe.accountLabel}<small>{testResult.probe.accountLabel}</small>{/if}
            </div>
          {/if}
        {:else if operationError}
          <div class="min-h-40 text-sm"><Notice layout="fill" message={operationError} onRetry={retryInitialize} /></div>
        {/if}
      {/if}
    </div>
  </main>

  <footer
    class="shrink-0"
    style="padding-left: {contentPaddingX}; padding-right: {contentPaddingX}; padding-bottom: {contentPaddingBottom};"
  >
    <div class="footer-content">
      <div class="footer-message">{#if operationError && family}<Notice message={operationError} />{/if}</div>
      <div class="footer-actions">
        <button type="button" class="secondary-button" onclick={onCancel}>{t("common.cancel")}</button>
        <button
          type="button"
          class="secondary-button"
          disabled={testing || saving || !validation.valid}
          onclick={() => void testSetup()}
        >
          {testing ? t("settings.chat.setup.testing") : t("settings.chat.setup.test")}
        </button>
        <button
          type="button"
          class="primary-button"
          disabled={saving || testing || !validation.valid}
          onclick={() => void save()}
        >
          {#if target.mode === "edit"}<Save size={13} />{/if}
          {target.mode === "edit" ? t("settings.chat.setup.save") : t("settings.chat.setup.create")}
        </button>
      </div>
    </div>
  </footer>
</div>

<style>
  .setup-section { display:flex; flex-direction:column; gap:0.9rem; }
  .setup-section > h2 { padding-inline:0.25rem; font-size:calc(0.866667rem * var(--type-scale)); font-weight:600; }
  .section-divider { height:1px; flex-shrink:0; background:var(--border); transform:scaleY(0.5); }
  .form-row { display:flex; min-width:0; align-items:center; justify-content:space-between; gap:1rem; padding:0.15rem 0.25rem; }
  .form-row > label,.form-row > span { min-width:0; flex:1; color:var(--foreground); font-size:calc(0.82rem * var(--type-scale)); }
  .field-control { display:flex; width:min(22rem,55%); min-width:0; flex-direction:column; gap:0.25rem; }
  input { height:1.75rem; min-width:0; border:1px solid var(--border); border-radius:var(--floating-item-radius); background:transparent; padding:0.25rem 0.5rem; color:var(--foreground); font-size:calc(0.78rem * var(--type-scale)); outline:none; }
  input:disabled { cursor:not-allowed; opacity:0.55; }
  .field-control small,.argument-row small,.environment-error,.field-error { color:var(--destructive); font-size:calc(0.68rem * var(--type-scale)); }
  .field-error { padding-inline:0.25rem; }
  .input-with-action,.input-with-actions { display:flex; min-width:0; gap:0.25rem; }
  .input-with-action input,.input-with-actions input { min-width:0; flex:1; }
  .input-with-action button,.input-with-actions button,.remove-row,.argument-row > button { display:inline-grid; width:1.75rem; height:1.75rem; flex:0 0 auto; place-items:center; border:1px solid var(--border); border-radius:0.375rem; background:var(--card); color:var(--muted-foreground); }
  .input-with-action button:hover,.input-with-actions button:hover,.remove-row:hover,.argument-row > button:hover { background:var(--accent); color:var(--foreground); }
  .segmented-control { display:grid; width:min(22rem,55%); grid-template-columns:repeat(2,minmax(0,1fr)); gap:0.2rem; border:1px solid var(--border); border-radius:0.375rem; padding:0.2rem; }
  .segmented-control button { min-height:1.65rem; border-radius:0.25rem; color:var(--muted-foreground); font-size:calc(0.72rem * var(--type-scale)); }
  .segmented-control button:hover { background:color-mix(in srgb,var(--accent) 50%,transparent); }
  .segmented-control button.active { background:var(--accent); color:var(--accent-foreground); }
  .security-warning { display:grid; gap:0.6rem; border-block:1px solid color-mix(in srgb,var(--status-tentative) 48%,var(--border)); padding:0.7rem 0.25rem; color:var(--status-tentative); font-size:calc(0.72rem * var(--type-scale)); }
  .row-action { display:flex; justify-content:flex-end; padding-inline:0.25rem; }
  .row-action button,.collection-action button { display:inline-flex; height:1.75rem; align-items:center; gap:0.35rem; border:1px solid var(--border); border-radius:0.375rem; background:var(--card); padding-inline:0.5rem; color:var(--foreground); font-size:calc(0.7rem * var(--type-scale)); }
  .row-action button:hover,.collection-action button:hover { background:var(--accent); }
  .advanced-content { display:grid; gap:1rem; padding-inline:0.25rem; }
  .advanced-group { display:grid; gap:0.45rem; }
  .collection-action { display:flex; width:min(22rem,55%); min-width:0; justify-content:flex-end; }
  .advanced-empty { padding-inline:0.25rem; color:var(--muted-foreground); font-size:calc(0.7rem * var(--type-scale)); }
  .argument-row { display:grid; grid-template-columns:minmax(0,1fr) auto; gap:0.25rem; }
  .argument-row small { grid-column:1/-1; }
  .environment-row { display:grid; grid-template-columns:minmax(7rem,0.8fr) minmax(10rem,1fr) minmax(8rem,1.2fr) auto; align-items:start; gap:0.35rem; padding-block:0.2rem; }
  .environment-kind { display:grid; grid-template-columns:repeat(3,minmax(0,1fr)); gap:0.12rem; border:1px solid var(--border); border-radius:0.375rem; padding:0.12rem; }
  .environment-kind button { min-width:0; min-height:1.5rem; overflow:hidden; border-radius:0.25rem; color:var(--muted-foreground); font-size:calc(0.61rem * var(--type-scale)); text-overflow:ellipsis; white-space:nowrap; }
  .environment-kind button.active { background:var(--accent); color:var(--accent-foreground); }
  .inherit-value { display:flex; min-height:1.75rem; align-items:center; padding-inline:0.5rem; color:var(--muted-foreground); font-size:calc(0.72rem * var(--type-scale)); }
  .environment-error { grid-column:1/-1; }
  .test-result { display:flex; flex-wrap:wrap; align-items:center; gap:0.4rem 0.65rem; border-block:1px solid var(--border); padding:0.65rem 0.25rem; color:var(--status-tentative); font-size:calc(0.75rem * var(--type-scale)); }
  .test-result[data-state="success"] { color:var(--action-confirm); }
  .test-result > i { display:grid; width:1rem; height:1rem; place-items:center; border:1px solid currentColor; border-radius:999px; }
  .test-result small { color:var(--muted-foreground); }
  .footer-content { display:flex; flex-wrap:wrap; align-items:center; justify-content:space-between; gap:0.6rem; border-top:1px solid var(--border); padding-top:0.75rem; }
  .footer-message { min-width:0; flex:1; font-size:calc(0.75rem * var(--type-scale)); }
  .footer-actions { display:flex; flex-wrap:wrap; justify-content:flex-end; gap:0.45rem; }
  .secondary-button,.primary-button { display:inline-flex; height:2rem; align-items:center; justify-content:center; gap:0.4rem; border-radius:0.375rem; padding-inline:0.75rem; font-size:calc(0.78rem * var(--type-scale)); font-weight:500; }
  .secondary-button { border:1px solid var(--border); background:var(--background); color:var(--foreground); }
  .secondary-button:hover:not(:disabled) { background:var(--accent); }
  .primary-button { background:var(--primary); color:var(--primary-foreground); }
  .primary-button:hover:not(:disabled) { background:color-mix(in srgb,var(--primary) 90%,black); }
  .secondary-button:disabled,.primary-button:disabled { cursor:not-allowed; opacity:0.5; }
  :global(.dark) .input-with-action button,
  :global(.dark) .input-with-actions button,
  :global(.dark) .remove-row,
  :global(.dark) .argument-row > button,
  :global(.dark) .row-action button,
  :global(.dark) .collection-action button { background:transparent; }
  @media (max-width:620px) {
    .form-row { align-items:stretch; flex-direction:column; gap:0.4rem; }
    .field-control,.segmented-control,.collection-action { width:100%; }
    .environment-row { grid-template-columns:minmax(0,1fr) auto; }
    .environment-kind,.environment-row > input:nth-of-type(2),.environment-row > .input-with-actions,.inherit-value { grid-column:1/2; }
    .remove-row { grid-column:2; grid-row:1; }
  }
</style>
