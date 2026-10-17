<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import CheckSquare from "@lucide/svelte/icons/square-check-big";
  import Square from "@lucide/svelte/icons/square";
  import { notesBlockMarker, notesHeadingTextClass } from "$lib/notes/blocks/editor-ui";
  import { parseNotesRichTextArray } from "$lib/notes/blocks/validation";
  import type { NotesRichText } from "$lib/notes/types";
  import NotesRichTextInline from "$lib/components/notes/rich-text/NotesRichTextInline.svelte";

  let {
    block,
    depth = 0,
    listOrdinal = 1,
  }: {
    block: Record<string, unknown>;
    depth?: number;
    listOrdinal?: number;
  } = $props();

  const localization = getLocalization();
  const type = $derived(typeof block.type === "string" ? block.type : "paragraph");
  const content = $derived(blockContent(block, type));
  const richText = $derived(parseRichText(content));
  const plainText = $derived(typeof block.plain_text === "string" ? block.plain_text : "");
  const checked = $derived(content.checked === true);

  function blockContent(
    row: Record<string, unknown>,
    blockType: string,
  ): Record<string, unknown> {
    const payload = typeof row.payload === "object"
      && row.payload !== null
      && !Array.isArray(row.payload)
      ? row.payload as Record<string, unknown>
      : row;
    const nested = payload[blockType];
    return typeof nested === "object" && nested !== null && !Array.isArray(nested)
      ? nested as Record<string, unknown>
      : payload;
  }

  function parseRichText(value: Record<string, unknown>): NotesRichText[] {
    try {
      return parseNotesRichTextArray(value.rich_text ?? [], "historical block rich text");
    } catch {
      return [];
    }
  }

  function contentClass(blockType: string): string {
    const headingClass = notesHeadingTextClass(blockType);
    if (headingClass) return `mt-3 ${headingClass}`;
    switch (blockType) {
      case "quote":
        return "border-l-2 border-foreground/40 pl-3 italic";
      case "code":
        return "rounded-md bg-muted px-3 py-2 font-mono text-[0.8rem] whitespace-pre-wrap";
      default:
        return "text-[0.933333rem] leading-6";
    }
  }
</script>

{#if plainText || richText.length > 0 || type === "divider"}
  <div
    class={`min-w-0 ${contentClass(type)}`}
    style={`margin-left: ${depth * 1.25}rem`}
  >
    {#if type === "divider"}
      <div class="my-2 h-px bg-border"></div>
    {:else}
      <div class="flex min-w-0 items-start gap-2">
        {#if type === "bulleted_list_item"}
          <span class="shrink-0">•</span>
        {:else if type === "numbered_list_item"}
          <span class="shrink-0">{notesBlockMarker("numbered_list_item", listOrdinal, localization.locale)}</span>
        {:else if type === "to_do"}
          {#if checked}
            <CheckSquare class="mt-1 size-4 shrink-0 text-muted-foreground" />
          {:else}
            <Square class="mt-1 size-4 shrink-0 text-muted-foreground" />
          {/if}
        {/if}
        <div class:line-through={type === "to_do" && checked} class="min-w-0 wrap-break-word">
          {#if richText.length > 0}
            <NotesRichTextInline {richText} />
          {:else}
            {plainText}
          {/if}
        </div>
      </div>
    {/if}
  </div>
{/if}
