<script lang="ts">
  import { notesRichTextColorStyle } from "$lib/notes/blocks/color";
  import type { NotesResolvedCommentAnchor } from "$lib/notes/collaboration/comments";
  import { equationPreviewText } from "$lib/notes/block-types/equation";
  import type { NotesResolvedSuggestionAnchor } from "$lib/notes/collaboration/suggestions";
  import type { NotesRichText } from "$lib/notes/types";
  import { normalizeNotesTextLinkUrl, openNotesTextLink } from "$lib/notes/links/navigation";
  import NotesLocalReference, { isNotesLocalReference } from "./NotesLocalReference.svelte";
  import NotesObjectMention from "./NotesObjectMention.svelte";
  import { isNotesExternalMentionRichText } from "$lib/notes/rich-text/core";

  let {
    richText,
    commentAnchors = [],
    suggestionAnchors = [],
  }: {
    richText: readonly NotesRichText[];
    commentAnchors?: readonly NotesResolvedCommentAnchor[];
    suggestionAnchors?: readonly NotesResolvedSuggestionAnchor[];
  } = $props();

  const visibleRichText = $derived(richText.filter(richTextItemIsVisible));
  const visibleRuns = $derived(visibleRichTextRuns(visibleRichText));
  const visibleLines = $derived(visibleRichTextLines(visibleRuns));

  interface VisibleRichTextRun {
    item: NotesRichText;
    text: string;
    start: number;
    end: number;
  }

  interface VisibleRichTextSegment {
    text: string;
    start: number;
    end: number;
  }

  interface VisibleRichTextLine {
    parts: VisibleRichTextLinePart[];
  }

  interface VisibleRichTextLinePart {
    item: NotesRichText;
    text: string;
    start: number;
    end: number;
  }

  function richTextItemIsVisible(item: NotesRichText): boolean {
    if (item.type === "equation") return equationPreviewText(item.equation.expression).length > 0;
    return item.plain_text.length > 0;
  }

  function textClass(item: NotesRichText): string {
    const classes = ["notes-rich-text-segment"];
    if (item.annotations.bold) classes.push("font-semibold");
    if (item.annotations.italic) classes.push("italic");
    if (item.annotations.underline) classes.push("underline");
    if (item.annotations.strikethrough) classes.push("line-through");
    if (item.annotations.code) {
      classes.push("rounded bg-muted/70 px-1 py-0.5 font-mono text-[0.9em]");
    }
    if (item.type === "text" && (item.href || item.text.link)) {
      classes.push("cursor-pointer underline decoration-muted-foreground/50 underline-offset-4");
    }
    return classes.join(" ");
  }

  function visibleText(item: NotesRichText): string {
    return item.type === "equation" ? equationPreviewText(item.equation.expression) : item.plain_text;
  }

  function visibleRichTextRuns(items: readonly NotesRichText[]): VisibleRichTextRun[] {
    let cursor = 0;
    return items.map((item) => {
      const text = visibleText(item);
      const start = cursor;
      cursor += text.length;
      return { item, text, start, end: cursor };
    });
  }

  function splitRunByAnchors(run: VisibleRichTextRun): VisibleRichTextSegment[] {
    const boundaries = new Set<number>([run.start, run.end]);
    for (const anchor of commentAnchors) {
      const start = Math.max(run.start, anchor.start);
      const end = Math.min(run.end, anchor.end);
      if (start < end) {
        boundaries.add(start);
        boundaries.add(end);
      }
    }
    for (const anchor of suggestionAnchors) {
      const start = Math.max(run.start, anchor.start);
      const end = Math.min(run.end, anchor.end);
      if (start < end) {
        boundaries.add(start);
        boundaries.add(end);
      }
    }
    const sorted = [...boundaries].sort((left, right) => left - right);
    const segments: VisibleRichTextSegment[] = [];
    for (let index = 0; index < sorted.length - 1; index += 1) {
      const start = sorted[index];
      const end = sorted[index + 1];
      if (start === end) continue;
      segments.push({
        start,
        end,
        text: run.text.slice(start - run.start, end - run.start),
      });
    }
    return segments;
  }

  function anchorsForRange(start: number, end: number): NotesResolvedCommentAnchor[] {
    return commentAnchors.filter((anchor) => anchor.start < end && anchor.end > start);
  }

  function anchorIdsForRange(start: number, end: number): string | undefined {
    const ids = anchorsForRange(start, end).map((anchor) => anchor.threadId);
    return ids.length > 0 ? ids.join(" ") : undefined;
  }

  function commentAnchorClass(start: number, end: number): string {
    const anchors = anchorsForRange(start, end);
    if (anchors.length === 0) return "";
    return anchors.some((anchor) => anchor.status === "open")
      ? " notes-rich-text-comment-anchor"
      : " notes-rich-text-comment-anchor notes-rich-text-comment-anchor-resolved";
  }

  function suggestionAnchorsForRange(start: number, end: number): NotesResolvedSuggestionAnchor[] {
    return suggestionAnchors.filter((anchor) => anchor.start < end && anchor.end > start);
  }

  function suggestionAnchorIdsForRange(start: number, end: number): string | undefined {
    const ids = suggestionAnchorsForRange(start, end).map((anchor) => anchor.suggestionId);
    return ids.length > 0 ? ids.join(" ") : undefined;
  }

  function suggestionAnchorClass(start: number, end: number): string {
    const anchors = suggestionAnchorsForRange(start, end);
    if (anchors.length === 0) return "";
    if (anchors.some((anchor) => anchor.status === "open")) return " notes-rich-text-suggestion-anchor";
    if (anchors.some((anchor) => anchor.status === "accepted")) {
      return " notes-rich-text-suggestion-anchor notes-rich-text-suggestion-anchor-accepted";
    }
    return " notes-rich-text-suggestion-anchor notes-rich-text-suggestion-anchor-rejected";
  }

  function linkUrl(item: NotesRichText): string | null {
    const value = item.type === "text" ? item.text.link?.url ?? item.href : item.href;
    return value ? normalizeNotesTextLinkUrl(value, typeof window === "undefined" ? undefined : window.location.href) : null;
  }

  /** Editing hosts own their link popover; read-only rich text opens the same destinations directly. */
  function activateReadOnlyLink(event: MouseEvent | KeyboardEvent): void {
    if (!(event.currentTarget instanceof HTMLElement)) return;
    const url = event.currentTarget.dataset.notesLinkUrl;
    if (!url) return;
    const editor = event.currentTarget.closest<HTMLElement>("[contenteditable='true']");
    if (editor) {
      if (!(event instanceof MouseEvent)) return;
      event.preventDefault();
      if (editor.hasAttribute("data-notes-link-actions")) return;
      if (!event.ctrlKey && !event.metaKey && !editor.ownerDocument.getSelection()?.isCollapsed) return;
    }
    event.preventDefault();
    void openNotesTextLink(url).catch((error: unknown) => console.warn("Notes inline link navigation failed", error));
  }

  /** Show a mention's icon once, on the part where the mention begins. */
  function partStartsItem(part: VisibleRichTextLinePart): boolean {
    return part.start === visibleRuns.find((run) => run.item === part.item)?.start;
  }

  function mentionAttributes(part: VisibleRichTextLinePart): { class: string; style: string; [name: `data-${string}`]: string | undefined } {
    const { annotations } = part.item;
    return {
      class: `${textClass(part.item)}${commentAnchorClass(part.start, part.end)}${suggestionAnchorClass(part.start, part.end)}`,
      style: notesRichTextColorStyle(annotations.color),
      "data-notes-bold": annotations.bold ? "true" : undefined,
      "data-notes-italic": annotations.italic ? "true" : undefined,
      "data-notes-underline": annotations.underline ? "true" : undefined,
      "data-notes-strikethrough": annotations.strikethrough ? "true" : undefined,
      "data-notes-code": annotations.code ? "true" : undefined,
      "data-notes-rich-text-color": annotations.color === "default" ? undefined : annotations.color,
      "data-notes-comment-anchor": anchorIdsForRange(part.start, part.end),
      "data-notes-suggestion-anchor": suggestionAnchorIdsForRange(part.start, part.end),
    };
  }

  function handleLinkKeydown(event: KeyboardEvent): void {
    if (event.key === "Enter") activateReadOnlyLink(event);
  }

  function appendLinePart(
    line: VisibleRichTextLine,
    item: NotesRichText,
    text: string,
    start: number,
  ): void {
    if (text.length === 0) return;
    line.parts.push({
      item,
      text,
      start,
      end: start + text.length,
    });
  }

  function visibleRichTextLines(runs: readonly VisibleRichTextRun[]): VisibleRichTextLine[] {
    if (runs.length === 0) return [];
    const lines: VisibleRichTextLine[] = [{ parts: [] }];

    for (const run of runs) {
      for (const segment of splitRunByAnchors(run)) {
        const parts = segment.text.split("\n");
        let cursor = segment.start;
        for (let index = 0; index < parts.length; index += 1) {
          const part = parts[index] ?? "";
          appendLinePart(lines.at(-1) ?? lines[0], run.item, part, cursor);
          cursor += part.length;
          if (index < parts.length - 1) {
            lines.push({ parts: [] });
            cursor += 1;
          }
        }
      }
    }

    return lines;
  }
