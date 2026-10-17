<script lang="ts">
  import { onMount, tick } from "svelte";
  import { openChatExternalUrl } from "$lib/api/chat";
  import { renderChatMarkdown } from "$lib/chat/timeline/markdown";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  let { markdown, onError = () => {} }: { markdown: string; onError?: (message: string) => void } = $props();
  const { t } = getLocalization();
  let root: HTMLDivElement | undefined = $state();
  const safeHtml = $derived(renderChatMarkdown(markdown));

  onMount(() => {
    root?.addEventListener("click", handleClick);
    return () => root?.removeEventListener("click", handleClick);
  });

  $effect(() => {
    safeHtml;
    void tick().then(enhanceCodeBlocks);
  });

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

<div bind:this={root} class="chat-markdown" data-selectable-content>{@html safeHtml}</div>

<style>
  .chat-markdown { overflow-wrap:anywhere; line-height:inherit; }
  .chat-markdown :global(p + p), .chat-markdown :global(p + ul), .chat-markdown :global(p + ol), .chat-markdown :global(ul + p), .chat-markdown :global(ol + p), .chat-markdown :global(pre + p), .chat-markdown :global(table + p), .chat-markdown :global(blockquote + p) { margin-top:var(--chat-conversation-flow-space,0.5rem); }
  .chat-markdown :global(pre:not(:first-child)), .chat-markdown :global(table:not(:first-child)), .chat-markdown :global(blockquote:not(:first-child)) { margin-top:var(--chat-conversation-block-space,0.65rem); }
  .chat-markdown :global(h1), .chat-markdown :global(h2), .chat-markdown :global(h3) { font-weight:650; }
  .chat-markdown :global(h1:not(:first-child)), .chat-markdown :global(h2:not(:first-child)), .chat-markdown :global(h3:not(:first-child)) { margin-top:0.8rem; }
  .chat-markdown :global(ul), .chat-markdown :global(ol) { padding-left: 1.4rem; }
  .chat-markdown :global(ul) { list-style: disc; }
  .chat-markdown :global(ol) { list-style: decimal; }
  .chat-markdown :global(blockquote) { border-left: 2px solid var(--border); padding-left: 0.8rem; color: var(--muted-foreground); }
  .chat-markdown :global(pre) { overflow-x:auto; border:1px solid var(--border); border-radius:0.5rem; background:var(--muted); padding:0.6rem; font-size: calc(0.8rem * var(--type-scale)); }
  .chat-markdown :global(pre.chat-code-wrap code) { white-space: pre-wrap; overflow-wrap: anywhere; }
  .chat-markdown :global(.chat-code-toolbar) { display:flex; align-items:center; gap:0.5rem; margin:-0.3rem -0.3rem 0.4rem; color:var(--muted-foreground); font-family:sans-serif; font-size: calc(0.75rem * var(--type-scale)); }
  .chat-markdown :global(.chat-code-toolbar span) { margin-right: auto; }
  .chat-markdown :global(.chat-code-toolbar button) { border-radius: 0.25rem; padding: 0.15rem 0.35rem; }
  .chat-markdown :global(.chat-code-toolbar button:hover) { background: var(--accent); color: var(--foreground); }
  .chat-markdown :global(table) { display: block; max-width: 100%; overflow-x: auto; border-collapse: collapse; }
  .chat-markdown :global(th), .chat-markdown :global(td) { border: 1px solid var(--border); padding: 0.35rem 0.5rem; text-align: left; }
  .chat-markdown :global(a) { color: var(--primary); text-decoration: underline; text-underline-offset: 2px; }
</style>
