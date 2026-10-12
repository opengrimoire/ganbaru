<script lang="ts">
  import Copy from "@lucide/svelte/icons/copy";
  import Plus from "@lucide/svelte/icons/plus";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { NotesTemplateBlockStatus } from "$lib/notes/block-types/template";

  let {
    blockId,
    title,
    status,
    onUseTemplate,
    onAddTemplateChild,
  }: {
    blockId: string;
    title: string;
    status: NotesTemplateBlockStatus;
    onUseTemplate: (blockId: string) => void;
    onAddTemplateChild: (blockId: string) => void;
  } = $props();

  const { t } = getLocalization();
  const statusId = $derived(`notes-template-status-${blockId}`);
  const statusText = $derived(templateStatusText());

  function templateStatusText(): string {
    switch (status.useUnavailableReason) {
      case "empty":
        return t("notes.templateEmpty");
      case "child_page":
        return t("notes.templateChildPageUnsupported");
      case null:
        return t("notes.templateChildCount", status.childCount);
    }
  }
</script>

<div
  class="mb-1 flex min-w-0 flex-wrap items-center justify-between gap-2 rounded-md border border-border bg-background/70 px-2 py-1.5 text-[0.8rem]"
>
  <div class="min-w-0">
    <span class="block min-w-0 truncate font-medium text-foreground">
      {t("notes.templateReusableContent")}
    </span>
    <span id={statusId} class="block min-w-0 text-muted-foreground">
      {statusText}
    </span>
  </div>
  <div class="flex min-w-0 flex-wrap items-center gap-1.5">
    <button
      type="button"
      class="inline-flex min-h-7 items-center gap-1.5 rounded border border-border bg-background px-2 font-medium text-foreground hover:bg-accent"
      onclick={() => onAddTemplateChild(blockId)}
    >
      <Plus class="size-3.5" aria-hidden="true" />
      <span>{t("notes.addTemplateContent")}</span>
    </button>
    <button
      type="button"
      class="inline-flex min-h-7 items-center gap-1.5 rounded border border-border bg-background px-2 font-medium text-foreground hover:bg-accent disabled:cursor-not-allowed disabled:opacity-45"
      aria-label={t("notes.useTemplate", title)}
      aria-describedby={statusId}
      disabled={!status.canUse}
      onclick={() => onUseTemplate(blockId)}
    >
      <Copy class="size-3.5" aria-hidden="true" />
      <span>{t("notes.useTemplateButton")}</span>
    </button>
  </div>
</div>