</script>{#each visibleLines as line}
  <div class="notes-rich-text-line" data-notes-editor-line="true">
    {#if line.parts.length === 0}
      <span class="notes-rich-text-empty-line-sentinel" data-notes-editor-sentinel="empty-line">{"\u200b"}</span>
    {:else}
      {#each line.parts as part}
        {@const url = linkUrl(part.item)}
        {#if part.item.type === "mention"}
          {#if isNotesLocalReference(part.item)}
            <NotesLocalReference reference={part.item} text={part.text} showIcon={partStartsItem(part)} attributes={mentionAttributes(part)} />
          {:else if isNotesExternalMentionRichText(part.item)}
            <NotesObjectMention mention={part.item} text={part.text} showIcon={partStartsItem(part)} attributes={mentionAttributes(part)} />
          {/if}
        {:else if part.item.type === "equation"}
          <svelte:element this={url ? "a" : "span"}
            href={url ?? undefined}
            role={url ? "link" : undefined}
            onclick={activateReadOnlyLink}
            onkeydown={handleLinkKeydown}
            class={`${textClass(part.item)} inline-flex max-w-full items-center rounded bg-muted/70 px-1 py-0.5 font-serif text-[1.02em]${commentAnchorClass(part.start, part.end)}${suggestionAnchorClass(part.start, part.end)}`}
            style={notesRichTextColorStyle(part.item.annotations.color)}
            title={part.item.equation.expression}
            data-notes-bold={part.item.annotations.bold ? "true" : undefined}
            data-notes-italic={part.item.annotations.italic ? "true" : undefined}
            data-notes-underline={part.item.annotations.underline ? "true" : undefined}
            data-notes-strikethrough={part.item.annotations.strikethrough ? "true" : undefined}
            data-notes-code={part.item.annotations.code ? "true" : undefined}
            data-notes-rich-text-color={part.item.annotations.color === "default" ? undefined : part.item.annotations.color}
            data-notes-link-url={linkUrl(part.item) ?? undefined}
            data-notes-comment-anchor={anchorIdsForRange(part.start, part.end)}
            data-notes-suggestion-anchor={suggestionAnchorIdsForRange(part.start, part.end)}
          >
            {part.text}
          </svelte:element>
        {:else}
          <svelte:element this={url ? "a" : "span"}
            href={url ?? undefined}
            role={url ? "link" : undefined}
            onclick={activateReadOnlyLink}
            onkeydown={handleLinkKeydown}
            class={`${textClass(part.item)}${commentAnchorClass(part.start, part.end)}${suggestionAnchorClass(part.start, part.end)}`}
            style={notesRichTextColorStyle(part.item.annotations.color)}
            data-notes-bold={part.item.annotations.bold ? "true" : undefined}
            data-notes-italic={part.item.annotations.italic ? "true" : undefined}
            data-notes-underline={part.item.annotations.underline ? "true" : undefined}
            data-notes-strikethrough={part.item.annotations.strikethrough ? "true" : undefined}
            data-notes-code={part.item.annotations.code ? "true" : undefined}
            data-notes-rich-text-color={part.item.annotations.color === "default" ? undefined : part.item.annotations.color}
            data-notes-link-url={linkUrl(part.item) ?? undefined}
            data-notes-comment-anchor={anchorIdsForRange(part.start, part.end)}
            data-notes-suggestion-anchor={suggestionAnchorIdsForRange(part.start, part.end)}
          >
            {part.text}
          </svelte:element>
        {/if}
      {/each}
    {/if}
  </div>
{/each}<style>
  .notes-rich-text-line {
    display: block;
    line-height: inherit;
  }

  .notes-rich-text-segment {
    color: var(--notes-rich-text-color, inherit);
    background: var(--notes-rich-text-bg, transparent);
    box-shadow: inset 0 0 0 1px var(--notes-rich-text-border, transparent);
  }

  .notes-rich-text-empty-line-sentinel {
    color: transparent;
  }

  .notes-rich-text-comment-anchor {
    border-radius: 0.2rem;
    background: hsl(var(--primary) / 0.16);
    box-shadow: inset 0 -0.12rem 0 hsl(var(--primary) / 0.45);
  }

  .notes-rich-text-comment-anchor-resolved {
    background: hsl(var(--muted-foreground) / 0.12);
    box-shadow: inset 0 -0.12rem 0 hsl(var(--muted-foreground) / 0.35);
  }

  .notes-rich-text-suggestion-anchor {
    border-radius: 0.2rem;
    background: hsl(var(--secondary) / 0.38);
    box-shadow: inset 0 -0.12rem 0 hsl(var(--primary) / 0.7);
  }

  .notes-rich-text-suggestion-anchor-accepted {
    background: hsl(var(--primary) / 0.12);
    box-shadow: inset 0 -0.12rem 0 hsl(var(--primary) / 0.45);
  }

  .notes-rich-text-suggestion-anchor-rejected {
    background: hsl(var(--destructive) / 0.1);
    box-shadow: inset 0 -0.12rem 0 hsl(var(--destructive) / 0.35);
  }
</style>
