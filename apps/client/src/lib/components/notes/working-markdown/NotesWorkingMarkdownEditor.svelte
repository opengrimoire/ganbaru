<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { openNotesExternalUrl } from "$lib/api/notes/knowledge";
  import Copy from "@lucide/svelte/icons/copy";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Eye from "@lucide/svelte/icons/eye";
  import FilePenLine from "@lucide/svelte/icons/file-pen-line";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Save from "@lucide/svelte/icons/save";
  import {
    openNotesWorkingMarkdown,
    readNotesWorkingMarkdown,
    saveNotesWorkingMarkdown,
  } from "$lib/api/notes";
  import { renderChatMarkdown } from "$lib/chat/timeline/markdown";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type {
    NotesWorkingMarkdownFileRead,
    NotesWorkingMarkdownFileRef,
  } from "$lib/notes/types";
  import {
    isCurrentWorkingMarkdownRevision,
    isWorkingMarkdownDirty,
    replaceWorkingMarkdownRevision,
    startWorkingMarkdownRead,
    workingMarkdownRefreshDecision,
  } from "$lib/notes/working-markdown/editor";
  import {
    idleAsyncResource,
    isAsyncResourceLoading,
    isCurrentAsyncRequest,
    resolveAsyncRequest,
    type AsyncResource,
  } from "$lib/utils/async-resource";
  import { cn } from "$lib/utils";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { notesFailure, type NotesFailure } from "$lib/notes/failure";

  let {
    file,
    onDirtyChange,
  }: {
    file: NotesWorkingMarkdownFileRef;
    onDirtyChange: (dirty: boolean) => void;
  } = $props();

  const { t } = getLocalization();
  /** The loaded revision of the shown file; every write and refresh applies only while its base is still loaded. */
  let fileDocument = $state.raw<AsyncResource<NotesWorkingMarkdownFileRead | null>>(idleAsyncResource(null));
  /** The revision a save in flight started from. */
  let saveBase = $state.raw<NotesWorkingMarkdownFileRead | null>(null);
  let remoteConflict = $state<NotesWorkingMarkdownFileRead | null>(null);
  let draft = $state("");
  let error = $state<NotesFailure | null>(null);
  let mode = $state<"edit" | "preview">("edit");
  const loadedFile = $derived(fileDocument.data);
  const loading = $derived(isAsyncResourceLoading(fileDocument));
  const saving = $derived(saveBase !== null && saveBase === loadedFile);
  const dirty = $derived(isWorkingMarkdownDirty(loadedFile?.content ?? null, draft));
  const renderedHtml = $derived(renderChatMarkdown(draft));

  $effect(() => {
    onDirtyChange(dirty);
  });

  $effect(() => {
    const workingFolderId = file.workingFolderId;
    const relativePath = file.relativePath;
    void loadFile(workingFolderId, relativePath);
  });

  onMount(() => {
    const protectNavigation = (event: BeforeUnloadEvent) => {
      if (!dirty) return;
      event.preventDefault();
    };
    window.addEventListener("beforeunload", protectNavigation);
    return () => window.removeEventListener("beforeunload", protectNavigation);
  });

  onDestroy(() => {
    fileDocument = idleAsyncResource(null);
    onDirtyChange(false);
  });

  async function loadFile(workingFolderId: string, relativePath: string): Promise<void> {
    const started = startWorkingMarkdownRead<NotesWorkingMarkdownFileRead>();
    fileDocument = started.resource;
    draft = "";
    error = null;
    remoteConflict = null;
    try {
      const result = await readNotesWorkingMarkdown(workingFolderId, relativePath);
      if (!isCurrentAsyncRequest(fileDocument, started.token)) return;
      fileDocument = resolveAsyncRequest(fileDocument, started.token, result);
      draft = result.content;
    } catch (reason) {
      if (!isCurrentAsyncRequest(fileDocument, started.token)) return;
      fileDocument = resolveAsyncRequest(fileDocument, started.token, null);
      error = notesFailure(reason, t("notes.workingMarkdown.loadFailed"));
    }
  }

  async function save(): Promise<void> {
    const base = loadedFile;
    if (!base || loading || !dirty || saving) return;
    const { workingFolderId, relativePath } = file;
    saveBase = base;
    error = null;
    remoteConflict = null;
    try {
      const result = await saveNotesWorkingMarkdown(workingFolderId, relativePath, draft, base.revision);
      const next = replaceWorkingMarkdownRevision(fileDocument, base, result);
      if (!next) return;
      fileDocument = next;
      draft = result.content;
    } catch (reason) {
      if (!isCurrentWorkingMarkdownRevision(fileDocument, base)) return;
      error = notesFailure(reason, t("notes.workingMarkdown.saveFailed"));
      await detectConflict(base, workingFolderId, relativePath);
    } finally {
      if (saveBase === base) saveBase = null;
    }
  }

  async function refresh(): Promise<void> {
    const base = loadedFile;
    if (!base) {
      await loadFile(file.workingFolderId, file.relativePath);
      return;
    }
    error = null;
    try {
      const remote = await readNotesWorkingMarkdown(file.workingFolderId, file.relativePath);
      if (!isCurrentWorkingMarkdownRevision(fileDocument, base)) return;
      const decision = workingMarkdownRefreshDecision(
        base.content,
        base.revision,
        draft,
        remote.revision,
      );
      if (decision === "conflict") {
        remoteConflict = remote;
        return;
      }
      if (decision === "preserve_local") return;
      fileDocument = idleAsyncResource(remote);
      draft = remote.content;
      remoteConflict = null;
    } catch (reason) {
      if (!isCurrentWorkingMarkdownRevision(fileDocument, base)) return;
      error = notesFailure(reason, t("notes.workingMarkdown.loadFailed"));
    }
  }

  async function detectConflict(
    base: NotesWorkingMarkdownFileRead,
    workingFolderId: string,
    relativePath: string,
  ): Promise<void> {
    try {
      const remote = await readNotesWorkingMarkdown(workingFolderId, relativePath);
      if (!isCurrentWorkingMarkdownRevision(fileDocument, base)) return;
      if (remote.revision !== base.revision) remoteConflict = remote;
    } catch {
      // Preserve the original save error when the recovery read also fails.
    }
  }

  function reloadRemote(): void {
    if (!remoteConflict) return;
    fileDocument = idleAsyncResource(remoteConflict);
    draft = remoteConflict.content;
    remoteConflict = null;
    error = null;
  }

  function handlePreviewClick(event: MouseEvent): void {
    const target = event.target instanceof Element
      ? event.target.closest<HTMLAnchorElement>("a[data-chat-external-link]")
      : null;
    if (!target) return;
    event.preventDefault();
    void openNotesExternalUrl(target.href).catch((reason) => {
      error = notesFailure(reason, t("notes.linkOpenFailed"));
    });
  }

  function previewLinks(node: HTMLElement): { destroy: () => void } {
    node.addEventListener("click", handlePreviewClick);
    return {
      destroy: () => node.removeEventListener("click", handlePreviewClick),
    };
  }
