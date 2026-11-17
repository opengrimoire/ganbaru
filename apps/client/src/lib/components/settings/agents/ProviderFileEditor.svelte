<script lang="ts">
  import Save from "@lucide/svelte/icons/save";
  import type { ProviderFileRead, ProviderInstanceId } from "$lib/chat/contracts";
  import { saveChatProviderFile } from "$lib/api/chat";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import { SETTINGS_PRIMARY_BUTTON_CLASS } from "$lib/components/settings/action-buttons";

  /**
   * Text editor for one group of provider files. The editor shows the saved contents until the
   * user types, so a file another control saves stays current while nothing is pending.
   */
  let {
    files,
    instanceId,
    ariaLabel,
    placeholder,
    onSaved,
    dirty = $bindable(false),
  }: {
    files: ProviderFileRead[];
    instanceId: ProviderInstanceId;
    ariaLabel: string;
    placeholder: string;
    onSaved: (file: ProviderFileRead) => void;
    dirty?: boolean;
  } = $props();

  const { t } = getLocalization();
  let selectedFileId = $state<string | null>(null);
  let draft = $state<string | null>(null);
  let saving = $state(false);
  let error = $state<string | null>(null);

  const selectedFile = $derived(files.find((file) => file.fileId === selectedFileId) ?? files[0] ?? null);
  const contents = $derived(draft ?? selectedFile?.contents ?? "");
  const pending = $derived(selectedFile !== null && draft !== null && draft !== selectedFile.contents);
  const fileOptions = $derived(files.map((file) => ({
    value: file.fileId,
    label: file.name,
    summary: file.format.toUpperCase(),
  })));

  $effect(() => {
    dirty = pending;
  });

  function chooseFile(fileId: string): void {
    if (fileId === selectedFile?.fileId) return;
    if (pending) {
      error = t("settings.agents.files.saveOrReload");
      return;
    }
    selectedFileId = fileId;
    draft = null;
    error = null;
  }

  async function save(): Promise<void> {
    if (!selectedFile || draft === null || !pending || saving) return;
    saving = true;
    error = null;
    try {
      const saved = await saveChatProviderFile({
        instanceId,
        fileId: selectedFile.fileId,
        contents: draft,
        expectedRevision: selectedFile.revision,
      });
      draft = null;
      onSaved(saved);
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.settingsAction"));
    } finally {
      saving = false;
    }
  }
</script>

{#if selectedFile}
  <div class="flex min-h-0 flex-col gap-2">
    <div class="flex flex-wrap items-end justify-between gap-3 px-1">
      <div class="min-w-48 flex-1">
        {#if files.length > 1}
          <Select inline class="w-full" value={selectedFile.fileId} options={fileOptions} onChange={chooseFile} ariaLabel={t("settings.agents.files.fileLabel")} />
        {:else}
          <p class="text-[0.8rem] font-medium text-foreground">{selectedFile.name}</p>
        {/if}
      </div>
      <button type="button" class={SETTINGS_PRIMARY_BUTTON_CLASS} disabled={!pending || saving} onclick={() => void save()}>
        <Save size={13} />
        {saving ? t("settings.agents.files.saving") : t("settings.agents.files.save")}
      </button>
    </div>
    <p class="break-all px-1 font-mono text-[0.7rem] text-muted-foreground">{selectedFile.path}</p>
    {#if error}
      <Notice class="px-1 text-[0.733333rem]" message={error} />
    {/if}
    <div class="overflow-hidden rounded-floating-item border border-border">
      <textarea
        class="h-[min(26rem,48vh)] min-h-48 w-full resize-y bg-transparent p-3 font-mono text-[0.766667rem] leading-5 text-foreground outline-none placeholder:text-muted-foreground"
        value={contents}
        aria-label={`${ariaLabel}: ${selectedFile.name}`}
        {placeholder}
        spellcheck="false"
        autocapitalize="off"
        oninput={(event) => { draft = event.currentTarget.value; error = null; }}
      ></textarea>
      <div class="flex flex-wrap items-center justify-between gap-2 border-t border-border px-3 py-2 text-[0.666667rem] text-muted-foreground">
        <span>{t("settings.agents.files.fileLimit")}</span>
        {#if pending}<span class="font-medium text-status-tentative">{t("settings.agents.files.unsaved")}</span>{/if}
      </div>
    </div>
  </div>
{/if}
