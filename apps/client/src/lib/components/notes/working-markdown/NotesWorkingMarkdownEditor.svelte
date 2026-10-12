<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
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
    isWorkingMarkdownDirty,
    workingMarkdownRefreshDecision,
  } from "$lib/notes/working-markdown/editor";
  import { cn } from "$lib/utils";
  import Notice from "$lib/components/ui/Notice.svelte";

  let {
    file,
    onDirtyChange,
  }: {
    file: NotesWorkingMarkdownFileRef;
    onDirtyChange: (dirty: boolean) => void;
  } = $props();

  const { t } = getLocalization();
  let loadedFile = $state<NotesWorkingMarkdownFileRead | null>(null);
  let remoteConflict = $state<NotesWorkingMarkdownFileRead | null>(null);
  let draft = $state("");
  let loading = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let mode = $state<"edit" | "preview">("edit");
  let loadGeneration = 0;
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
    loadGeneration += 1;
    onDirtyChange(false);
  });

  async function loadFile(workingFolderId: string, relativePath: string): Promise<void> {
    const generation = ++loadGeneration;
    loading = true;
    error = null;
    remoteConflict = null;
    try {
      const result = await readNotesWorkingMarkdown(workingFolderId, relativePath);
      if (generation !== loadGeneration) return;
      loadedFile = result;
      draft = result.content;
    } catch (reason) {
      if (generation !== loadGeneration) return;
      loadedFile = null;
      draft = "";
      error = errorMessage(reason);
    } finally {
      if (generation === loadGeneration) loading = false;
    }
  }

  async function save(): Promise<void> {
    if (!loadedFile || !dirty || saving) return;
    saving = true;
    error = null;
    remoteConflict = null;
    try {
      const result = await saveNotesWorkingMarkdown(
        file.workingFolderId,
        file.relativePath,
        draft,
        loadedFile.revision,
      );
      loadedFile = result;
      draft = result.content;
    } catch (reason) {
      error = errorMessage(reason);
      await detectConflict();
    } finally {
      saving = false;
    }
  }

  async function refresh(): Promise<void> {
    if (!loadedFile) {
      await loadFile(file.workingFolderId, file.relativePath);
      return;
    }
    error = null;
    try {
      const remote = await readNotesWorkingMarkdown(file.workingFolderId, file.relativePath);
      const decision = workingMarkdownRefreshDecision(
        loadedFile.content,
        loadedFile.revision,
        draft,
        remote.revision,
      );
      if (decision === "conflict") {
        remoteConflict = remote;
        return;
      }
      if (decision === "preserve_local") return;
      loadedFile = remote;
      draft = remote.content;
      remoteConflict = null;
    } catch (reason) {
      error = errorMessage(reason);
    }
  }

  async function detectConflict(): Promise<void> {
    if (!loadedFile) return;
    try {
      const remote = await readNotesWorkingMarkdown(file.workingFolderId, file.relativePath);
      if (remote.revision !== loadedFile.revision) remoteConflict = remote;
    } catch {
      // Preserve the original save error when the recovery read also fails.
    }
  }

  function reloadRemote(): void {
    if (!remoteConflict) return;
    loadedFile = remoteConflict;
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
    void openUrl(target.href).catch((reason) => {
      error = errorMessage(reason);
    });
  }

  function previewLinks(node: HTMLElement): { destroy: () => void } {
    node.addEventListener("click", handlePreviewClick);
    return {
      destroy: () => node.removeEventListener("click", handlePreviewClick),
    };
  }

  function errorMessage(reason: unknown): string {
    return reason instanceof Error ? reason.message : String(reason);
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
    <Notice class="shrink-0 border-b border-border px-3 py-2 text-sm" message={error} />
  {/if}

  {#if loading && !loadedFile}
    <div class="flex flex-1 items-center justify-center text-sm text-muted-foreground">{t("common.loading")}</div>
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
