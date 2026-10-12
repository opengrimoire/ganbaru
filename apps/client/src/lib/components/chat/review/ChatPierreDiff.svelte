<script module lang="ts">
  import type { ReviewDiffRuntime as ReviewDiffViewportRuntime } from "$lib/chat/review/diff-runtime";

  export interface ChatDiffViewport {
    scrollToFile: ReviewDiffViewportRuntime["scrollToFile"];
    scrollToLine: ReviewDiffViewportRuntime["scrollToLine"];
    clearSelection: ReviewDiffViewportRuntime["clearSelection"];
  }
</script>

<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import type {
    ReviewAnnotationLabels,
    ReviewDiffRenderItem,
    ReviewDiffRuntime,
    ReviewLineSelection,
  } from "$lib/chat/review/diff-runtime";
  import { loadChatReviewDiffRuntime } from "$lib/chat/review/diff-loader";
  import { chatSyntaxStyle } from "$lib/chat/workspace/syntax-theme";
  import { getTheme } from "$lib/stores/theme.svelte";
  import ChatPlainDiff from "./ChatPlainDiff.svelte";

  let {
    active = true,
    items,
    selectedFileId,
    diffStyle,
    wrap,
    labels,
    fallbackLabel,
    rawLoadMoreLabel,
    onSelection,
    onActiveFile,
    onAttachComment,
    onResolveComment,
    onViewport = () => {},
  }: {
    active?: boolean;
    items: readonly ReviewDiffRenderItem[];
    selectedFileId: string | null;
    diffStyle: "unified" | "split";
    wrap: boolean;
    labels: ReviewAnnotationLabels;
    fallbackLabel: string;
    rawLoadMoreLabel: string;
    onSelection: (selection: ReviewLineSelection | null) => void;
    onActiveFile: (fileId: string) => void;
    onAttachComment: ReviewDiffRuntimeOptions["onAttachComment"];
    onResolveComment: ReviewDiffRuntimeOptions["onResolveComment"];
    onViewport?: (viewport: ChatDiffViewport | null) => void;
  } = $props();

  const theme = getTheme();
  const syntaxStyle = $derived(chatSyntaxStyle(theme.current));

  type ReviewDiffRuntimeOptions = import("$lib/chat/review/diff-runtime").ReviewDiffRuntimeOptions;
  let host: HTMLDivElement | undefined = $state();
  let runtime: ReviewDiffRuntime | null = null;
  let enhancedVisible = $state(false);
  let enhancedFailed = $state(false);
  let rejectedItems: ReviewDiffRenderItem[] = $state([]);
  let error = $state<string | null>(null);
  let destroyed = false;
  let cancelRenderWait: (() => void) | null = null;

  onMount(() => {
    const observer = new MutationObserver(() => applyOptions());
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
    return () => observer.disconnect();
  });

  onDestroy(() => {
    destroyed = true;
    releaseRuntime();
  });

  $effect(() => {
    if (!active) {
      releaseRuntime();
      return;
    }
    void loadRuntime();
  });

  $effect(() => {
    const nextItems = items;
    if (!runtime) return;
    try {
      rejectedItems = runtime.setItems(nextItems);
    } catch (reason: unknown) {
      failEnhanced(reason);
    }
  });

  $effect(() => {
    diffStyle;
    wrap;
    labels;
    applyOptions();
  });

  async function loadRuntime(): Promise<void> {
    if (!active || !host || runtime || destroyed) return;
    enhancedFailed = false;
    error = null;
    try {
      const module = await loadChatReviewDiffRuntime();
      if (!active || !host || destroyed) return;
      runtime = new module.ReviewDiffRuntime(host, runtimeOptions());
      rejectedItems = runtime.setItems(items);
      onViewport({
        scrollToFile: (fileId, behavior) => runtime?.scrollToFile(fileId, behavior),
        scrollToLine: (fileId, lineNumber, side) => runtime?.scrollToLine(fileId, lineNumber, side),
        clearSelection: () => runtime?.clearSelection(),
      });
      await waitForRenderedDiff(host);
      if (!destroyed) enhancedVisible = true;
    } catch (reason: unknown) {
      if (!destroyed) failEnhanced(reason);
    }
  }

  function releaseRuntime(): void {
    cancelRenderWait?.();
    cancelRenderWait = null;
    runtime?.cleanUp();
    runtime = null;
    enhancedVisible = false;
    rejectedItems = [];
    onViewport(null);
  }

  function applyOptions(): void {
    try {
      runtime?.setOptions(runtimeOptions());
    } catch (reason: unknown) {
      failEnhanced(reason);
    }
  }

  function runtimeOptions(): ReviewDiffRuntimeOptions {
    return {
      diffStyle,
      wrap,
      labels,
      onSelection,
      onAttachComment,
      onResolveComment,
      onActiveFile,
      onWorkerError: failEnhanced,
    };
  }

  function failEnhanced(reason: unknown): void {
    if (enhancedFailed) return;
    void reason;
    cancelRenderWait?.();
    cancelRenderWait = null;
    error = fallbackLabel;
    enhancedFailed = true;
    enhancedVisible = false;
    runtime?.cleanUp();
    runtime = null;
    onViewport(null);
  }

  function waitForRenderedDiff(element: HTMLElement): Promise<void> {
    const RENDER_TIMEOUT_MS = 5_000;
    return new Promise((resolve, reject) => {
      let settled = false;
      let frame = 0;
      const timeout = window.setTimeout(() => finish(new Error("The enhanced diff did not render in time")), RENDER_TIMEOUT_MS);
      const observer = new MutationObserver(queueCheck);
      observer.observe(element, { childList: true, subtree: true });
      cancelRenderWait = () => finish(new Error("The enhanced diff render was cancelled"));

      function finish(reason?: Error): void {
        if (settled) return;
        settled = true;
        window.clearTimeout(timeout);
        window.cancelAnimationFrame(frame);
        observer.disconnect();
        cancelRenderWait = null;
        if (reason) reject(reason);
        else resolve();
      }

      function check(): void {
        if (settled) return;
        const containers = element.querySelectorAll<HTMLElement>("diffs-container");
        const rendered = [...containers].some((container) => container.shadowRoot?.querySelector("pre") != null);
        if (rendered) {
          finish();
          return;
        }
        queueCheck();
      }

      function queueCheck(): void {
        if (settled || frame !== 0) return;
        frame = window.requestAnimationFrame(() => {
          frame = 0;
          check();
        });
      }

      check();
    });
  }
