<script lang="ts">
  import { onMount } from "svelte";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Save from "@lucide/svelte/icons/save";
  import X from "@lucide/svelte/icons/x";
  import type { ProviderFileRead, ProviderInstanceId } from "$lib/chat/contracts";
  import {
    readChatProviderFiles,
    readPreviewModelOrigins,
    revokePreviewModelOrigin,
    saveChatProviderFile,
    type PreviewModelOriginGrant,
  } from "$lib/api/chat";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Select from "$lib/components/ui/Select.svelte";

  const { t } = getLocalization();
  const chat = getChat();
  let selectedInstanceId = $state<ProviderInstanceId | null>(null);
  let selectedFileId = $state<string | null>(null);
  let files = $state<ProviderFileRead[]>([]);
  let draft = $state("");
  let loading = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let loadGeneration = 0;
  let siteGrants = $state<PreviewModelOriginGrant[]>([]);
  let siteError = $state<string | null>(null);
  let revoking = $state(false);

  const providers = $derived(chat.settings?.providerInstances ?? []);
  const selectedProvider = $derived(
    providers.find((provider) => provider.configuration.instanceId === selectedInstanceId) ?? null,
  );
  const selectedFile = $derived(files.find((file) => file.fileId === selectedFileId) ?? null);
  const dirty = $derived(selectedFile !== null && draft !== selectedFile.contents);
  const providerOptions = $derived(providers.map((provider) => ({
    value: provider.configuration.instanceId,
    label: provider.configuration.label,
  })));
  const fileOptions = $derived(files.map((file) => ({
    value: file.fileId,
    label: fileLabel(file),
    summary: file.format.toUpperCase(),
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
    try {
      const next = await readChatProviderFiles(instanceId);
      if (generation !== loadGeneration || instanceId !== selectedInstanceId) return;
      files = next;
      const preferred = next.find((file) => file.fileId === selectedFileId) ?? next[0] ?? null;
      selectedFileId = preferred?.fileId ?? null;
      draft = preferred?.contents ?? "";
    } catch (cause: unknown) {
      if (generation !== loadGeneration) return;
      files = [];
      selectedFileId = null;
      draft = "";
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
    selectedFileId = null;
    selectedInstanceId = instanceId;
  }

  function chooseFile(file: ProviderFileRead): void {
    if (file.fileId === selectedFileId) return;
    if (dirty) {
      error = t("settings.chat.permissions.saveOrReload");
      return;
    }
    selectedFileId = file.fileId;
    draft = file.contents;
    error = null;
  }

  async function save(): Promise<void> {
    if (!selectedProvider || !selectedFile || !dirty || saving) return;
    saving = true;
    error = null;
    try {
      const saved = await saveChatProviderFile({
        instanceId: selectedProvider.configuration.instanceId,
        fileId: selectedFile.fileId,
        contents: draft,
        expectedRevision: selectedFile.revision,
      });
      files = files.map((file) => file.fileId === saved.fileId ? saved : file);
      draft = saved.contents;
    } catch (cause: unknown) {
      error = errorMessage(cause);
    } finally {
      saving = false;
    }
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

  function fileLabel(file: ProviderFileRead): string {
    return file.name;
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

  function threadLabel(threadId: string): string {
    return [...chat.activeThreads, ...chat.archivedThreads].find((thread) => thread.id === threadId)?.title || threadId;
  }

  onMount(() => {
    void loadSiteGrants().catch(() => undefined);
  });

  function errorMessage(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }
</script>

<section class="flex min-h-0 flex-col gap-4">
  <div class="px-1">
    <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("settings.chat.permissions.heading")}</h2>
  </div>

  {#if providers.length === 0}
    <p class="rounded-lg border border-dashed border-border p-4 text-sm text-muted-foreground">
      {t("settings.chat.permissions.empty")}
    </p>
  {:else}
    <Select
      label={t("settings.chat.permissions.providerLabel")}
      description={t("settings.chat.permissions.providerDescription")}
      value={selectedInstanceId ?? ""}
      options={providerOptions}
      onChange={(value) => chooseProvider(value)}
      ariaLabel={t("settings.chat.permissions.providerLabel")}
    />

    {#if error}
      <Notice class="px-1 text-[0.733333rem]" message={error} onRetry={files.length === 0 && selectedInstanceId ? retryLoadFiles : undefined} />
    {/if}

    {#if loading}
      <Loading layout="fill" class="min-h-24" />
    {:else if selectedProvider && files.length > 0}
      <div class="flex min-h-0 flex-col gap-3">
        <div class="flex flex-wrap items-end justify-between gap-3 px-1">
          <div class="min-w-48 flex-1">
            <Select
              inline
              class="w-full"
              value={selectedFileId ?? ""}
              options={fileOptions}
              onChange={(value) => { const file = files.find((entry) => entry.fileId === value); if (file) chooseFile(file); }}
              ariaLabel={t("settings.chat.permissions.fileLabel")}
            />
          </div>
          <div class="flex gap-2">
            <button type="button" class="chat-settings-button" disabled={loading || saving} onclick={reload}>
              <RefreshCw size={13} />
              {dirty ? t("settings.chat.permissions.discardAndReload") : t("settings.chat.permissions.reload")}
            </button>
            <button type="button" class="setup-primary-button gap-1.5" disabled={!dirty || saving} onclick={() => void save()}>
              <Save size={13} />
              {saving ? t("settings.chat.permissions.saving") : t("settings.chat.permissions.save")}
            </button>
          </div>
        </div>

        {#if selectedFile}
          <div class="px-1">
            <p class="break-all font-mono text-[0.7rem] text-muted-foreground">{selectedFile.path}</p>
            <p class="mt-1 text-[0.666667rem] text-muted-foreground">
              {selectedFile.exists
                ? t("settings.chat.permissions.existingFile", selectedFile.format.toUpperCase())
                : t("settings.chat.permissions.newFile", selectedFile.format.toUpperCase())}
            </p>
          </div>
          <div class="overflow-hidden rounded-floating-item border border-border">
            <textarea
              class="h-[min(26rem,48vh)] min-h-48 w-full resize-y bg-transparent p-3 font-mono text-[0.766667rem] leading-5 text-foreground outline-none placeholder:text-muted-foreground"
              value={draft}
              aria-label={fileLabel(selectedFile)}
              placeholder={selectedFile.kind === "instructions"
                ? t("settings.chat.permissions.instructionsPlaceholder")
                : t("settings.chat.permissions.configurationPlaceholder")}
              spellcheck="false"
              autocapitalize="off"
              oninput={(event) => { draft = event.currentTarget.value; error = null; }}
            ></textarea>
            <div class="flex flex-wrap items-center justify-between gap-2 border-t border-border px-3 py-2 text-[0.666667rem] text-muted-foreground">
              <span>{t("settings.chat.permissions.fileLimit")}</span>
              {#if dirty}<span class="font-medium text-status-tentative">{t("settings.chat.permissions.unsaved")}</span>{/if}
            </div>
          </div>
        {/if}
      </div>
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
</section>
