<script lang="ts">
  import type { ProviderFileRead } from "$lib/chat/contracts";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Select from "$lib/components/ui/Select.svelte";

  /**
   * Text editor for one group of provider files. Edits stay drafts that the page's footer saves,
   * and each file shows its saved contents until the user types in it, so switching files keeps
   * every draft.
   */
  let {
    files,
    ariaLabel,
    placeholder,
    drafts,
    disabled,
    onDraft,
  }: {
    files: ProviderFileRead[];
    ariaLabel: string;
    placeholder: string;
    /** Edited text, keyed by file ID. */
    drafts: Readonly<Record<string, string>>;
    disabled: boolean;
    onDraft: (fileId: string, contents: string) => void;
  } = $props();

  const { t } = getLocalization();
  let selectedFileId = $state<string | null>(null);

  const selectedFile = $derived(files.find((file) => file.fileId === selectedFileId) ?? files[0] ?? null);
  const contents = $derived(selectedFile ? drafts[selectedFile.fileId] ?? selectedFile.contents : "");
  const pending = $derived(selectedFile !== null && contents !== selectedFile.contents);
  const fileOptions = $derived(files.map((file) => ({
    value: file.fileId,
    label: file.name,
    summary: file.format.toUpperCase(),
  })));
</script>

{#if selectedFile}
  <div class="flex min-h-0 flex-col gap-3">
    <div class="px-1 py-1">
      {#if files.length > 1}
        <Select inline class="w-full" value={selectedFile.fileId} options={fileOptions} onChange={(fileId) => { selectedFileId = fileId; }} ariaLabel={t("settings.agents.files.fileLabel")} />
      {:else}
        <div class="text-[0.866667rem] text-foreground">{selectedFile.name}</div>
      {/if}
      <div class="mt-0.5 wrap-break-word text-[0.8rem] leading-5 text-muted-foreground">{selectedFile.path}</div>
    </div>
    <div class="overflow-hidden rounded-floating-item border border-border">
      <textarea
        class="h-[min(26rem,48vh)] min-h-48 w-full resize-y bg-transparent p-3 font-mono text-[0.766667rem] leading-5 text-foreground outline-none placeholder:text-muted-foreground"
        value={contents}
        aria-label={`${ariaLabel}: ${selectedFile.name}`}
        {placeholder}
        {disabled}
        spellcheck="false"
        autocapitalize="off"
        oninput={(event) => onDraft(selectedFile.fileId, event.currentTarget.value)}
      ></textarea>
      <div class="flex flex-wrap items-center justify-between gap-2 border-t border-border px-3 py-2 text-[0.666667rem] text-muted-foreground">
        <span>{t("settings.agents.files.fileLimit")}</span>
        {#if pending}<span class="font-medium text-status-tentative">{t("settings.agents.files.unsaved")}</span>{/if}
      </div>
    </div>
  </div>
{/if}