</script>

<div class="diff-stack" class:enhanced={enhancedVisible && !enhancedFailed} style={syntaxStyle}>
  <div bind:this={host} class="pierre-host" aria-hidden={!enhancedVisible || enhancedFailed}></div>
  {#if !enhancedVisible || enhancedFailed}
    <ChatPlainDiff
      {items}
      {selectedFileId}
      label={fallbackLabel}
      loadMoreLabel={rawLoadMoreLabel}
      initialRowLimit={enhancedFailed ? 12_000 : 3_000}
      onSelectFile={onActiveFile}
    />
  {:else if rejectedItems.length > 0}
    <div class="partial-fallback">
      <p role="status">{fallbackLabel}</p>
      <ChatPlainDiff items={rejectedItems} {selectedFileId} label={fallbackLabel} loadMoreLabel={rawLoadMoreLabel} onSelectFile={onActiveFile} />
    </div>
  {/if}
  {#if error}<span class="sr-only" role="status">{error}</span>{/if}
</div>

<style>
  .diff-stack { position: relative; display: flex; min-width: 0; min-height: 0; flex: 1; flex-direction: column; overflow: hidden; }
  .pierre-host { position: absolute; inset: 0; display: block; min-width: 0; min-height: 0; overflow: auto; visibility: hidden; pointer-events: none; overscroll-behavior: contain; scrollbar-gutter: stable; background: var(--cal-bg); color: var(--foreground); --diffs-font-size: calc(0.75rem * var(--type-scale)); --diffs-line-height: calc(1.25rem * var(--type-scale)); --diffs-font-family: "SF Mono", "SFMono-Regular", "JetBrains Mono", "Cascadia Code", Consolas, monospace; --diffs-header-font-family: inherit; --diffs-light-bg: var(--cal-bg); --diffs-dark-bg: var(--cal-bg); --diffs-light: var(--foreground); --diffs-dark: var(--foreground); --diffs-addition-color: var(--action-confirm); --diffs-deletion-color: var(--destructive); --diffs-modified-color: var(--primary); --diffs-bg-context-override: color-mix(in srgb, var(--muted) 58%, var(--cal-bg)); --diffs-bg-context-gutter-override: color-mix(in srgb, var(--muted) 76%, var(--cal-bg)); --diffs-bg-separator-override: color-mix(in srgb, var(--muted) 72%, var(--cal-bg)); }
  .enhanced > .pierre-host { position: relative; inset: auto; flex: 1; visibility: visible; pointer-events: auto; }
  .partial-fallback { display: flex; max-height: 16rem; min-height: 6rem; flex: 0 0 auto; flex-direction: column; border-top: 1px solid var(--border); }
  .partial-fallback > p { padding: 0.35rem 0.55rem; color: var(--muted-foreground); font-size: calc(0.666667rem * var(--type-scale)); }
</style>
