<script lang="ts">
  import { tick } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import BookmarkIcon from "@lucide/svelte/icons/bookmark";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import LinkIcon from "@lucide/svelte/icons/link";
  import Sigma from "@lucide/svelte/icons/sigma";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { bookmarkCaptionPlainText, canOpenBookmarkUrl } from "$lib/notes/block-types/bookmark";
  import { blockPlainText } from "$lib/notes/blocks/factory";
  import {
    planNotesKeyboardAction,
    type NotesKeyboardAction,
  } from "$lib/notes/blocks/keyboard";
  import { notesUndoShortcutAction } from "$lib/notes/history/undo-history";
  import { canOpenEmbedUrl, embedDisplayTitle, embedUrlPlainText } from "$lib/notes/block-types/embed";
  import { equationExpressionPlainText, equationPreviewText } from "$lib/notes/block-types/equation";
  import {
    canOpenLinkPreviewUrl,
    linkPreviewDisplaySource,
    linkPreviewDisplayTitle,
    linkPreviewUrlPlainText,
  } from "$lib/notes/links/preview";
  import type { NotesBlock, NotesBlockType } from "$lib/notes/types";

  let {
    block,
    previousBlockType,
    isOnlyBlock,
    focusBlockId,
    focusRequestId,
    onKeyboardAction,
    onUndo,
    onRedo,
    onBookmarkChange,
    onLinkPreviewUrlChange,
    onEmbedUrlChange,
    onEquationExpressionChange,
  }: {
    block: NotesBlock;
    previousBlockType: NotesBlockType | null;
    isOnlyBlock: boolean;
    focusBlockId: string | null;
    focusRequestId: number;
    onKeyboardAction: (blockId: string, action: NotesKeyboardAction) => void;
    onUndo: () => Promise<void> | void;
    onRedo: () => Promise<void> | void;
    onBookmarkChange: (blockId: string, url: string, caption: string) => void;
    onLinkPreviewUrlChange: (blockId: string, url: string) => void;
    onEmbedUrlChange: (blockId: string, url: string) => void;
    onEquationExpressionChange: (blockId: string, expression: string) => void;
  } = $props();

  const { t } = getLocalization();
  let bookmarkUrlInput: HTMLInputElement | null = $state(null);
  let linkPreviewUrlInput: HTMLInputElement | null = $state(null);
  let embedUrlInput: HTMLInputElement | null = $state(null);
  let equationInput: HTMLInputElement | null = $state(null);
  let bookmarkOpenError = $state<string | null>(null);
  let linkPreviewOpenError = $state<string | null>(null);
  let embedOpenError = $state<string | null>(null);
  const text = $derived(blockPlainText(block));
  const bookmarkCaption = $derived(
    block.type === "bookmark" ? bookmarkCaptionPlainText(block.bookmark) : "",
  );
  const bookmarkCanOpen = $derived(
    block.type === "bookmark" && canOpenBookmarkUrl(block.bookmark.url),
  );
  const linkPreviewUrl = $derived(
    block.type === "link_preview" ? linkPreviewUrlPlainText(block.link_preview) : "",
  );
  const linkPreviewCanOpen = $derived(
    block.type === "link_preview" && canOpenLinkPreviewUrl(block.link_preview.url),
  );
  const linkPreviewTitle = $derived(linkPreviewDisplayTitle(linkPreviewUrl));
  const linkPreviewSource = $derived(linkPreviewDisplaySource(linkPreviewUrl));
  const embedUrl = $derived(block.type === "embed" ? embedUrlPlainText(block.embed) : "");
  const embedCanOpen = $derived(block.type === "embed" && canOpenEmbedUrl(block.embed.url));
  const embedTitle = $derived(embedDisplayTitle(embedUrl));
  const equationExpression = $derived(
    block.type === "equation" ? equationExpressionPlainText(block.equation) : "",
  );
  const equationPreview = $derived(equationPreviewText(equationExpression));

  $effect(() => {
    const _focusRequestId = focusRequestId;
    if (focusBlockId !== block.id) return;
    void tick().then(() => {
      bookmarkUrlInput?.focus();
      linkPreviewUrlInput?.focus();
      embedUrlInput?.focus();
      equationInput?.focus();
    });
  });

  function handleBookmarkInput(url: string, caption: string): void {
    bookmarkOpenError = null;
    onBookmarkChange(block.id, url, caption);
  }

  function handleLinkPreviewInput(url: string): void {
    linkPreviewOpenError = null;
    onLinkPreviewUrlChange(block.id, url);
  }

  function handleEmbedInput(url: string): void {
    embedOpenError = null;
    onEmbedUrlChange(block.id, url);
  }

  function handleInputKeydown(event: KeyboardEvent, actionText: string): void {
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
      text: actionText,
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
    onKeyboardAction(block.id, action);
  }

  async function openBookmark(): Promise<void> {
    if (block.type !== "bookmark" || !bookmarkCanOpen) return;
    try {
      bookmarkOpenError = null;
      await openUrl(block.bookmark.url.trim());
    } catch (error) {
      bookmarkOpenError = error instanceof Error ? error.message : String(error);
    }
  }

  async function openEmbed(): Promise<void> {
    if (block.type !== "embed" || !embedCanOpen) return;
    try {
      embedOpenError = null;
      await openUrl(block.embed.url.trim());
    } catch (error) {
      embedOpenError = error instanceof Error ? error.message : String(error);
    }
  }

  async function openLinkPreview(): Promise<void> {
    if (block.type !== "link_preview" || !linkPreviewCanOpen) return;
    try {
      linkPreviewOpenError = null;
      await openUrl(block.link_preview.url.trim());
    } catch (error) {
      linkPreviewOpenError = error instanceof Error ? error.message : String(error);
    }
  }
