<script module lang="ts">
  import type { RenderedMarkdownBlock } from "$lib/chat/timeline/markdown";

  /** Whole renders of finished messages kept across mounts, oldest evicted first. */
  const MAX_FINISHED_MARKDOWN_RENDERS = 200;
  const finishedRenders = new Map<string, { markdown: string; blocks: RenderedMarkdownBlock[] }>();
</script>

<script lang="ts">
  import { onMount, tick } from "svelte";
  import { openChatExternalUrl } from "$lib/api/chat";
  import { renderChatMarkdownBlocks } from "$lib/chat/timeline/markdown";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import "./chat-markdown.css";

  let { markdown, renderKey, onError = () => {} }: {
    markdown: string;
    /** Identifies a finished text (for example a row id and its change sequence) so its render is reused; omit while it streams. */
    renderKey?: string;
    onError?: (message: string) => void;
  } = $props();
  const { t } = getLocalization();
  let root: HTMLDivElement | undefined = $state();
  const blockCache = new Map<string, string>();
  const blocks = $derived(renderBlocks(markdown, renderKey));

  onMount(() => {
    root?.addEventListener("click", handleClick);
    return () => root?.removeEventListener("click", handleClick);
  });

  $effect(() => {
    blocks;
    void tick().then(enhanceCodeBlocks);
  });

  function renderBlocks(text: string, key: string | undefined): RenderedMarkdownBlock[] {
    if (key === undefined) return renderChatMarkdownBlocks(text, blockCache);
    const cached = finishedRenders.get(key);
    if (cached?.markdown === text) return cached.blocks;
    const rendered = renderChatMarkdownBlocks(text, blockCache);
    finishedRenders.delete(key);
    finishedRenders.set(key, { markdown: text, blocks: rendered });
    if (finishedRenders.size > MAX_FINISHED_MARKDOWN_RENDERS) {
      const oldest = finishedRenders.keys().next().value;
      if (oldest !== undefined) finishedRenders.delete(oldest);
    }
    return rendered;
  }

  function enhanceCodeBlocks(): void {
    for (const pre of root?.querySelectorAll("pre") ?? []) {
      if (pre.dataset.chatEnhanced === "true") continue;
      pre.dataset.chatEnhanced = "true";
      const code = pre.querySelector("code");
      const toolbar = document.createElement("div");
      toolbar.className = "chat-code-toolbar";
      const language = document.createElement("span");
      language.textContent = code?.className.match(/language-([^ ]+)/)?.[1] ?? t("chat.timeline.code");
      const wrap = document.createElement("button");
      wrap.type = "button";
      wrap.textContent = t("chat.timeline.wrapCode");
      wrap.addEventListener("click", () => { pre.classList.toggle("chat-code-wrap"); });
      const copy = document.createElement("button");
      copy.type = "button";
      copy.textContent = t("chat.timeline.copyCode");
      copy.addEventListener("click", () => {
        void navigator.clipboard.writeText(code?.textContent ?? "").catch(reportError);
      });
      toolbar.append(language, wrap, copy);
      pre.prepend(toolbar);
    }
  }

  function handleClick(event: MouseEvent): void {
    const target = event.target instanceof Element ? event.target.closest<HTMLAnchorElement>("a[data-chat-external-link]") : null;
    if (!target) return;
    event.preventDefault();
    void openChatExternalUrl(target.href).catch(reportError);
  }

  function reportError(error: unknown): void {
    onError(error instanceof Error ? error.message : String(error));
  }
</script>

<div bind:this={root} class="chat-markdown" data-selectable-content>{#each blocks as block (block.key)}{@html block.html}{/each}</div>