</script>

<section class="flex min-w-0 flex-1 flex-col bg-background" data-notes-working-markdown-editor>
  <header class="flex min-h-11 shrink-0 flex-wrap items-center gap-2 border-b border-border px-3 py-2">
    <div class="min-w-0 flex-1">
      <div class="flex min-w-0 items-center gap-2">
        <FilePenLine class="size-4 shrink-0 text-muted-foreground" />
        <strong class="truncate text-sm">{file.relativePath.split("/").at(-1)}</strong>
        {#if dirty}<span class="size-2 shrink-0 rounded-full bg-primary" title={t("notes.workingMarkdown.unsaved")}></span>{/if}
      </div>
      <p class="truncate pl-6 text-[0.7rem] text-muted-foreground">{file.workingFolderName} / {file.relativePath}</p>
    </div>
    <div class="flex shrink-0 items-center gap-1">
      <button type="button" class={cn("notes-markdown-action", mode === "edit" && "bg-accent text-foreground")} onclick={() => { mode = "edit"; }}>
        <FilePenLine class="size-3.5" />{t("notes.workingMarkdown.edit")}
      </button>
      <button type="button" class={cn("notes-markdown-action", mode === "preview" && "bg-accent text-foreground")} onclick={() => { mode = "preview"; }}>
        <Eye class="size-3.5" />{t("notes.workingMarkdown.preview")}
      </button>
      <button type="button" class="notes-markdown-action" disabled={loading} onclick={() => { void refresh(); }}>
        <RefreshCw class={cn("size-3.5", loading && "animate-spin")} />{t("notes.workingMarkdown.refreshFile")}
      </button>
      <button type="button" class="notes-markdown-action" onclick={() => { void openNotesWorkingMarkdown(file.workingFolderId, file.relativePath); }}>
        <ExternalLink class="size-3.5" />{t("notes.workingMarkdown.openExternally")}
      </button>
      <button type="button" class="notes-markdown-action" onclick={() => { void navigator.clipboard.writeText(file.relativePath); }}>
        <Copy class="size-3.5" />{t("notes.workingMarkdown.copyPath")}
      </button>
      <button type="button" class="notes-markdown-save" disabled={!dirty || saving || loading} onclick={() => { void save(); }}>
        <Save class="size-3.5" />{saving ? t("notes.workingMarkdown.saving") : t("common.save")}
      </button>
    </div>
  </header>

  {#if remoteConflict}
    <div class="shrink-0 border-b border-warning/40 bg-warning/10 px-3 py-2 text-sm" role="alert">
      <p>{t("notes.workingMarkdown.conflict")}</p>
      <div class="mt-2 flex flex-wrap gap-2">
        <button type="button" class="notes-markdown-action border border-border" onclick={reloadRemote}>{t("notes.workingMarkdown.reloadRemote")}</button>
        <button type="button" class="notes-markdown-action border border-border" onclick={() => { void navigator.clipboard.writeText(draft); }}>{t("notes.workingMarkdown.copyLocal")}</button>
        <button type="button" class="notes-markdown-action border border-border" onclick={() => { void openNotesWorkingMarkdown(file.workingFolderId, file.relativePath); }}>{t("notes.workingMarkdown.openExternally")}</button>
      </div>
    </div>
  {:else if error}
    <Notice class="shrink-0 border-b border-border px-3 py-2 text-sm" message={error.message} copy={error.detail} />
  {/if}

  {#if loading && !loadedFile}
    <Loading layout="fill" label={file.relativePath} class="flex-1" />
  {:else if mode === "edit"}
    <textarea
      class="min-h-0 flex-1 resize-none bg-background p-4 font-mono text-sm leading-6 text-foreground outline-none"
      bind:value={draft}
      spellcheck="false"
      aria-label={t("notes.workingMarkdown.rawEditor")}
    ></textarea>
  {:else}
    <article
      class="chat-markdown min-h-0 flex-1 overflow-auto p-5 text-sm leading-6"
      use:previewLinks
    >{@html renderedHtml}</article>
  {/if}
</section>

<style>
  .notes-markdown-action,
  .notes-markdown-save {
    display: inline-flex;
    min-height: 1.875rem;
    align-items: center;
    gap: 0.25rem;
    border-radius: 0.375rem;
    padding-inline: 0.5rem;
    font-size: calc(0.75rem * var(--type-scale));
    color: var(--muted-foreground);
  }

  .notes-markdown-action:hover:not(:disabled) {
    background: var(--accent);
    color: var(--foreground);
  }

  .notes-markdown-save {
    background: var(--primary);
    color: var(--primary-foreground);
  }

  button:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }

  @container notes-view (max-width: 46rem) {
    .notes-markdown-action {
      width: 1.875rem;
      justify-content: center;
      overflow: hidden;
      padding-inline: 0;
      white-space: nowrap;
      color: transparent;
    }

    .notes-markdown-action :global(svg) {
      color: var(--muted-foreground);
    }
  }
</style>