</script>

{#if block.type === "bookmark"}
  <section
    class="my-1 flex min-w-0 items-start gap-2 rounded-md border border-border bg-background/70 p-2"
    aria-label={t("notes.blockType.bookmark")}
  >
    <div
      class="mt-1 flex size-7 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground"
      aria-hidden="true"
    >
      <BookmarkIcon class="size-4" />
    </div>
    <div class="flex min-w-0 flex-1 flex-col gap-1">
      <input
        bind:this={bookmarkUrlInput}
        class="min-h-7 w-full min-w-0 bg-transparent text-[0.866667rem] font-medium outline-none placeholder:text-muted-foreground"
        type="url"
        inputmode="url"
        aria-label={t("notes.bookmarkUrl")}
        value={block.bookmark.url}
        placeholder={t("notes.bookmarkUrlPlaceholder")}
        oninput={(event) => {
          handleBookmarkInput(event.currentTarget.value, bookmarkCaption);
        }}
        onkeydown={(event) => handleInputKeydown(event, text || event.currentTarget.value)}
      />
      <input
        class="min-h-7 w-full min-w-0 bg-transparent text-[0.8rem] text-muted-foreground outline-none placeholder:text-muted-foreground"
        type="text"
        aria-label={t("notes.bookmarkCaption")}
        value={bookmarkCaption}
        placeholder={t("notes.bookmarkCaptionPlaceholder")}
        oninput={(event) => {
          handleBookmarkInput(block.bookmark.url, event.currentTarget.value);
        }}
        onkeydown={(event) => handleInputKeydown(event, text || event.currentTarget.value)}
      />
      {#if bookmarkOpenError}
        <Notice class="text-[0.733333rem]" message={t("notes.openBookmarkFailed", bookmarkOpenError)} copy={bookmarkOpenError} />
      {/if}
    </div>
    <button
      type="button"
      class="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40"
      aria-label={t("notes.openBookmark", block.bookmark.url)}
      disabled={!bookmarkCanOpen}
      onclick={() => {
        void openBookmark();
      }}
    >
      <ExternalLink class="size-4" />
    </button>
  </section>
{:else if block.type === "link_preview"}
  <section
    class="my-1 flex min-w-0 items-start gap-2 rounded-md border border-border bg-background/70 p-2"
    aria-label={t("notes.blockType.linkPreview")}
  >
    <div
      class="mt-1 flex size-7 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground"
      aria-hidden="true"
    >
      <LinkIcon class="size-4" />
    </div>
    <div class="flex min-w-0 flex-1 flex-col gap-1.5">
      <input
        bind:this={linkPreviewUrlInput}
        class="min-h-7 w-full min-w-0 bg-transparent text-[0.866667rem] font-medium outline-none placeholder:text-muted-foreground"
        type="url"
        inputmode="url"
        aria-label={t("notes.linkPreviewUrl")}
        value={linkPreviewUrl}
        placeholder={t("notes.linkPreviewUrlPlaceholder")}
        oninput={(event) => {
          handleLinkPreviewInput(event.currentTarget.value);
        }}
        onkeydown={(event) => handleInputKeydown(event, linkPreviewUrl)}
      />
      <output
        class="flex min-h-14 min-w-0 flex-col justify-center gap-0.5 rounded-md border border-border bg-muted/30 px-3 py-2 text-[0.866667rem]"
        aria-label={t("notes.linkPreviewPreview")}
      >
        <span class="min-w-0 truncate font-medium text-foreground">
          {linkPreviewTitle || t("notes.blockType.linkPreview")}
        </span>
        {#if linkPreviewSource}
          <span class="min-w-0 truncate text-[0.8rem] text-muted-foreground">
            {linkPreviewSource}
          </span>
        {/if}
      </output>
      {#if linkPreviewOpenError}
        <Notice class="text-[0.733333rem]" message={t("notes.openLinkPreviewFailed", linkPreviewOpenError)} copy={linkPreviewOpenError} />
      {/if}
    </div>
    <button
      type="button"
      class="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40"
      aria-label={t("notes.openLinkPreview", linkPreviewUrl)}
      disabled={!linkPreviewCanOpen}
      onclick={() => {
        void openLinkPreview();
      }}
    >
      <ExternalLink class="size-4" />
    </button>
  </section>
{:else if block.type === "embed"}
  <section
    class="my-1 flex min-w-0 items-start gap-2 rounded-md border border-border bg-background/70 p-2"
    aria-label={t("notes.blockType.embed")}
  >
    <div
      class="mt-1 flex size-7 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground"
      aria-hidden="true"
    >
      <ExternalLink class="size-4" />
    </div>
    <div class="flex min-w-0 flex-1 flex-col gap-1.5">
      <input
        bind:this={embedUrlInput}
        class="min-h-7 w-full min-w-0 bg-transparent text-[0.866667rem] font-medium outline-none placeholder:text-muted-foreground"
        type="url"
        inputmode="url"
        aria-label={t("notes.embedUrl")}
        value={embedUrl}
        placeholder={t("notes.embedUrlPlaceholder")}
        oninput={(event) => {
          handleEmbedInput(event.currentTarget.value);
        }}
        onkeydown={(event) => handleInputKeydown(event, embedUrl)}
      />
      <output
        class="min-h-10 min-w-0 truncate rounded-md bg-muted/40 px-3 py-2 text-[0.866667rem] text-muted-foreground"
        aria-label={t("notes.embedPreview")}
      >
        {embedTitle || t("notes.blockType.embed")}
      </output>
      {#if embedOpenError}
        <Notice class="text-[0.733333rem]" message={t("notes.openEmbedFailed", embedOpenError)} copy={embedOpenError} />
      {/if}
    </div>
    <button
      type="button"
      class="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40"
      aria-label={t("notes.openEmbed", embedUrl)}
      disabled={!embedCanOpen}
      onclick={() => {
        void openEmbed();
      }}
    >
      <ExternalLink class="size-4" />
    </button>
  </section>
{:else if block.type === "equation"}
  <section
    class="my-1 flex min-w-0 flex-col gap-2 rounded-md border border-border bg-background/70 p-2"
    aria-label={t("notes.blockType.equation")}
  >
    <div class="flex min-w-0 items-center gap-2">
      <div
        class="flex size-7 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground"
        aria-hidden="true"
      >
        <Sigma class="size-4" />
      </div>
      <input
        bind:this={equationInput}
        class="min-h-8 w-full min-w-0 bg-transparent font-mono text-[0.866667rem] outline-none placeholder:text-muted-foreground"
        type="text"
        inputmode="text"
        aria-label={t("notes.equationExpression")}
        value={equationExpression}
        placeholder={t("notes.equationPlaceholder")}
        spellcheck={false}
        oninput={(event) => {
          onEquationExpressionChange(block.id, event.currentTarget.value);
        }}
        onkeydown={(event) => handleInputKeydown(event, equationExpression)}
      />
    </div>
    <output
      class="min-h-10 min-w-0 overflow-x-auto rounded-md bg-muted/40 px-3 py-2 text-center font-serif text-[1.066667rem] text-foreground"
      aria-label={t("notes.equationPreview")}
    >
      {equationPreview}
    </output>
  </section>
{/if}
