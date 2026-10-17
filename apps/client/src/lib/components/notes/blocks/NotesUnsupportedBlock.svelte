<script lang="ts">
  import { tick } from "svelte";
  import Code2 from "@lucide/svelte/icons/code-2";
  import FileText from "@lucide/svelte/icons/file-text";
  import CircleHelp from "@lucide/svelte/icons/circle-help";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    unsupportedBlockHasRawPayload,
    unsupportedBlockTypeName,
    unsupportedBlockWarnings,
    type NotesUnsupportedConversionTarget,
  } from "$lib/notes/block-types/unsupported";
  import type { NotesUnsupportedBlock } from "$lib/notes/types";

  let {
    block,
    focusBlockId,
    focusRequestId,
    onSurfaceKeydown,
    onFocusBlock,
    onConvertUnsupported,
  }: {
    block: NotesUnsupportedBlock;
    focusBlockId: string | null;
    focusRequestId: number;
    onSurfaceKeydown: (event: KeyboardEvent) => void;
    onFocusBlock: (blockId: string) => void;
    onConvertUnsupported: (
      blockId: string,
      target: NotesUnsupportedConversionTarget,
    ) => Promise<void> | void;
  } = $props();

  const { t } = getLocalization();
  let focusButton: HTMLButtonElement | null = $state(null);
  const importedType = $derived(unsupportedBlockTypeName(block.unsupported));
  const sourceType = $derived(
    typeof block.unsupported.source_type === "string" ? block.unsupported.source_type.trim() : "",
  );
  const warnings = $derived(unsupportedBlockWarnings(block.unsupported));
  const hasRawPayload = $derived(unsupportedBlockHasRawPayload(block.unsupported));
  const conversionWarningId = $derived(`notes-unsupported-conversion-${block.id}`);

  $effect(() => {
    const _focusRequestId = focusRequestId;
    if (focusBlockId !== block.id) return;
    void tick().then(() => {
      focusButton?.focus();
    });
  });

  function convert(target: NotesUnsupportedConversionTarget): void {
    void Promise.resolve(onConvertUnsupported(block.id, target));
  }
</script>

<section
  class="my-1 flex min-w-0 items-start gap-2 rounded-md border border-dashed border-border bg-muted/30 p-2"
  aria-label={t("notes.blockType.unsupported")}
>
  <div
    class="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground"
    aria-hidden="true"
  >
    <CircleHelp class="size-4" />
  </div>
  <div class="flex min-w-0 flex-1 flex-col gap-2">
    <button
      bind:this={focusButton}
      type="button"
      class="flex min-h-8 min-w-0 flex-col gap-0.5 rounded-sm text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
      onkeydown={onSurfaceKeydown}
      onclick={() => onFocusBlock(block.id)}
    >
      <span class="text-[0.866667rem] font-medium text-foreground">
        {t("notes.unsupportedBlockTitle")}
      </span>
      <span class="min-w-0 truncate text-[0.8rem] text-muted-foreground">
        {#if importedType}
          {t("notes.unsupportedBlockType", importedType)}
        {:else}
          {t("notes.unsupportedBlockUnknownType")}
        {/if}
      </span>
      {#if sourceType && sourceType !== importedType}
        <span class="min-w-0 truncate text-[0.733333rem] text-muted-foreground">
          {t("notes.unsupportedBlockSourceType", sourceType)}
        </span>
      {/if}
      <span class="min-w-0 truncate text-[0.733333rem] text-muted-foreground">
        {#if hasRawPayload}
          {t("notes.unsupportedBlockPayloadPreserved")}
        {:else}
          {t("notes.unsupportedBlockPayloadMissing")}
        {/if}
      </span>
    </button>

    <div class="grid gap-1 text-[0.733333rem] text-muted-foreground">
      <span class="font-medium text-foreground">{t("notes.unsupportedBlockWarnings")}</span>
      {#if warnings.length > 0}
        <ul class="grid gap-0.5">
          {#each warnings as warning}
            <li class="min-w-0 wrap-break-word">{warning}</li>
          {/each}
        </ul>
      {:else}
        <span>{t("notes.unsupportedBlockNoWarnings")}</span>
      {/if}
    </div>

    <div class="flex min-w-0 flex-wrap items-center gap-1.5 text-[0.733333rem] text-muted-foreground">
      <span id={conversionWarningId} class="min-w-full">
        {t("notes.unsupportedBlockConversionWarning")}
      </span>
      <button
        type="button"
        class="inline-flex min-h-7 items-center gap-1.5 rounded border border-border bg-background px-2 font-medium text-foreground hover:bg-accent"
        aria-describedby={conversionWarningId}
        onclick={() => convert("paragraph")}
      >
        <FileText class="size-3.5" aria-hidden="true" />
        <span>{t("notes.convertUnsupportedToParagraph")}</span>
      </button>
      <button
        type="button"
        class="inline-flex min-h-7 items-center gap-1.5 rounded border border-border bg-background px-2 font-medium text-foreground hover:bg-accent"
        aria-describedby={conversionWarningId}
        onclick={() => convert("code")}
      >
        <Code2 class="size-3.5" aria-hidden="true" />
        <span>{t("notes.convertUnsupportedToCode")}</span>
      </button>
    </div>
  </div>
</section>
