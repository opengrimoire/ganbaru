<script lang="ts">
  import type { ReviewDiffRenderItem } from "$lib/chat/review/diff-runtime";

  let {
    items,
    selectedFileId = null,
    label,
    loadMoreLabel,
    initialRowLimit = 12_000,
    rowBatchSize = 12_000,
    onSelectFile = () => {},
  }: {
    items: readonly ReviewDiffRenderItem[];
    selectedFileId?: string | null;
    label: string;
    loadMoreLabel: string;
    initialRowLimit?: number;
    rowBatchSize?: number;
    onSelectFile?: (fileId: string) => void;
  } = $props();

  const ROW_HEIGHT = 20;
  const OVERSCAN = 36;
  type PlainDiffRowKind = "header" | "addition" | "deletion" | "metadata" | "context" | "more";
  interface PlainDiffRow {
    key: string;
    fileId: string;
    kind: PlainDiffRowKind;
    text: string;
  }

  let scroller: HTMLDivElement | undefined = $state();
  let viewportHeight = $state(400);
  let scrollTop = $state(0);
  let additionalRowLimit = $state(0);
  const visibleRowLimit = $derived(initialRowLimit + additionalRowLimit);
  const rows = $derived(buildRows(items, visibleRowLimit, loadMoreLabel));
  const startIndex = $derived(Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN));
  const endIndex = $derived(Math.min(rows.length, Math.ceil((scrollTop + viewportHeight) / ROW_HEIGHT) + OVERSCAN));
  const visibleRows = $derived(rows.slice(startIndex, endIndex));

  $effect(() => {
    if (!scroller) return;
    const observer = new ResizeObserver(([entry]) => {
      if (entry) viewportHeight = entry.contentRect.height;
    });
    observer.observe(scroller);
    return () => observer.disconnect();
  });

  function lineKind(text: string): "addition" | "deletion" | "metadata" | "context" {
    if (text.startsWith("+") && !text.startsWith("+++")) return "addition";
    if (text.startsWith("-") && !text.startsWith("---")) return "deletion";
    if (text.startsWith("@@") || text.startsWith("diff ") || text.startsWith("index ") || text.startsWith("+++") || text.startsWith("---")) return "metadata";
    return "context";
  }

  function buildRows(
    sourceItems: readonly ReviewDiffRenderItem[],
    limit: number,
    moreLabel: string,
  ): PlainDiffRow[] {
    const nextRows: PlainDiffRow[] = [];
    let truncated = false;
    itemLoop: for (const item of sourceItems) {
      if (nextRows.length >= limit) {
        truncated = true;
        break;
      }
      nextRows.push({
        key: `${item.key}:header`,
        fileId: item.fileId,
        kind: "header",
        text: item.fileName,
      });
      let lineStart = 0;
      let lineIndex = 0;
      while (lineStart <= item.patch.length) {
        if (nextRows.length >= limit) {
          truncated = true;
          break itemLoop;
        }
        const newlineIndex = item.patch.indexOf("\n", lineStart);
        const lineEnd = newlineIndex < 0 ? item.patch.length : newlineIndex;
        const text = item.patch.slice(lineStart, lineEnd);
        nextRows.push({
          key: `${item.key}:${lineIndex}`,
          fileId: item.fileId,
          kind: lineKind(text),
          text,
        });
        if (newlineIndex < 0) break;
        lineStart = newlineIndex + 1;
        lineIndex += 1;
      }
    }
    if (truncated) {
      nextRows.push({
        key: `raw-diff-more:${limit}`,
        fileId: "",
        kind: "more",
        text: moreLabel,
      });
    }
    return nextRows;
  }
</script>

<div
  bind:this={scroller}
  class="plain-diff"
  role="region"
  aria-label={label}
  onscroll={(event) => { scrollTop = event.currentTarget.scrollTop; }}
>
  <div class="plain-diff-space" style:height={`${rows.length * ROW_HEIGHT}px`}>
    <div class="plain-diff-window" style:transform={`translateY(${startIndex * ROW_HEIGHT}px)`}>
      {#each visibleRows as row (row.key)}
        {#if row.kind === "header"}
          <button
            type="button"
            class="plain-file-header"
            class:active={row.fileId === selectedFileId}
            title={row.text}
            onclick={() => onSelectFile(row.fileId)}
          >{row.text}</button>
        {:else if row.kind === "more"}
          <button type="button" class="plain-diff-more" onclick={() => { additionalRowLimit += rowBatchSize; }}>{row.text}</button>
        {:else}
          <div class="plain-diff-line {row.kind}"><code>{row.text || " "}</code></div>
        {/if}
      {/each}
    </div>
  </div>
</div>

<style>
  .plain-diff { min-width: 0; min-height: 0; flex: 1; overflow: auto; background: var(--cal-bg); color: var(--foreground); font-family: "SF Mono", "SFMono-Regular", "JetBrains Mono", "Cascadia Code", Consolas, monospace; font-size: calc(0.7rem * var(--type-scale)); line-height: 20px; }
  .plain-diff-space { position: relative; min-width: max-content; }
  .plain-diff-window { position: absolute; inset: 0 auto auto 0; width: 100%; }
  .plain-file-header { position: sticky; left: 0; display: block; width: 100%; height: 20px; overflow: hidden; border-block: 1px solid var(--border); background: var(--background); padding-inline: 0.65rem; color: var(--foreground); text-align: left; text-overflow: ellipsis; white-space: nowrap; }
  .plain-file-header.active { box-shadow: inset 2px 0 var(--primary); }
  .plain-diff-line { height: 20px; padding-inline: 0.65rem; white-space: pre; }
  .plain-diff-line.addition { background: color-mix(in srgb, var(--action-confirm) 13%, var(--cal-bg)); }
  .plain-diff-line.deletion { background: color-mix(in srgb, var(--destructive) 12%, var(--cal-bg)); }
  .plain-diff-line.metadata { background: color-mix(in srgb, var(--muted) 72%, var(--cal-bg)); color: var(--muted-foreground); }
  .plain-diff-more { position: sticky; left: 0; display: block; width: 100%; height: 20px; background: var(--background); padding-inline: 0.65rem; color: var(--primary); text-align: left; }
  .plain-diff-more:hover { background: var(--accent); }
</style>
