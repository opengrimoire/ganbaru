<script lang="ts">
  import { notesFileAssetUrl, releaseNotesFileAssetUrl } from "$lib/api/notes/file-assets";
  import { openNotesExternalUrl } from "$lib/api/notes/knowledge";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesCommentAttachmentFile } from "$lib/notes/collaboration/comment-attachments";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import FileText from "@lucide/svelte/icons/file-text";
  import X from "@lucide/svelte/icons/x";

  /** One comment attachment: an image preview for managed images, otherwise a named file chip. */
  let {
    attachment,
    variant = "full",
    onRemove,
  }: {
    attachment: Record<string, unknown>;
    /** Compact keeps image previews to a thumbnail, as in a composer draft. */
    variant?: "compact" | "full";
    onRemove?: () => void;
  } = $props();

  const { t } = getLocalization();
  const file = $derived(notesCommentAttachmentFile(attachment));
  const name = $derived(file?.name || t("notes.commentAttachmentUnavailable"));
  const imagePath = $derived(file?.isImage ? file.assetPath : null);

  let previewUrl = $state<string | null>(null);
  let previewFailed = $state(false);
  let openError = $state<string | null>(null);

  $effect(() => {
    const path = imagePath;
    previewUrl = null;
    previewFailed = false;
    if (!path) return;
    let cancelled = false;
    void notesFileAssetUrl(path)
      .then((url) => { if (!cancelled) previewUrl = url; })
      .catch((error: unknown) => {
        if (cancelled) return;
        previewFailed = true;
        console.warn("notes comment attachment preview failed", error);
      });
    return () => {
      cancelled = true;
      releaseNotesFileAssetUrl(path);
    };
  });

  async function openExternal(url: string): Promise<void> {
    openError = null;
    try {
      await openNotesExternalUrl(url);
    } catch (error) {
      openError = error instanceof Error ? error.message : String(error);
    }
  }
</script>

<div class={["comment-attachment relative min-w-0", variant === "full" && imagePath && !previewFailed ? "max-w-full" : "max-w-64"]}>
  {#if imagePath && !previewFailed}
    {#if previewUrl}
      <img
        src={previewUrl}
        alt={name}
        class={[
          "block rounded-md border border-border bg-muted object-cover",
          variant === "compact" ? "size-14" : "max-h-72 max-w-full object-contain",
        ]}
      />
    {:else}
      <div class={["rounded-md border border-border bg-muted", variant === "compact" ? "size-14" : "h-32 w-48"]} role="img" aria-label={name}></div>
    {/if}
  {:else if file?.externalUrl}
    {@const url = file.externalUrl}
    <button
      type="button"
      class="flex min-h-8 min-w-0 max-w-full items-center gap-1.5 rounded-md border border-border px-2 py-1 text-left text-panel hover:bg-accent"
      title={url}
      onclick={() => void openExternal(url)}
    >
      <FileText class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
      <span class="min-w-0 truncate">{name}</span>
      <ExternalLink class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
    </button>
  {:else}
    <div class="flex min-h-8 min-w-0 max-w-full items-center gap-1.5 rounded-md border border-border px-2 py-1 text-panel" title={name}>
      <FileText class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
      <span class="min-w-0 truncate">{name}</span>
    </div>
  {/if}
  {#if onRemove}
    <button
      type="button"
      class="remove-attachment absolute -right-1.5 -top-1.5 flex size-5 items-center justify-center rounded-full border border-border bg-popover text-muted-foreground hover:text-foreground"
      aria-label={t("notes.commentRemoveAttachment", name)}
      title={t("notes.commentRemoveAttachment", name)}
      onclick={onRemove}
    >
      <X class="size-3" aria-hidden="true" />
    </button>
  {/if}
  {#if openError}
    <Notice class="mt-1 text-panel-detail" message={openError} />
  {/if}
</div>

<style>
  @media (hover: hover) {
    .remove-attachment { opacity: 0; transition: opacity 120ms ease; }
    .comment-attachment:is(:hover, :focus-within) .remove-attachment { opacity: 1; }
  }
</style>
