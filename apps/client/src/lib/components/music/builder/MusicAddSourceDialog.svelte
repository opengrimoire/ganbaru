<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { untrack } from "svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicAddSourceKind, MusicLocalSourceSelection } from "$lib/music/sources/drafts";
  import type { MusicSourcesController } from "$lib/music/sources/controller.svelte";
  import type { MusicYouTubeSourcePreview } from "$lib/music/sources/youtube-source-resolver";
  import MusicBuilderDialog from "./MusicBuilderDialog.svelte";
  import MusicYouTubeSourcePreviewPanel from "./MusicYouTubeSourcePreview.svelte";

  let {
    controller,
    kind,
    localSource = null,
    onClose,
    onSaved,
  }: {
    controller: MusicSourcesController;
    kind: MusicAddSourceKind;
    localSource?: MusicLocalSourceSelection | null;
    onClose: () => void;
    onSaved: () => void;
  } = $props();

  const { t } = getLocalization();
  let localSelection = $state(untrack(() => localSource?.selection ?? null));
  let localRelationship = $state<MusicLocalSourceSelection["relationship"]>(untrack(() => localSource?.relationship ?? "separate"));
  let name = $state(untrack(() => localSource?.name ?? ""));
  let link = $state("");
  let preview = $state<MusicYouTubeSourcePreview | null>(null);
  let error = $state<string | null>(null);
  let saving = $state(false);

  function dialogTitle(): string {
    if (kind === "local-root") return t("music.builder.localFolder");
    return t("music.builder.youtube");
  }

  async function chooseLocal(): Promise<void> {
    error = null;
    const result = await controller.chooseLocalFolder();
    if (!result) return;
    localSelection = result.selection;
    localRelationship = result.relationship;
    name = result.name;
  }

  async function resolveLink(): Promise<void> {
    const parsed = controller.parseYouTubeInput(link);
    if (!parsed.source) {
      error = parsed.error;
      return;
    }
    error = null;
    preview = await controller.resolveYouTube(parsed.source);
    if (!preview) error = controller.resolutionError;
    else if (!name.trim()) name = preview.title;
  }

  async function saveLocal(): Promise<void> {
    if (!localSelection || localRelationship === "duplicate") return;
    saving = true;
    error = null;
    try {
      await controller.addLocalFolder(localSelection, name);
      onSaved();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      saving = false;
    }
  }

  async function saveYouTube(): Promise<void> {
    if (!preview) return;
    saving = true;
    error = null;
    try {
      await controller.addYouTube(preview, name);
      onSaved();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      saving = false;
    }
  }
</script>

<MusicBuilderDialog
  title={dialogTitle()}
  titleId="music-add-source-title"
  size={preview ? "large" : "small"}
  dismissDisabled={controller.busy || controller.resolving || saving}
  onDismiss={onClose}
>
  <div class="source-dialog-scroll" data-add-source-kind={kind}>
    {#if kind === "local-root"}
      {#if localSelection}
        <div class="rounded-md bg-secondary/55 p-3">
          <p class="truncate text-sm font-medium" title={localSelection.folderPath}>{localSelection.displayName ?? localSelection.folderPath.split(/[\\/]/).filter(Boolean).at(-1)}</p>
          <p class="mt-1 text-xs text-muted-foreground">{t("music.builder.previewFound", localSelection.tracks.length)}</p>
          {#if localSelection.truncated}<p class="mt-2 text-xs leading-relaxed text-muted-foreground">{t("music.builder.previewTruncated")}</p>{/if}
        </div>
        <label class="mt-4 block"><span class="mb-1.5 block text-xs font-medium">{t("music.builder.sourceName")}</span><input data-dialog-autofocus bind:value={name} class="source-input field h-11" maxlength="200" /></label>
        {#if localRelationship === "duplicate"}<p class="mt-3 text-xs text-destructive">{t("music.builder.duplicateFolder")}</p>{:else if localRelationship !== "separate"}<p class="mt-3 text-xs text-destructive">{t("music.builder.nestedFolder")}</p>{/if}
        <button type="button" class="mt-3 text-xs font-medium text-primary hover:underline" onclick={() => { void chooseLocal(); }}>{t("music.builder.chooseDifferentFolder")}</button>
      {/if}
    {:else}
      <label class="block">
        <span class="sr-only">{t("music.builder.youtubeLink")}</span>
        <span class="grid grid-cols-[minmax(0,1fr)_auto] gap-2 max-[480px]:grid-cols-1">
          <input data-dialog-autofocus bind:value={link} oninput={() => { if (preview) name = ""; preview = null; error = null; }} class="source-input field h-11 min-w-0" placeholder="https://www.youtube.com/…" />
          <button type="button" onclick={() => { void resolveLink(); }} disabled={controller.resolving || !link.trim()} class="source-action h-11">{#if controller.resolving}<LoaderCircle class="animate-spin motion-reduce:animate-none" size={15} />{:else}{t("music.builder.resolveLink")}{/if}</button>
        </span>
      </label>
      {#if preview}
        <div class="mt-4"><MusicYouTubeSourcePreviewPanel {preview} /></div>
        {#if preview.kind === "youtube-playlist"}<label class="mt-4 block"><span class="mb-1.5 block text-xs font-medium">{t("music.builder.sourceName")}</span><input bind:value={name} class="source-input field h-11" maxlength="200" /></label>{/if}
        {#if preview.kind === "youtube-playlist" && preview.duplicateCount > 0}<p class="mt-2 text-xs text-muted-foreground">{t("music.builder.knownDuplicates", preview.duplicateCount)}</p>{/if}
      {/if}
    {/if}
    {#if error}<Notice class="mt-3 text-sm" message={error} />{/if}
  </div>

  {#snippet footer()}
    <button type="button" onclick={onClose} disabled={controller.busy || controller.resolving || saving} class="source-cancel">{t("music.builder.cancel")}</button>
    {#if kind === "local-root"}
      <button type="button" onclick={() => { void saveLocal(); }} disabled={!localSelection || !name.trim() || localRelationship === "duplicate" || saving} class="source-save">{t("music.builder.saveAndScan")}</button>
    {:else if preview}
      <button type="button" onclick={() => { void saveYouTube(); }} disabled={saving} class="source-save">{t("music.builder.addToLibrary")}</button>
    {/if}
  {/snippet}
</MusicBuilderDialog>

<style>
  .source-dialog-scroll { scrollbar-width: thin; scrollbar-color: color-mix(in srgb, var(--foreground) 18%, transparent) transparent; }
  .source-action { border-radius: var(--floating-item-radius); }
  .source-input { width: 100%; padding-inline: 0.75rem; color: var(--foreground); font-size: calc(0.875rem * var(--type-scale)); }
  .source-action, .source-save, .source-cancel { display: inline-flex; align-items: center; justify-content: center; gap: 0.35rem; border: 1px solid var(--border); padding-inline: 0.95rem; font-size: calc(0.875rem * var(--type-scale)); font-weight: 500; white-space: nowrap; }
  .source-action { background: var(--primary); color: var(--primary-foreground); }
  .source-save, .source-cancel { min-height: 3rem; border-radius: 0.375rem; padding-block: 0.5rem; }
  .source-save { background: var(--primary); color: var(--primary-foreground); }
  .source-cancel { background: var(--card); color: var(--foreground); }
  .source-cancel:hover { background: var(--accent); }
  .source-action:disabled, .source-save:disabled { opacity: 0.45; }
</style>
