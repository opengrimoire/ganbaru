<script lang="ts">
  import { onMount } from "svelte";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import X from "@lucide/svelte/icons/x";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { ProviderFileRead, ProviderInstanceId } from "$lib/chat/contracts";
  import {
    clearBrowserPreviewData,
    readChatProviderFiles,
    readPreviewModelOrigins,
    revokePreviewModelOrigin,
    saveChatProviderPreference,
    type PreviewModelOriginGrant,
  } from "$lib/api/chat";
  import { PROVIDER_DOC_LINKS, providerFilesNote, splitProviderFiles } from "$lib/chat/provider-files";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { getChat } from "$lib/stores/chat.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import ProviderFileEditor from "./ProviderFileEditor.svelte";
  import ProviderPreferenceList from "./ProviderPreferenceList.svelte";

  const { t } = getLocalization();
  const chat = getChat();
  let selectedInstanceId = $state<ProviderInstanceId | null>(null);
  let files = $state<ProviderFileRead[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let loadGeneration = $state(0);
  let instructionsDirty = $state(false);
  let configurationDirty = $state(false);
  let configurationOpen = $state(false);
  let savingPreference = $state(false);
  let preferenceError = $state<string | null>(null);
  let siteGrants = $state<PreviewModelOriginGrant[]>([]);
  let siteError = $state<string | null>(null);
  let revoking = $state(false);
  let clearingPreviewData = $state(false);
  let previewDataCleared = $state(false);
  let previewDataError = $state<string | null>(null);

  const providers = $derived(chat.settings?.providerInstances ?? []);
  const selectedProvider = $derived(
    providers.find((provider) => provider.configuration.instanceId === selectedInstanceId) ?? null,
  );
  const familyId = $derived(selectedProvider?.configuration.familyId ?? null);
  const dirty = $derived(instructionsDirty || configurationDirty);
  const groups = $derived(splitProviderFiles(files));
  const preferenceFile = $derived(groups.configuration.find((file) => file.preferences !== null) ?? null);
  const note = $derived(familyId ? providerFilesNote(familyId) : null);
  const docLinks = $derived(familyId ? PROVIDER_DOC_LINKS[familyId] ?? [] : []);
  const providerOptions = $derived(providers.map((provider) => ({
    value: provider.configuration.instanceId,
    label: provider.configuration.label,
  })));

  $effect(() => {
    if (providers.length === 0) {
      selectedInstanceId = null;
      files = [];
      return;
    }
    if (!providers.some((provider) => provider.configuration.instanceId === selectedInstanceId)) {
      selectedInstanceId = providers[0].configuration.instanceId;
    }
  });

  $effect(() => {
    const instanceId = selectedInstanceId;
    if (instanceId) void loadFiles(instanceId);
  });

  async function loadFiles(instanceId: ProviderInstanceId): Promise<void> {
    const generation = ++loadGeneration;
    loading = true;
    error = null;
    preferenceError = null;
    try {
      const next = await readChatProviderFiles(instanceId);
      if (generation !== loadGeneration || instanceId !== selectedInstanceId) return;
      files = next;
    } catch (cause: unknown) {
      if (generation !== loadGeneration) return;
      files = [];
      error = errorMessage(cause);
    } finally {
      if (generation === loadGeneration) loading = false;
    }
  }

  function chooseProvider(instanceId: ProviderInstanceId): void {
    if (instanceId === selectedInstanceId) return;
    if (dirty) {
      error = t("settings.chat.permissions.saveOrReload");
      return;
    }
    configurationOpen = false;
    selectedInstanceId = instanceId;
  }

  function reload(): void {
    if (selectedInstanceId) void loadFiles(selectedInstanceId);
  }

  /** Retries loading the selected provider's files and rejects when the load fails again. */
  async function retryLoadFiles(): Promise<void> {
    if (!selectedInstanceId) throw new Error("No Chat provider is selected to reload");
    await loadFiles(selectedInstanceId);
    if (error) throw new Error(error);
  }

  function replaceFile(saved: ProviderFileRead): void {
    files = files.map((file) => file.fileId === saved.fileId ? saved : file);
  }

  async function savePreference(preferenceId: string, value: boolean | string | null): Promise<void> {
    const file = preferenceFile;
    if (!selectedInstanceId || !file || savingPreference) return;
    savingPreference = true;
    preferenceError = null;
    try {
      replaceFile(await saveChatProviderPreference({
        instanceId: selectedInstanceId,
        fileId: file.fileId,
        preferenceId,
        value,
        expectedRevision: file.revision,
      }));
    } catch (cause: unknown) {
      preferenceError = errorMessage(cause);
    } finally {
      savingPreference = false;
    }
  }

  async function openDocs(url: string): Promise<void> {
    try {
      await openUrl(url);
    } catch (cause: unknown) {
      error = errorMessage(cause);
    }
  }

  async function loadSiteGrants(): Promise<void> {
    siteError = null;
    try {
      siteGrants = await readPreviewModelOrigins();
    } catch (cause: unknown) {
      siteError = errorMessage(cause);
      throw cause;
    }
  }

  async function revokeSite(grant: PreviewModelOriginGrant): Promise<void> {
    if (revoking) return;
    revoking = true;
    siteError = null;
    try {
      await revokePreviewModelOrigin(grant.threadId, grant.origin);
      siteGrants = siteGrants.filter((entry) => entry.threadId !== grant.threadId || entry.origin !== grant.origin);
    } catch (cause: unknown) {
      siteError = errorMessage(cause);
    } finally {
      revoking = false;
    }
  }

  async function clearPreviewData(): Promise<void> {
    if (clearingPreviewData) return;
    clearingPreviewData = true;
    previewDataCleared = false;
    previewDataError = null;
    try {
      await clearBrowserPreviewData();
      previewDataCleared = true;
    } catch (cause: unknown) {
      previewDataError = errorMessage(cause);
    } finally {
      clearingPreviewData = false;
    }
  }

  function threadLabel(threadId: string): string {
    return [...chat.activeThreads, ...chat.archivedThreads].find((thread) => thread.id === threadId)?.title || threadId;
  }

  onMount(() => {
    void loadSiteGrants().catch(() => undefined);
  });

  function errorMessage(cause: unknown): string {
    return chatErrorMessage(cause, t("chatErrors.actions.settingsAction"));
  }
</script>

{#snippet sectionHeading(title: string, description: string)}
  <div class="px-1">
    <h3 class="text-[0.8rem] font-semibold text-foreground">{title}</h3>
    <p class="mt-0.5 text-[0.7rem] text-muted-foreground">{description}</p>
  </div>
{/snippet}

<section class="flex min-h-0 flex-col gap-4">
  <div class="px-1">
    <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("settings.chat.permissions.heading")}</h2>
  </div>

  {#if providers.length === 0}
    <p class="rounded-lg border border-dashed border-border p-4 text-sm text-muted-foreground">
      {t("settings.chat.permissions.empty")}
    </p>
  {:else}
    <div class="flex flex-wrap items-end gap-3">
      <div class="min-w-56 flex-1">
        <Select
          label={t("settings.chat.permissions.providerLabel")}
          description={t("settings.chat.permissions.providerDescription")}
          value={selectedInstanceId ?? ""}
          options={providerOptions}
          onChange={(value) => chooseProvider(value)}
          ariaLabel={t("settings.chat.permissions.providerLabel")}
        />
      </div>
      <button type="button" class="chat-settings-button mx-1 mb-1" disabled={loading} onclick={reload}>
        <RefreshCw size={13} />
        {dirty ? t("settings.chat.permissions.discardAndReload") : t("settings.chat.permissions.reload")}
      </button>
    </div>

    {#if error}
      <Notice class="px-1 text-[0.733333rem]" message={error} onRetry={files.length === 0 && selectedInstanceId ? retryLoadFiles : undefined} />
    {/if}

    {#if loading}
      <Loading layout="fill" class="min-h-24" />
    {:else if selectedProvider && selectedInstanceId && files.length > 0}
      {#if note === "native" || note === "cursor_rules" || docLinks.length > 0}
        <div class="flex flex-col gap-2 px-1">
          {#if note === "native"}
            <p class="text-[0.733333rem] text-muted-foreground">{t("settings.chat.permissions.nativeNote")}</p>
          {:else if note === "cursor_rules"}
            <p class="text-[0.733333rem] text-muted-foreground">{t("settings.chat.permissions.cursorRulesNote")}</p>
          {/if}
          {#if docLinks.length > 0}
            <div class="flex flex-wrap items-center gap-x-3 gap-y-1" role="group" aria-label={t("settings.chat.permissions.docsHeading")}>
              {#each docLinks as link (link.url)}
                <button type="button" class="inline-flex items-center gap-1 text-[0.733333rem] font-medium text-foreground underline-offset-2 hover:underline" onclick={() => void openDocs(link.url)}>
                  {t(`settings.chat.permissions.docs.${link.kind}`)}
                  <ExternalLink size={11} />
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      {#key `${selectedInstanceId} ${loadGeneration}`}
        {#if groups.instructions.length > 0}
          <div class="flex flex-col gap-2">
            {@render sectionHeading(t("settings.chat.permissions.instructionsHeading"), t("settings.chat.permissions.instructionsDescription"))}
            <ProviderFileEditor
              files={groups.instructions}
              instanceId={selectedInstanceId}
              ariaLabel={t("settings.chat.permissions.instructionsHeading")}
              placeholder={t("settings.chat.permissions.instructionsPlaceholder")}
              onSaved={replaceFile}
              bind:dirty={instructionsDirty}
            />
          </div>
        {/if}

        {#if preferenceFile?.preferences}
          <div class="flex flex-col gap-2">
            {@render sectionHeading(t("settings.chat.permissions.preferencesHeading"), t("settings.chat.permissions.preferencesDescription"))}
            {#if !preferenceFile.preferences.editable}
              <Notice class="px-1 text-[0.733333rem]" tone="warning" role="status" copy={false} message={t("settings.chat.permissions.preferencesNotEditable")} />
            {:else if configurationDirty}
              <Notice class="px-1 text-[0.733333rem]" tone="warning" role="status" copy={false} message={t("settings.chat.permissions.preferencesWaitForFile")} />
            {/if}
            {#if preferenceError}
              <Notice class="px-1 text-[0.733333rem]" message={preferenceError} />
            {/if}
            <ProviderPreferenceList
              preferences={preferenceFile.preferences}
              disabled={savingPreference || configurationDirty}
              onSave={savePreference}
            />
          </div>
        {/if}

        {#if groups.configuration.length > 0}
          <div class="border-y border-border">
            <button
              type="button"
              class="flex w-full items-start gap-2 px-1 py-2.5 text-left hover:bg-accent/40"
              aria-expanded={configurationOpen}
              onclick={() => { configurationOpen = !configurationOpen; }}
            >
              <ChevronRight size={13} class="mt-0.5 shrink-0 transition-transform {configurationOpen ? 'rotate-90' : ''}" />
              <span class="min-w-0">
                <span class="block text-[0.8rem] font-semibold text-foreground">{t("settings.chat.permissions.configurationHeading")}</span>
                <span class="mt-0.5 block text-[0.7rem] text-muted-foreground">{t("settings.chat.permissions.configurationDescription")}</span>
              </span>
            </button>
            <div class="pb-3" hidden={!configurationOpen}>
              <ProviderFileEditor
                files={groups.configuration}
                instanceId={selectedInstanceId}
                ariaLabel={t("settings.chat.permissions.configurationHeading")}
                placeholder={t("settings.chat.permissions.configurationPlaceholder")}
                onSaved={replaceFile}
                bind:dirty={configurationDirty}
              />
            </div>
          </div>
        {/if}
      {/key}
    {/if}
  {/if}

  <div class="flex flex-col gap-2 px-1">
    <h3 class="text-[0.8rem] font-semibold text-foreground">{t("settings.chat.permissions.siteAccessHeading")}</h3>
    <p class="text-[0.7rem] text-muted-foreground">{t("settings.chat.permissions.siteAccessDescription")}</p>
    {#if siteError}
      <Notice class="text-[0.733333rem]" message={siteError} onRetry={loadSiteGrants} />
    {/if}
    {#if siteGrants.length === 0}
      <p class="text-[0.7rem] text-muted-foreground">{t("settings.chat.permissions.siteAccessEmpty")}</p>
    {:else}
      <ul class="flex flex-col">
        {#each siteGrants as grant (`${grant.threadId} ${grant.origin}`)}
          <li class="flex items-center justify-between gap-2 py-1">
            <div class="min-w-0">
              <p class="truncate font-mono text-[0.733333rem] text-foreground">{grant.origin}</p>
              <p class="truncate text-[0.666667rem] text-muted-foreground">{t("settings.chat.permissions.siteAccessThread", threadLabel(grant.threadId))}</p>
            </div>
            <button type="button" class="chat-settings-button" disabled={revoking} aria-label={t("settings.chat.permissions.siteAccessRevoke", grant.origin)} title={t("settings.chat.permissions.siteAccessRevoke", grant.origin)} onclick={() => void revokeSite(grant)}><X size={13} /></button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  <div class="flex flex-col gap-2 px-1">
    <h3 class="text-[0.8rem] font-semibold text-foreground">{t("settings.chat.permissions.previewDataHeading")}</h3>
    <p class="text-[0.7rem] text-muted-foreground">{t("settings.chat.permissions.previewDataDescription")}</p>
    {#if previewDataError}
      <Notice class="text-[0.733333rem]" message={previewDataError} />
    {/if}
    <div class="flex flex-wrap items-center gap-2">
      <button type="button" class="chat-settings-button" disabled={clearingPreviewData} onclick={() => void clearPreviewData()}>{t("settings.chat.permissions.previewDataClear")}</button>
      {#if previewDataCleared}<p role="status" class="text-[0.7rem] text-muted-foreground">{t("settings.chat.permissions.previewDataCleared")}</p>{/if}
    </div>
  </div>
</section>
