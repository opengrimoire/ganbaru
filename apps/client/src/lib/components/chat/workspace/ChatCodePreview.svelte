<script module lang="ts">
  import type { ChatCodeEditorSelection } from "$lib/chat/workspace/code-editor-runtime";

  export type ChatCodeSelection = ChatCodeEditorSelection;
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import type { ChatCodeEditorRuntime } from "$lib/chat/workspace/code-editor-runtime";
  import { loadChatCodeEditorRuntime } from "$lib/chat/workspace/code-editor-loader";
  import { chatSyntaxStyle } from "$lib/chat/workspace/syntax-theme";
  import { getTheme } from "$lib/stores/theme.svelte";

  let {
    text,
    relativePath,
    readOnly = false,
    onChange = () => {},
    onSelectionChange = () => {},
    onSave = () => {},
  }: {
    text: string;
    relativePath: string;
    readOnly?: boolean;
    onChange?: (text: string) => void;
    onSelectionChange?: (selection: ChatCodeSelection) => void;
    onSave?: () => void;
  } = $props();

  const theme = getTheme();
  const syntaxStyle = $derived(chatSyntaxStyle(theme.current));
  let host: HTMLDivElement | undefined = $state();
  let runtime: ChatCodeEditorRuntime | null = null;
  let enhanced = $state(false);
  let destroyed = false;

  onMount(() => {
    void loadRuntime();
    return () => {
      destroyed = true;
      runtime?.destroy();
      runtime = null;
    };
  });

  $effect(() => {
    runtime?.setText(text);
  });

  $effect(() => {
    runtime?.setRelativePath(relativePath);
  });

  $effect(() => {
    runtime?.setReadOnly(readOnly);
  });

  async function loadRuntime(): Promise<void> {
    if (!host || runtime || destroyed) return;
    try {
      const module = await loadChatCodeEditorRuntime();
      if (!host || destroyed) return;
      runtime = new module.ChatCodeEditorRuntime(host, {
        text,
        relativePath,
        readOnly,
        onChange: (nextText) => onChange(nextText),
        onSelectionChange: (selection) => onSelectionChange(selection),
        onSave: () => onSave(),
      });
      enhanced = true;
    } catch {
      runtime = null;
      enhanced = false;
    }
  }

  function handleFallbackSelection(element: HTMLTextAreaElement): void {
    const start = positionAt(text, element.selectionStart);
    const end = positionAt(text, element.selectionEnd);
    onSelectionChange({
      text: text.slice(element.selectionStart, element.selectionEnd),
      startLine: start.line,
      startColumn: start.column,
      endLine: end.line,
      endColumn: end.column,
    });
  }

  function positionAt(value: string, offset: number): { line: number; column: number } {
    const prefix = value.slice(0, offset);
    const lines = prefix.split("\n");
    return { line: lines.length, column: (lines.at(-1)?.length ?? 0) + 1 };
  }

  function handleFallbackKeydown(event: KeyboardEvent): void {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      onSave();
    }
  }
</script>

<div class="code-editor-shell" style={syntaxStyle} data-language-path={relativePath}>
  {#if !enhanced}
    <textarea
      class="plain-editor"
      value={text}
      readonly={readOnly}
      spellcheck="false"
      aria-label={relativePath}
      oninput={(event) => onChange(event.currentTarget.value)}
      onselect={(event) => handleFallbackSelection(event.currentTarget)}
      onkeyup={(event) => handleFallbackSelection(event.currentTarget)}
      onkeydown={handleFallbackKeydown}
    ></textarea>
  {/if}
  <div bind:this={host} class="code-editor" class:hidden={!enhanced}></div>
</div>

<style>
  .code-editor-shell { display: flex; min-width: 0; min-height: 0; flex: 1; overflow: hidden; background: var(--chat-syntax-background); }
  .code-editor { min-width: 0; min-height: 0; flex: 1; overflow: hidden; }
  .code-editor.hidden { display: none; }
  .code-editor :global(.cm-editor) { height: 100%; }
  .plain-editor { min-width: 0; min-height: 0; flex: 1; resize: none; border: 0; outline: 0; background: var(--chat-syntax-background); padding: 0.3rem 0.5rem; color: var(--chat-syntax-foreground); font-family: "SF Mono", "SFMono-Regular", "JetBrains Mono", "Cascadia Code", Consolas, "Liberation Mono", Menlo, monospace; font-size: calc(0.733333rem * var(--type-scale)); line-height: 21px; tab-size: 4; white-space: pre; }
</style>
