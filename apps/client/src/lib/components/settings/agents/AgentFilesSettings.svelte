<script lang="ts">
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { ProviderFileNote, ProviderFileRead, ProviderInstanceId } from "$lib/chat/contracts";
  import {
    readChatProviderFiles,
    saveChatProviderPreference,
  } from "$lib/api/chat";
  import {
    PROVIDER_DOC_LINKS,
    PROVIDER_FILE_NOTE_LINKS,
    PROVIDER_FILE_NOTE_NAMES,
    providerFilesFolder,
    providerFilesNote,
    providerPreferencesAllowlisted,
    splitProviderFiles,
  } from "$lib/chat/provider-files";
  import { providerFamilyDisplayName } from "$lib/chat/provider-setup";
  import { formatList } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { getChat } from "$lib/stores/chat.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import ChatModelAvatar from "$lib/components/chat/identity/ChatModelAvatar.svelte";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import ProviderFileEditor from "./ProviderFileEditor.svelte";
  import ProviderPreferenceList from "./ProviderPreferenceList.svelte";

  const localization = getLocalization();
  const { t } = localization;
  const chat = getChat();
  let selectedInstanceId = $state<ProviderInstanceId | null>(null);
  let files = $state<ProviderFileRead[]>([]);
  let fileNotes = $state<ProviderFileNote[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let loadGeneration = 0;
  let editorKey = $state(0);
  let instructionsDirty = $state(false);
  let configurationDirty = $state(false);
  let directoryScrollElement = $state<HTMLElement>();
  let detailScrollElement = $state<HTMLElement>();
  let savingPreference = $state(false);
  let preferenceError = $state<string | null>(null);

  const providers = $derived(chat.settings?.providerInstances ?? []);
  const selectedProvider = $derived(
    providers.find((provider) => provider.configuration.instanceId === selectedInstanceId) ?? null,
  );
  const familyId = $derived(selectedProvider?.configuration.familyId ?? null);
  const dirty = $derived(instructionsDirty || configurationDirty);
  const groups = $derived(splitProviderFiles(files));
  const filesFolder = $derived(providerFilesFolder(files));
  const preferenceFile = $derived(groups.configuration.find((file) => file.preferences !== null) ?? null);
  const note = $derived(familyId ? providerFilesNote(familyId) : null);
  const preferencesAllowlisted = $derived(familyId ? providerPreferencesAllowlisted(familyId) : false);
  const docLinks = $derived(familyId ? PROVIDER_DOC_LINKS[familyId] ?? [] : []);
  const familyNames = $derived(new Map((chat.settings?.providerFamilies ?? []).map((family): [string, string] => [
    family.familyId,
    providerFamilyDisplayName(family, t("chat.providers.acp.name")),
  ])));

  $effect(() => {
    if (providers.length === 0) {
      selectedInstanceId = null;
      files = [];
      fileNotes = [];
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
      files = next.files;
      fileNotes = next.notes;
      editorKey += 1;
    } catch (cause: unknown) {
      if (generation !== loadGeneration) return;
      files = [];
      fileNotes = [];
      error = errorMessage(cause);
    } finally {
      if (generation === loadGeneration) loading = false;
    }
  }

  function chooseProvider(instanceId: ProviderInstanceId): void {
    if (instanceId === selectedInstanceId) return;
    if (dirty) {
      error = t("settings.agents.files.saveOrReload");
      return;
    }
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

  function fileNoteMessage(fileNote: ProviderFileNote): string {
    return t(`settings.agents.files.fileNotes.${PROVIDER_FILE_NOTE_NAMES[fileNote.kind]}`, formatList(localization.locale, fileNote.paths));
  }

  function familyDisplayName(familyId: string): string {
    return familyNames.get(familyId) ?? familyId;
  }

  function errorMessage(cause: unknown): string {
    return chatErrorMessage(cause, t("chatErrors.actions.settingsAction"));
  }
</script>

{#snippet separator()}
  <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>
{/snippet}

{#snippet sectionHeading(title: string, description: string)}
  <div class="px-1">
    <h3 class="text-[0.866667rem] font-semibold text-foreground">{title}</h3>
    <p class="mt-0.5 text-[0.8rem] text-muted-foreground">{description}</p>
  </div>
{/snippet}

<section class="agent-files" data-agents-settings-subsection="files">
  <header class="directory-header">
    <div><h2>{t("settings.agents.files.heading")}</h2></div>
    <button type="button" class="settings-button" disabled={loading || !selectedInstanceId} onclick={reload}>
      <RefreshCw size={13} />
      {dirty ? t("settings.agents.files.discardAndReload") : t("settings.agents.files.reload")}
    </button>
  </header>

  {#if chat.loading}
    <Loading layout="rows" rows={3} />
  {:else if providers.length === 0}
    <p class="px-1 text-[0.8rem] text-muted-foreground">{t("settings.agents.files.empty")}</p>
  {:else}
    <div class="directory-layout">
      <aside class="directory-panel">
        <div class="scroll-frame">
          <div bind:this={directoryScrollElement} data-mobile-scroll-fade class="directory-scroll hide-scrollbar">
            <nav class="provider-directory" aria-label={t("settings.agents.files.directoryLabel")}>
              {#each providers as provider (provider.configuration.instanceId)}
                {@const instanceId = provider.configuration.instanceId}
                {@const active = instanceId === selectedInstanceId}
                {@const familyName = familyDisplayName(provider.configuration.familyId)}
                <button
                  type="button"
                  class="provider-row"
                  class:active
                  aria-current={active ? "page" : undefined}
                  onclick={() => chooseProvider(instanceId)}
                >
                  <ChatModelAvatar familyId={provider.configuration.familyId} label={familyName} size={32} />
                  <span class="directory-summary">
                    <strong>{provider.configuration.label}</strong>
                    <small>{familyName}</small>
                  </span>
                </button>
              {/each}
            </nav>
          </div>
          <CalendarScrollbar scrollContainer={directoryScrollElement} wheelPassthrough />
        </div>
      </aside>

      <div class="detail-panel">
        <div class="detail-scroll-frame">
          <div bind:this={detailScrollElement} data-mobile-scroll-fade class="detail-scroll hide-scrollbar">
            {#if selectedProvider}
              <div class="editor-heading">
                <ChatModelAvatar familyId={selectedProvider.configuration.familyId} label={familyDisplayName(selectedProvider.configuration.familyId)} size={38} />
                <div class="min-w-0">
                  <h3>{selectedProvider.configuration.label}</h3>
                  <p class="font-mono" title={filesFolder ?? undefined}>{filesFolder ?? "\u00a0"}</p>
                </div>
              </div>
            {/if}

            {#if error}
              <Notice class="px-1 text-[0.733333rem]" message={error} onRetry={files.length === 0 && selectedInstanceId ? retryLoadFiles : undefined} />
            {/if}

            {#if loading}
              <Loading layout="fill" class="min-h-24" />
            {:else if selectedProvider && selectedInstanceId && files.length > 0}
              {#if note === "native" || docLinks.length > 0}
                <div class="flex flex-col gap-2 px-1">
                  {#if note === "native"}
                    <p class="text-[0.8rem] text-muted-foreground">{t("settings.agents.files.nativeNote")}</p>
                  {/if}
                  {#if docLinks.length > 0}
                    <div class="flex flex-wrap items-center gap-x-3 gap-y-1" role="group" aria-label={t("settings.agents.files.docsHeading")}>
                      {#each docLinks as link (link.url)}
                        <button type="button" class="inline-flex items-center gap-1 text-[0.8rem] font-medium text-foreground underline-offset-2 hover:underline" onclick={() => void openDocs(link.url)}>
                          {t(`settings.agents.files.docs.${link.kind}`)}
                          <ExternalLink size={11} />
                        </button>
                      {/each}
                    </div>
                  {/if}
                </div>
              {/if}

              {#each fileNotes as fileNote (fileNote.kind)}
                <Notice class="px-1 text-[0.733333rem]" tone="warning" role="status" copy={false} message={fileNoteMessage(fileNote)}>
                  {#snippet actions()}
                    <button type="button" class="notice-action-text" onclick={() => void openDocs(PROVIDER_FILE_NOTE_LINKS[fileNote.kind])}>{t("settings.agents.files.fileNoteDocs")}</button>
                  {/snippet}
                </Notice>
              {/each}

              {#key `${selectedInstanceId} ${editorKey}`}
                {#if groups.instructions.length > 0}
                  <section class="flex flex-col gap-3">
                    {@render sectionHeading(t("settings.agents.files.instructionsHeading"), t("settings.agents.files.instructionsDescription"))}
                    <ProviderFileEditor
                      files={groups.instructions}
                      instanceId={selectedInstanceId}
                      ariaLabel={t("settings.agents.files.instructionsHeading")}
                      placeholder={t("settings.agents.files.instructionsPlaceholder")}
                      onSaved={replaceFile}
                      bind:dirty={instructionsDirty}
                    />
                  </section>
                {/if}

                {#if preferenceFile?.preferences}
                  {@render separator()}
                  <section class="flex flex-col gap-3">
                    {@render sectionHeading(
                      t("settings.agents.files.preferencesHeading"),
                      t(preferencesAllowlisted ? "settings.agents.files.preferencesDescription" : "settings.agents.files.preferencesNativeDescription"),
                    )}
                    {#if !preferenceFile.preferences.editable}
                      <Notice class="px-1 text-[0.733333rem]" tone="warning" role="status" copy={false} message={t("settings.agents.files.preferencesNotEditable")} />
                    {:else if configurationDirty}
                      <Notice class="px-1 text-[0.733333rem]" tone="warning" role="status" copy={false} message={t("settings.agents.files.preferencesWaitForFile")} />
                    {/if}
                    {#if preferenceError}
                      <Notice class="px-1 text-[0.733333rem]" message={preferenceError} />
                    {/if}
                    <ProviderPreferenceList
                      preferences={preferenceFile.preferences}
                      allowlisted={preferencesAllowlisted}
                      disabled={savingPreference || configurationDirty}
                      onSave={savePreference}
                    />
                  </section>
                {/if}

                {#if groups.configuration.length > 0}
                  {#if groups.instructions.length > 0 || preferenceFile?.preferences}{@render separator()}{/if}
                  <section class="flex flex-col gap-3">
                    {@render sectionHeading(t("settings.agents.files.configurationHeading"), t("settings.agents.files.configurationDescription"))}
                    <ProviderFileEditor
                      files={groups.configuration}
                      instanceId={selectedInstanceId}
                      ariaLabel={t("settings.agents.files.configurationHeading")}
                      placeholder={t("settings.agents.files.configurationPlaceholder")}
                      onSaved={replaceFile}
                      bind:dirty={configurationDirty}
                    />
                  </section>
                {/if}
              {/key}
            {/if}
          </div>
          <CalendarScrollbar scrollContainer={detailScrollElement} wheelPassthrough />
        </div>
      </div>
    </div>
  {/if}
</section>

<style>
  .agent-files { display:grid; height:100%; min-height:0; grid-template-rows:auto minmax(0,1fr); gap:0.8rem; }
  .directory-header { display:flex; flex-wrap:wrap; align-items:start; justify-content:space-between; gap:0.75rem; padding-inline:0.25rem; }
  .directory-header h2 { font-size:calc(0.866667rem * var(--type-scale)); font-weight:600; }
  .settings-button { display:inline-flex; min-height:1.9rem; align-items:center; justify-content:center; gap:0.35rem; border:1px solid var(--border); border-radius:0.42rem; background:var(--background); padding:0.3rem 0.65rem; color:var(--foreground); font-size:calc(0.733333rem * var(--type-scale)); font-weight:600; line-height:1; white-space:nowrap; }
  .settings-button:hover:not(:disabled) { background:var(--accent); }
  .settings-button:disabled { cursor:not-allowed; opacity:0.5; }
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
  .directory-summary small { margin-top:0.05rem; color:var(--muted-foreground); font-size:calc(0.65rem * var(--type-scale)); }
  .detail-panel { position:relative; z-index:2; min-width:0; min-height:0; border-left:1px solid var(--border); padding-left:1rem; }
  .detail-scroll { display:grid; align-content:start; gap:1.25rem; padding:0.2rem 0.75rem 1rem 0; }
  .editor-heading { display:grid; min-width:0; grid-template-columns:auto minmax(0,1fr); align-items:center; gap:0.65rem; padding-inline:0.25rem; }
  .editor-heading h3,.editor-heading p { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .editor-heading h3 { font-size:calc(0.833333rem * var(--type-scale)); font-weight:600; }
  .editor-heading p { margin-top:0.08rem; color:var(--muted-foreground); font-size:calc(0.7rem * var(--type-scale)); }
  @media (max-width:700px) {
    .directory-layout { grid-template-columns:1fr; grid-template-rows:minmax(8rem,30%) minmax(0,1fr); }
    .directory-panel { border-bottom:1px solid var(--border); padding:0 0 0.75rem; }
    .detail-panel { border-left:0; padding:0.8rem 0 0; }
    .detail-scroll { padding-right:0.35rem; }
  }
</style>
