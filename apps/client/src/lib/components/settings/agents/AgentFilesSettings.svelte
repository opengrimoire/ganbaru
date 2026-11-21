<script lang="ts">
  import { untrack } from "svelte";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Save from "@lucide/svelte/icons/save";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type {
    ProviderFamilyMetadataRead,
    ProviderFileNote,
    ProviderFileRead,
    ProviderInstanceId,
    ProviderPreferenceEdit,
  } from "$lib/chat/contracts";
  import type { ChatProviderSetupTarget } from "$lib/settings/types";
  import {
    readChatProviderFiles,
    saveChatProviderFile,
  } from "$lib/api/chat";
  import {
    PROVIDER_DOC_LINKS,
    PROVIDER_FILE_NOTE_LINKS,
    PROVIDER_FILE_NOTE_NAMES,
    providerFileSaves,
    providerFilesFolder,
    providerPreferencesAllowlisted,
    splitProviderFiles,
    type ProviderPreferenceDrafts,
  } from "$lib/chat/provider-files";
  import { providerFamilyDisplayName } from "$lib/chat/provider-setup";
  import { formatList } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { getChat } from "$lib/stores/chat.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { SETTINGS_PRIMARY_BUTTON_CLASS, SETTINGS_SECONDARY_BUTTON_CLASS } from "$lib/components/settings/action-buttons";
  import { familiesInPickerOrder, providersInPickerOrder } from "$lib/chat/composer/model-picker-model";
  import ChatModelAvatar from "$lib/components/chat/identity/ChatModelAvatar.svelte";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import ProviderFileEditor from "./ProviderFileEditor.svelte";
  import ProviderPreferenceList from "./ProviderPreferenceList.svelte";
  import UnconfiguredProviderFamily from "./UnconfiguredProviderFamily.svelte";
  import { unconfiguredFamilySituation } from "./provider-install-steps";

  let {
    onOpenProviderSetup,
  }: {
    onOpenProviderSetup: (target: ChatProviderSetupTarget) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const chat = getChat();
  let selectedInstanceId = $state<ProviderInstanceId | null>(null);
  /** A family with no configuration whose setup steps the detail panel shows instead of files. */
  let selectedFamilyId = $state<string | null>(null);
  let files = $state<ProviderFileRead[]>([]);
  let fileNotes = $state<ProviderFileNote[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let loadGeneration = 0;
  let editorKey = $state(0);
  let directoryScrollElement = $state<HTMLElement>();
  let detailScrollElement = $state<HTMLElement>();
  /** Edited file text, keyed by file ID, that the footer saves. */
  let textDrafts = $state<Record<string, string>>({});
  /** Unsaved preference values, keyed by file ID, that the footer saves. */
  let preferenceDrafts = $state<Record<string, ProviderPreferenceDrafts>>({});
  let saving = $state(false);
  let saveError = $state<string | null>(null);
  /** Folder each provider reads its files from, for the directory rows. */
  let providerFolders = $state<ReadonlyMap<ProviderInstanceId, string>>(new Map());
  let folderGeneration = 0;

  // Providers and families follow the model picker order, so every provider list reads the same way.
  const providers = $derived(providersInPickerOrder(chat.settings?.providerInstances ?? [], Date.now()));
  const providerIdsKey = $derived(providers.map((provider) => provider.configuration.instanceId).sort().join("\n"));
  const selectedProvider = $derived(
    providers.find((provider) => provider.configuration.instanceId === selectedInstanceId) ?? null,
  );
  const familyId = $derived(selectedProvider?.configuration.familyId ?? null);
  const pendingSaves = $derived(providerFileSaves(files, textDrafts, preferenceDrafts));
  const dirty = $derived(pendingSaves.length > 0);
  const groups = $derived(splitProviderFiles(files));
  const preferenceFile = $derived(groups.configuration.find((file) => file.preferences !== null) ?? null);
  const preferencesAllowlisted = $derived(familyId ? providerPreferencesAllowlisted(familyId) : false);
  const docLinks = $derived(familyId ? PROVIDER_DOC_LINKS[familyId] ?? [] : []);
  const showInstructions = $derived(groups.instructions.length > 0 || fileNotes.length > 0);
  const showPreferences = $derived(Boolean(preferenceFile?.preferences));
  const showConfiguration = $derived(groups.configuration.length > 0);
  const families = $derived(familiesInPickerOrder((chat.settings?.providerFamilies ?? []).map((family): ProviderFamilyMetadataRead => ({
    ...family,
    displayName: providerFamilyDisplayName(family, t("chat.providers.acp.name")),
  })), providers, Date.now()));
  const familyNames = $derived(new Map(families.map((family): [string, string] => [family.familyId, family.displayName])));
  /** Families this device could set up that have no configuration yet, listed after the configured providers. */
  const unconfiguredFamilies = $derived(families.filter((family) => family.implementationStatus === "available"
    && !providers.some((provider) => provider.configuration.familyId === family.familyId)));
  const selectedFamily = $derived(unconfiguredFamilies.find((family) => family.familyId === selectedFamilyId) ?? null);

  $effect(() => {
    if (selectedFamily) return;
    if (providers.some((provider) => provider.configuration.instanceId === selectedInstanceId)) return;
    // A family that was just set up, such as by its Refresh step, stays selected through its new provider.
    const first = providers.find((provider) => provider.configuration.familyId === selectedFamilyId) ?? providers[0];
    if (first) {
      selectedFamilyId = null;
      selectedInstanceId = first.configuration.instanceId;
      return;
    }
    selectedInstanceId = null;
    files = [];
    fileNotes = [];
    selectedFamilyId = unconfiguredFamilies[0]?.familyId ?? null;
  });

  $effect(() => {
    const instanceId = selectedInstanceId;
    if (instanceId) void loadFiles(instanceId);
  });

  $effect(() => {
    void providerIdsKey;
    void loadProviderFolders(untrack(() => providers.map((provider) => provider.configuration.instanceId)));
  });

  /**
   * Reads every provider's files once to name the folder under each directory row. A provider whose
   * read fails shows no folder; selecting it reports the failure in the detail panel.
   */
  async function loadProviderFolders(instanceIds: readonly ProviderInstanceId[]): Promise<void> {
    const generation = ++folderGeneration;
    const entries = await Promise.all(instanceIds.map(async (instanceId): Promise<[ProviderInstanceId, string | null]> => {
      try {
        return [instanceId, providerFilesFolder((await readChatProviderFiles(instanceId)).files)];
      } catch {
        return [instanceId, null];
      }
    }));
    if (generation !== folderGeneration) return;
    providerFolders = new Map(entries.filter((entry): entry is [ProviderInstanceId, string] => entry[1] !== null));
  }

  function rememberProviderFolder(instanceId: ProviderInstanceId, folder: string | null): void {
    const next = new Map(providerFolders);
    if (folder === null) next.delete(instanceId);
    else next.set(instanceId, folder);
    providerFolders = next;
  }

  async function loadFiles(instanceId: ProviderInstanceId): Promise<void> {
    const generation = ++loadGeneration;
    loading = true;
    error = null;
    discardDrafts();
    try {
      const next = await readChatProviderFiles(instanceId);
      if (generation !== loadGeneration || instanceId !== selectedInstanceId) return;
      files = next.files;
      fileNotes = next.notes;
      rememberProviderFolder(instanceId, providerFilesFolder(next.files));
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
    selectedFamilyId = null;
    selectedInstanceId = instanceId;
  }

  function chooseFamily(familyId: string): void {
    if (familyId === selectedFamilyId) return;
    if (dirty) {
      error = t("settings.agents.files.saveOrReload");
      return;
    }
    error = null;
    selectedInstanceId = null;
    files = [];
    fileNotes = [];
    selectedFamilyId = familyId;
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

  function discardDrafts(): void {
    textDrafts = {};
    preferenceDrafts = {};
    saveError = null;
  }

  function changeText(fileId: string, contents: string): void {
    textDrafts = { ...textDrafts, [fileId]: contents };
    saveError = null;
  }

  function changePreference(fileId: string, preferenceId: string, value: ProviderPreferenceEdit["value"]): void {
    preferenceDrafts = { ...preferenceDrafts, [fileId]: { ...preferenceDrafts[fileId], [preferenceId]: value } };
    saveError = null;
  }

  function withoutFile<T>(drafts: Readonly<Record<string, T>>, fileId: string): Record<string, T> {
    return Object.fromEntries(Object.entries(drafts).filter(([key]) => key !== fileId));
  }

  /**
   * Saves every pending file edit and preference change, one file at a time. A failed save stops
   * the rest and keeps their drafts, while files already written drop theirs.
   */
  async function saveAll(): Promise<void> {
    const instanceId = selectedInstanceId;
    const saves = pendingSaves;
    if (!instanceId || saving || saves.length === 0) return;
    saving = true;
    saveError = null;
    try {
      for (const save of saves) {
        replaceFile(await saveChatProviderFile(instanceId, save));
        textDrafts = withoutFile(textDrafts, save.fileId);
        preferenceDrafts = withoutFile(preferenceDrafts, save.fileId);
      }
    } catch (cause: unknown) {
      saveError = errorMessage(cause);
    } finally {
      saving = false;
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

  /** Whether an unconfigured family's CLI is missing or only waits for setup. */
  function familyStatus(family: ProviderFamilyMetadataRead): string {
    return unconfiguredFamilySituation(chat.settings?.installedExecutables[family.familyId]).kind === "not-installed"
      ? t("settings.agents.providers.notInstalled")
      : t("settings.agents.providers.notConfigured");
  }

  function familyDisplayName(familyId: string): string {
    return familyNames.get(familyId) ?? familyId;
  }

  function errorMessage(cause: unknown): string {
    return chatErrorMessage(cause, t("chatErrors.actions.settingsAction"));
  }
</script>

{#snippet separator()}
  <div class="h-px shrink-0 bg-border/50" aria-hidden="true"></div>
{/snippet}

{#snippet sectionHeading(title: string)}
  <h3 class="px-1 text-[0.866667rem] font-semibold text-foreground">{title}</h3>
{/snippet}

<section class="agent-files" data-agents-settings-subsection="files">
  {#if chat.loading}
    <Loading layout="rows" rows={3} />
  {:else if providers.length === 0 && unconfiguredFamilies.length === 0}
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
                {@const folder = providerFolders.get(instanceId)}
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
                    <small>{folder ?? "\u00a0"}</small>
                  </span>
                </button>
              {/each}
              {#each unconfiguredFamilies as family (family.familyId)}
                {@const active = family.familyId === selectedFamilyId}
                <button
                  type="button"
                  class="provider-row"
                  class:active
                  aria-current={active ? "page" : undefined}
                  onclick={() => chooseFamily(family.familyId)}
                >
                  <ChatModelAvatar familyId={family.familyId} label={family.displayName} size={32} />
                  <span class="directory-summary">
                    <strong>{family.displayName}</strong>
                    <small class="unconfigured-state"><i></i>{familyStatus(family)}</small>
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
            {#if selectedFamily}
              <UnconfiguredProviderFamily
                family={selectedFamily}
                installedExecutables={chat.settings?.installedExecutables[selectedFamily.familyId]}
                onSetUp={() => onOpenProviderSetup({ mode: "create", familyId: selectedFamily.familyId })}
              />
            {/if}

            {#if error}
              <Notice class="px-1 text-[0.8rem]" message={error} onRetry={files.length === 0 && selectedInstanceId ? retryLoadFiles : undefined} />
            {/if}

            {#if loading && !selectedFamily}
              <Loading layout="fill" class="min-h-24" />
            {:else if selectedProvider && selectedInstanceId && files.length > 0}
              {#key `${selectedInstanceId} ${editorKey}`}
                {#if showInstructions}
                  <section class="flex flex-col gap-4">
                    {@render sectionHeading(t("settings.agents.files.instructionsHeading"))}
                    {#each fileNotes as fileNote (fileNote.kind)}
                      <Notice class="px-1 text-[0.8rem]" tone="warning" role="status" copy={false} message={fileNoteMessage(fileNote)}>
                        {#snippet actions()}
                          <button type="button" class="notice-action-text" onclick={() => void openDocs(PROVIDER_FILE_NOTE_LINKS[fileNote.kind])}>{t("settings.agents.files.fileNoteDocs")}</button>
                        {/snippet}
                      </Notice>
                    {/each}
                    {#if groups.instructions.length > 0}
                      <ProviderFileEditor
                        files={groups.instructions}
                        ariaLabel={t("settings.agents.files.instructionsHeading")}
                        placeholder={t("settings.agents.files.instructionsPlaceholder")}
                        drafts={textDrafts}
                        disabled={saving}
                        onDraft={changeText}
                      />
                    {/if}
                  </section>
                {/if}

                {#if preferenceFile?.preferences}
                  {@const fileId = preferenceFile.fileId}
                  {#if showInstructions}{@render separator()}{/if}
                  <section class="flex flex-col gap-4">
                    {@render sectionHeading(t("settings.agents.files.preferencesHeading"))}
                    {#if !preferenceFile.preferences.editable}
                      <Notice class="px-1 text-[0.8rem]" tone="warning" role="status" copy={false} message={t("settings.agents.files.preferencesNotEditable")} />
                    {/if}
                    <ProviderPreferenceList
                      preferences={preferenceFile.preferences}
                      allowlisted={preferencesAllowlisted}
                      disabled={saving}
                      drafts={preferenceDrafts[fileId] ?? {}}
                      onChange={(preferenceId, value) => changePreference(fileId, preferenceId, value)}
                    />
                  </section>
                {/if}

                {#if showConfiguration}
                  {#if showInstructions || showPreferences}{@render separator()}{/if}
                  <section class="flex flex-col gap-4">
                    {@render sectionHeading(t("settings.agents.files.configurationHeading"))}
                    <ProviderFileEditor
                      files={groups.configuration}
                      ariaLabel={t("settings.agents.files.configurationHeading")}
                      placeholder={t("settings.agents.files.configurationPlaceholder")}
                      drafts={textDrafts}
                      disabled={saving}
                      onDraft={changeText}
                    />
                  </section>
                {/if}
              {/key}

              {#if docLinks.length > 0}
                {#if showInstructions || showPreferences || showConfiguration}{@render separator()}{/if}
                <section class="flex flex-col gap-4">
                  {@render sectionHeading(t("settings.agents.files.docsHeading"))}
                  <div class="flex flex-wrap gap-2 px-1">
                    {#each docLinks as link (link.url)}
                      <button type="button" class={SETTINGS_SECONDARY_BUTTON_CLASS} onclick={() => void openDocs(link.url)}>
                        <span>{t(`settings.agents.files.docs.${link.kind}`)}</span>
                        <ExternalLink size={13} strokeWidth={2.25} aria-hidden="true" />
                      </button>
                    {/each}
                  </div>
                </section>
              {/if}
            {/if}
          </div>
          <CalendarScrollbar scrollContainer={detailScrollElement} wheelPassthrough />
        </div>
        {#if selectedProvider}
          <footer class="detail-footer">
            <div>
              <button type="button" class={SETTINGS_SECONDARY_BUTTON_CLASS} disabled={loading || saving} onclick={reload}>
                <RefreshCw size={14} strokeWidth={1.9} class="shrink-0" />
                <span>{dirty ? t("settings.agents.files.discardAndReload") : t("settings.agents.files.reload")}</span>
              </button>
            </div>
            <div class="save-area">
              {#if saveError}<span class="save-error" role="alert">{saveError}</span>{/if}
              <button type="button" class={SETTINGS_PRIMARY_BUTTON_CLASS} disabled={saving || !dirty} onclick={() => void saveAll()}>
                <Save size={14} strokeWidth={1.9} class="shrink-0" />
                <span>{saving ? t("settings.agents.files.saving") : t("settings.agents.files.save")}</span>
              </button>
            </div>
          </footer>
        {/if}
      </div>
    </div>
  {/if}
</section>

<style>
  .agent-files { display:grid; height:100%; min-height:0; grid-template-rows:minmax(0,1fr); }
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
  .unconfigured-state { display:flex; align-items:center; gap:0.3rem; }
  .unconfigured-state i { width:0.38rem; height:0.38rem; flex:0 0 auto; border-radius:50%; background:var(--destructive); }
  .detail-panel { position:relative; z-index:2; display:grid; min-width:0; min-height:0; grid-template-rows:minmax(0,1fr) auto; border-left:1px solid var(--border); padding-left:1rem; }
  .detail-footer,.detail-footer > div { display:flex; min-width:0; align-items:center; gap:0.5rem; }
  .detail-footer { justify-content:space-between; gap:0.75rem; border-top:1px solid var(--border); padding:0.65rem 0.75rem 0.65rem 0; }
  .save-area { margin-left:auto; justify-content:flex-end; }
  .save-error { min-width:0; overflow-wrap:anywhere; color:var(--destructive); font-size:calc(0.733333rem * var(--type-scale)); }
  .detail-scroll { display:grid; grid-template-columns:minmax(0,1fr); align-content:start; gap:1.5rem; padding:0.2rem 0.75rem 1rem 0; }
  @media (max-width:700px) {
    .directory-layout { grid-template-columns:1fr; grid-template-rows:minmax(8rem,30%) minmax(0,1fr); }
    .directory-panel { border-bottom:1px solid var(--border); padding:0 0 0.75rem; }
    .detail-panel { border-left:0; padding:0.8rem 0 0; }
    .detail-scroll { padding-right:0.35rem; }
    .detail-footer { align-items:stretch; flex-direction:column; padding-right:0; }
    .detail-footer > div { justify-content:space-between; }
    .save-area :global(button) { flex:1; }
  }
</style>
