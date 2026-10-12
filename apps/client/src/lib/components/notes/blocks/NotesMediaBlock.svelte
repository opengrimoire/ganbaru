<script lang="ts">
  import { tick } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    notesFileAssetUrl,
    pickNotesFileAsset,
    releaseNotesFileAssetUrl,
  } from "$lib/api/notes/file-assets";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import {
    mediaCaptionPlainText,
    mediaDisplayNameFromSource,
    mediaDisplayName,
    mediaManagedAssetMetadata,
    mediaPlainText,
    mediaPreviewKindForUrl,
    mediaSourceUrl,
    mediaSourceKind,
    mediaUrlIssue,
    type NotesFileAssetMetadata,
    type NotesMediaSourceKind,
    type NotesMediaUrlIssue,
    type NotesMediaBlockType,
  } from "$lib/notes/block-types/media";
  import type { NotesMediaAssetChange } from "$lib/notes/blocks/factory";
  import {
    planNotesKeyboardAction,
    type NotesKeyboardAction,
  } from "$lib/notes/blocks/keyboard";
  import { notesUndoShortcutAction } from "$lib/notes/history/undo-history";
  import type {
    NotesAudioBlock,
    NotesBlockType,
    NotesFileBlock,
    NotesImageBlock,
    NotesPdfBlock,
    NotesVideoBlock,
  } from "$lib/notes/types";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import FileIcon from "@lucide/svelte/icons/file";
  import FileText from "@lucide/svelte/icons/file-text";
  import ImageIcon from "@lucide/svelte/icons/image";
  import Music from "@lucide/svelte/icons/music";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Upload from "@lucide/svelte/icons/upload";
  import Video from "@lucide/svelte/icons/video";

  type NotesMediaBlock =
    | NotesImageBlock
    | NotesVideoBlock
    | NotesAudioBlock
    | NotesFileBlock
    | NotesPdfBlock;

  const nativeFilePickerAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "storage.native-file-picker",
  );

  let {
    block,
    previousBlockType,
    isOnlyBlock,
    focusBlockId,
    focusRequestId,
    onKeyboardAction,
    onUndo,
    onRedo,
    onMediaChange,
  }: {
    block: NotesMediaBlock;
    previousBlockType: NotesBlockType | null;
    isOnlyBlock: boolean;
    focusBlockId: string | null;
    focusRequestId: number;
    onKeyboardAction: (blockId: string, action: NotesKeyboardAction) => void;
    onUndo: () => Promise<void> | void;
    onRedo: () => Promise<void> | void;
    onMediaChange: (
      blockId: string,
      url: string,
      caption: string,
      name?: string,
      assetChange?: NotesMediaAssetChange,
    ) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const locale = $derived(localization.locale);
  let urlInput: HTMLInputElement | null = $state(null);
  let openError = $state<string | null>(null);
  let localFileError = $state<string | null>(null);
  let localPreviewUrl = $state<string | null>(null);
  let localPreviewError = $state<string | null>(null);
  let isPickingLocalFile = $state(false);
  let urlDraft = $state("");
  let captionDraft = $state("");
  let nameDraft = $state("");
  let draftSourceKey = $state("");
  const mediaType = $derived(block.type as NotesMediaBlockType);
  const media = $derived(
    block.type === "image"
      ? block.image
      : block.type === "video"
        ? block.video
        : block.type === "audio"
          ? block.audio
          : block.type === "pdf"
            ? block.pdf
            : block.file,
  );
  const localAsset = $derived(mediaManagedAssetMetadata(media));
  const localAssetPath = $derived(localAsset?.relativePath ?? null);
  const sourceKind = $derived(mediaSourceKind(media));
  const url = $derived(mediaSourceUrl(media));
  const caption = $derived(mediaCaptionPlainText(media));
  const name = $derived(media.name ?? "");
  const displayName = $derived(
    mediaDisplayNameFromSource(urlDraft, mediaType === "file" ? nameDraft : undefined)
      || mediaDisplayName(media)
      || t(`notes.blockType.${mediaType}`),
  );
  const draftIssue = $derived(localAssetPath ? null : mediaUrlIssue(mediaType, urlDraft));
  const canOpen = $derived(!localAssetPath && !draftIssue && canOpenDraftUrl(urlDraft));
  const previewKind = $derived(
    localAssetPath
      ? (localPreviewUrl && mediaType !== "file" ? mediaType : "none")
      : mediaPreviewKindForUrl(mediaType, urlDraft),
  );
  const previewUrl = $derived(localPreviewUrl ?? "");
  const sourceStatusMessage = $derived(mediaSourceStatusMessage(sourceKind));
  const localMetadata = $derived(localAsset ? formatLocalAssetMetadata(localAsset) : null);
  const currentText = $derived(
    [captionDraft, displayName, urlDraft || mediaPlainText(media)]
      .map((part) => part.trim())
      .filter(Boolean)
      .join(" "),
  );
  const MediaIcon = $derived(mediaIcon());

  $effect(() => {
    const sourceKey = `${block.id}\u0000${url}\u0000${caption}\u0000${name}`;
    if (draftSourceKey === sourceKey) return;
    urlDraft = url;
    captionDraft = caption;
    nameDraft = name;
    localFileError = null;
    draftSourceKey = sourceKey;
  });

  $effect(() => {
    const assetPath = localAssetPath;
    const type = mediaType;
    if (!assetPath || type === "file") {
      localPreviewUrl = null;
      localPreviewError = null;
      return;
    }
    let cancelled = false;
    localPreviewUrl = null;
    localPreviewError = null;
    void notesFileAssetUrl(assetPath)
      .then((assetUrl) => {
        if (cancelled) return;
        localPreviewUrl = assetUrl;
      })
      .catch((error) => {
        if (cancelled) return;
        localPreviewError = error instanceof Error ? error.message : String(error);
      });
    return () => {
      cancelled = true;
      releaseNotesFileAssetUrl(assetPath);
    };
  });

  $effect(() => {
    const _focusRequestId = focusRequestId;
    if (focusBlockId !== block.id) return;
    void tick().then(() => {
      urlInput?.focus();
    });
  });

  function mediaIcon() {
    if (mediaType === "image") return ImageIcon;
    if (mediaType === "video") return Video;
    if (mediaType === "audio") return Music;
    if (mediaType === "pdf") return FileText;
    return FileIcon;
  }

  function canOpenDraftUrl(source: string): boolean {
    const trimmed = source.trim();
    if (!trimmed) return false;
    try {
      return new URL(trimmed).protocol === "https:";
    } catch {
      return false;
    }
  }

  function validationMessage(issue: NotesMediaUrlIssue | null): string | null {
    if (!issue) return null;
    if (issue === "invalid_url") return t("notes.mediaUrlInvalid");
    if (issue === "requires_https") return t("notes.mediaUrlRequiresHttps");
    return t("notes.mediaUrlUnsupported", t(`notes.blockType.${mediaType}`));
  }

  function mediaSourceStatusMessage(kind: NotesMediaSourceKind): string | null {
    if (localAssetPath) return localPreviewError ? null : t("notes.mediaStoredOffline");
    const trimmedDraft = urlDraft.trim();
    const draftReplacesSource = Boolean(trimmedDraft) && trimmedDraft !== url.trim();
    if (!draftIssue && trimmedDraft && (kind === "external_reference" || draftReplacesSource)) {
      return t("notes.mediaExternalReference");
    }
    if (kind === "file_upload_reference") return t("notes.mediaImportedUploadReference");
    if (kind === "imported_file_reference") return t("notes.mediaImportedRemoteReference");
    if (!draftIssue && trimmedDraft) return t("notes.mediaExternalReference");
    return null;
  }

  function formatLocalAssetMetadata(asset: NotesFileAssetMetadata): string {
    return t("notes.mediaLocalFileMetadata", asset.contentType, formatFileSize(asset.byteSize));
  }

  function formatFileSize(byteSize: number): string {
    if (byteSize < 1024) return t("notes.mediaBytes", formatNumber(locale, byteSize));
    const kib = byteSize / 1024;
    if (kib < 1024) {
      return t("notes.mediaKiB", formatNumber(locale, kib, { maximumFractionDigits: 1 }));
    }
    return t("notes.mediaMiB", formatNumber(locale, kib / 1024, { maximumFractionDigits: 1 }));
  }

  function commitDrafts(): void {
    if (draftIssue) return;
    openError = null;
    if (localAssetPath) {
      onMediaChange(
        block.id,
        "",
        captionDraft,
        mediaType === "file" ? nameDraft : undefined,
        { type: "preserve" },
      );
      return;
    }
    onMediaChange(
      block.id,
      urlDraft,
      captionDraft,
      mediaType === "file" ? nameDraft : undefined,
    );
  }

  async function chooseLocalFile(): Promise<void> {
    try {
      localFileError = null;
      isPickingLocalFile = true;
      const asset = await pickNotesFileAsset(mediaType);
      if (!asset) return;
      const assetName = mediaType === "file"
        ? (nameDraft.trim() || asset.originalName || undefined)
        : (asset.originalName ?? undefined);
      onMediaChange(block.id, "", captionDraft, assetName, { type: "attach", asset });
    } catch (error) {
      localFileError = error instanceof Error ? error.message : String(error);
    } finally {
      isPickingLocalFile = false;
    }
  }

  function removeLocalFile(): void {
    localFileError = null;
    localPreviewUrl = null;
    localPreviewError = null;
    onMediaChange(
      block.id,
      "",
      captionDraft,
      mediaType === "file" ? nameDraft : undefined,
      { type: "clear" },
    );
  }

  function handleKeydown(event: KeyboardEvent): void {
    const undoAction = notesUndoShortcutAction(event);
    if (undoAction) {
      event.preventDefault();
      void Promise.resolve(undoAction === "undo" ? onUndo() : onRedo());
      return;
    }
    const target = event.currentTarget;
    const input = target instanceof HTMLInputElement ? target : null;
    const action = planNotesKeyboardAction({
      key: event.key,
      shiftKey: event.shiftKey,
      ctrlKey: event.ctrlKey,
      metaKey: event.metaKey,
      altKey: event.altKey,
      text: currentText || (input?.value ?? ""),
      selectionStart: input?.selectionStart ?? 0,
      selectionEnd: input?.selectionEnd ?? 0,
      blockType: block.type,
      previousBlockType,
      isOnlyBlock,
    });
    if (
      action.type === "none"
      || action.type === "insert_newline"
      || action.type === "open_slash_menu"
    ) {
      return;
    }
    if (action.preventDefault) event.preventDefault();
    commitDrafts();
    onKeyboardAction(block.id, action);
  }

  async function openMedia(): Promise<void> {
    if (!canOpen) return;
    try {
      openError = null;
      await openUrl(urlDraft.trim());
    } catch (error) {
      openError = error instanceof Error ? error.message : String(error);
    }
  }
</script>

<section
  class="my-1 flex min-w-0 flex-col gap-2 rounded-md border border-border bg-background/70 p-2"
  aria-label={t(`notes.blockType.${mediaType}`)}
>
  <div class="flex min-w-0 items-start gap-2">
    <div
      class="mt-1 flex size-7 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground"
      aria-hidden="true"
    >
      <MediaIcon class="size-4" />
    </div>
    <div class="flex min-w-0 flex-1 flex-col gap-1">
      {#if mediaType === "file"}
        <input
          class="min-h-7 w-full min-w-0 bg-transparent text-[0.866667rem] font-medium outline-none placeholder:text-muted-foreground"
          type="text"
          aria-label={t("notes.mediaName")}
          value={nameDraft}
          placeholder={t("notes.mediaNamePlaceholder")}
          oninput={(event) => {
            nameDraft = event.currentTarget.value;
            openError = null;
          }}
          onblur={commitDrafts}
          onkeydown={handleKeydown}
        />
      {/if}
      {#if localAssetPath}
        <div class="flex min-h-7 min-w-0 items-center gap-2 text-[0.866667rem] font-medium">
          <span class="min-w-0 truncate">{t("notes.mediaLocalFile")}</span>
        </div>
        {#if localMetadata}
          <p class="min-w-0 text-[0.733333rem] text-muted-foreground">
            {localMetadata}
          </p>
        {/if}
      {:else}
        <input
          bind:this={urlInput}
          class="min-h-7 w-full min-w-0 bg-transparent text-[0.866667rem] font-medium outline-none placeholder:text-muted-foreground"
          type="url"
          inputmode="url"
          aria-label={t("notes.mediaUrl")}
          value={urlDraft}
          placeholder={t("notes.mediaUrlPlaceholder")}
          oninput={(event) => {
            urlDraft = event.currentTarget.value;
            openError = null;
          }}
          onblur={commitDrafts}
          onkeydown={handleKeydown}
        />
      {/if}
      <input
        class="min-h-7 w-full min-w-0 bg-transparent text-[0.8rem] text-muted-foreground outline-none placeholder:text-muted-foreground"
        type="text"
        aria-label={t("notes.mediaCaption")}
        value={captionDraft}
        placeholder={t("notes.mediaCaptionPlaceholder")}
        oninput={(event) => {
          captionDraft = event.currentTarget.value;
          openError = null;
        }}
        onblur={commitDrafts}
        onkeydown={handleKeydown}
      />
      {#if validationMessage(draftIssue)}
        <p class="text-[0.733333rem] text-destructive">
          {validationMessage(draftIssue)}
        </p>
      {:else if localPreviewError}
        <p class="text-[0.733333rem] text-muted-foreground">
          {t("notes.mediaLocalPreviewUnavailable", localPreviewError)}
        </p>
      {/if}
      {#if sourceStatusMessage}
        <p class="text-[0.733333rem] text-muted-foreground">
          {sourceStatusMessage}
        </p>
      {/if}
      {#if localFileError}
        <Notice class="text-[0.733333rem]" message={t("notes.mediaLocalAttachFailed", localFileError)} copy={localFileError} />
      {/if}
      {#if openError}
        <Notice class="text-[0.733333rem]" message={t("notes.openMediaFailed", openError)} copy={openError} />
      {/if}
    </div>
    {#if nativeFilePickerAvailable}
      <button
        type="button"
        class="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-wait disabled:opacity-40"
        aria-label={localAssetPath
          ? t("notes.replaceLocalMedia", t(`notes.blockType.${mediaType}`))
          : t("notes.chooseLocalMedia", t(`notes.blockType.${mediaType}`))}
        disabled={isPickingLocalFile}
        onclick={() => {
          void chooseLocalFile();
        }}
      >
        <Upload class="size-4" />
      </button>
    {/if}
    {#if localAssetPath}
      <button
        type="button"
        class="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
        aria-label={t("notes.removeLocalMedia", t(`notes.blockType.${mediaType}`))}
        onclick={removeLocalFile}
      >
        <Trash2 class="size-4" />
      </button>
    {/if}
    <button
      type="button"
      class="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40"
      aria-label={t("notes.openMedia", urlDraft)}
      disabled={!canOpen}
      onclick={() => {
        void openMedia();
      }}
    >
      <ExternalLink class="size-4" />
    </button>
  </div>

  {#if previewKind !== "none" && previewKind !== "link"}
    {#if previewKind === "image"}
      <img
        class="notes-media-preview notes-media-preview-image"
        src={previewUrl}
        alt={captionDraft || displayName}
        loading="lazy"
      />
    {:else if previewKind === "video"}
      <video class="notes-media-preview" src={previewUrl} controls>
        <track kind="captions" />
        {displayName}
      </video>
    {:else if previewKind === "audio"}
      <audio class="w-full" src={previewUrl} controls>
        {displayName}
      </audio>
    {:else if previewKind === "pdf"}
      <object
        class="notes-media-preview notes-media-preview-pdf"
        data={previewUrl}
        type="application/pdf"
        aria-label={t("notes.mediaPreview")}
      >
        <span class="block p-3 text-[0.8rem] text-muted-foreground">{displayName}</span>
      </object>
    {/if}
  {:else}
    <div class="notes-media-placeholder">
      <span class="min-w-0 truncate">{displayName || t(`notes.blockType.${mediaType}`)}</span>
    </div>
  {/if}
</section>

<style>
  .notes-media-preview {
    width: 100%;
    max-height: 20rem;
    min-width: 0;
    border: 1px solid var(--border);
    border-radius: 0.375rem;
  }

  .notes-media-preview-image {
    object-fit: contain;
  }

  .notes-media-preview-pdf {
    height: min(20rem, 55vh);
  }

  .notes-media-placeholder {
    display: flex;
    min-width: 0;
    align-items: center;
    border-radius: 0.375rem;
    background: hsl(var(--muted) / 0.4);
    padding: 0.5rem 0.75rem;
    color: hsl(var(--muted-foreground));
    font-size: calc(0.866667rem * var(--type-scale));
  }
</style>
