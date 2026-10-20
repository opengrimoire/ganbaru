<script module lang="ts">
  const terminalResizeOwners = new Map<string, symbol>();
</script>

<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import * as chatApi from "$lib/api/chat";
  import type { ChatTerminalRead } from "$lib/chat/contracts";
  import { parseChatTerminal, parseChatTerminalOutput } from "$lib/chat/validation";
  import {
    applyTerminalOutput,
    terminalErrorMessage,
    sanitizeTerminalPaste,
    terminalPasteNeedsConfirmation,
  } from "$lib/chat/workspace/terminal-model";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChat } from "$lib/stores/chat.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import "@xterm/xterm/css/xterm.css";

  let {
    terminalRead,
    onState,
  }: {
    terminalRead: ChatTerminalRead;
    onState: (terminal: ChatTerminalRead) => void;
  } = $props();

  const { t } = getLocalization();
  const chat = useChat();
  const preferences = getPreferences();
  const terminalIdentity = untrack(() => ({
    id: terminalRead.id,
    threadId: terminalRead.threadId,
    workingFolderId: terminalRead.workingFolderId,
  }));
  const notifyState = untrack(() => onState);
  let host: HTMLDivElement | undefined = $state();
  let xterm: import("@xterm/xterm").Terminal | null = null;
  let refitTerminal: (() => void) | null = null;
  let outputState = { generation: 0, lastSequence: 0 };
  let error = $state<string | null>(null);
  const resizeOwnerId = Symbol("terminal-resize-owner");
  const TERMINAL_FONT_FAMILY = '"SF Mono", "SFMono-Regular", "JetBrains Mono", "Cascadia Code", Consolas, "Liberation Mono", Menlo, monospace';
  const TERMINAL_BASE_FONT_SIZE = 13;
  const terminalFontSize = $derived(TERMINAL_BASE_FONT_SIZE * preferences.fontScale);

  onMount(() => {
    let disposed = false;
    let resizeTimer: number | null = null;
    let resizeObserver: ResizeObserver | null = null;
    const disposers: (() => void)[] = [];
    const ownsResize = () => terminalResizeOwners.get(terminalIdentity.id) === resizeOwnerId;
    const releaseResize = () => {
      if (ownsResize()) terminalResizeOwners.delete(terminalIdentity.id);
    };
    void Promise.all([
      import("@xterm/xterm"),
      import("@xterm/addon-fit"),
      chatApi.readChatTerminalSnapshot(
        terminalIdentity.id,
        terminalIdentity.threadId,
        terminalIdentity.workingFolderId,
      ),
    ]).then(async ([xtermModule, fitModule, snapshot]) => {
      if (disposed || !host) return;
      const terminalHost = host;
      const terminal = new xtermModule.Terminal({
        allowProposedApi: false,
        convertEol: false,
        cursorBlink: true,
        disableStdin: !snapshot.terminal.running,
        scrollback: chat.settings?.configuration.behavior.terminalScrollbackLines ?? 10_000,
        fontFamily: TERMINAL_FONT_FAMILY,
        fontSize: terminalFontSize,
        fontWeight: "400",
        fontWeightBold: "600",
        letterSpacing: 0,
        lineHeight: 1.1,
        theme: terminalTheme(terminalHost),
      });
      const themeObserver = new MutationObserver(() => {
        terminal.options.theme = terminalTheme(terminalHost);
      });
      themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "style"] });
      disposers.push(() => themeObserver.disconnect());
      const fit = new fitModule.FitAddon();
      terminal.loadAddon(fit);
      terminal.open(host);
      xterm = terminal;
      outputState = { generation: snapshot.terminal.generation, lastSequence: 0 };
      for (const chunk of snapshot.scrollback) applyChunk(chunk);
      terminal.onData((data) => {
        if (terminal.options.disableStdin) return;
        claimResize();
        void chatApi.writeChatTerminal(
          terminalIdentity.id,
          terminalIdentity.threadId,
          terminalIdentity.workingFolderId,
          data,
        ).catch((reason: unknown) => { error = terminalMessage(reason); });
      });
      const resizeTerminal = () => {
        if (disposed) return;
        const proposed = fit.proposeDimensions();
        if (!proposed || proposed.cols < 20 || proposed.rows < 2) return;
        fit.fit();
        if (!terminalResizeOwners.has(terminalIdentity.id)) {
          terminalResizeOwners.set(terminalIdentity.id, resizeOwnerId);
        }
        if (!ownsResize()) return;
        void chatApi.resizeChatTerminal(
          terminalIdentity.id,
          terminalIdentity.threadId,
          terminalIdentity.workingFolderId,
          terminal.cols,
          terminal.rows,
        ).catch(() => undefined);
      };
      const scheduleResize = (delay: number) => {
        if (resizeTimer !== null) window.clearTimeout(resizeTimer);
        resizeTimer = window.setTimeout(resizeTerminal, delay);
      };
      refitTerminal = () => scheduleResize(0);
      const claimResize = () => {
        terminalResizeOwners.set(terminalIdentity.id, resizeOwnerId);
        scheduleResize(0);
      };
      const claimResizeFromInteraction = () => claimResize();
      host.addEventListener("focusin", claimResizeFromInteraction);
      host.addEventListener("pointerdown", claimResizeFromInteraction);
      disposers.push(() => {
        host?.removeEventListener("focusin", claimResizeFromInteraction);
        host?.removeEventListener("pointerdown", claimResizeFromInteraction);
      });
      resizeObserver = new ResizeObserver(() => {
        scheduleResize(80);
      });
      resizeObserver.observe(host);
      resizeTerminal();

      const unlistenOutput = await listen<unknown>("chat://terminal-output", (event) => {
        try {
          const chunk = parseChatTerminalOutput(event.payload);
          if (chunk.terminalId === terminalIdentity.id) applyChunk(chunk);
        } catch (reason: unknown) {
          error = terminalMessage(reason);
        }
      });
      if (disposed) {
        unlistenOutput();
        return;
      }
      disposers.push(unlistenOutput);
      const unlistenState = await listen<unknown>("chat://terminal-state", (event) => {
        try {
          const state = parseChatTerminal(event.payload);
          if (state.id === terminalIdentity.id) notifyState(state);
        } catch (reason: unknown) {
          error = terminalMessage(reason);
        }
      });
      if (disposed) {
        unlistenState();
        return;
      }
      disposers.push(unlistenState);
    }).catch((reason: unknown) => { error = terminalMessage(reason); });
    return () => {
      disposed = true;
      releaseResize();
      if (resizeTimer !== null) window.clearTimeout(resizeTimer);
      resizeObserver?.disconnect();
      refitTerminal = null;
      for (const dispose of disposers) dispose();
      xterm?.dispose();
      xterm = null;
    };
  });

  /** Reads the terminal colors from the host's current theme tokens; the scrollbar is styled by the shared tokens in CSS. */
  function terminalTheme(element: HTMLElement): import("@xterm/xterm").ITheme {
    const styles = getComputedStyle(element);
    const foreground = styles.color;
    return {
      background: styles.getPropertyValue("--cal-bg").trim() || styles.backgroundColor,
      foreground,
      cursor: foreground,
      selectionBackground: styles.getPropertyValue("--selection-background").trim(),
    };
  }

  function applyChunk(chunk: import("$lib/chat/contracts").ChatTerminalOutputChunk): void {
    if (!xterm) return;
    const decision = applyTerminalOutput(outputState, chunk);
    if (decision.kind === "accept") {
      outputState = decision.state;
      xterm.write(decision.bytes);
    } else if (decision.kind === "gap") {
      void replay().catch((reason: unknown) => { error = terminalMessage(reason); });
    }
  }

  async function replay(): Promise<void> {
    const snapshot = await chatApi.readChatTerminalSnapshot(
      terminalIdentity.id,
      terminalIdentity.threadId,
      terminalIdentity.workingFolderId,
    );
    xterm?.reset();
    outputState = { generation: snapshot.terminal.generation, lastSequence: 0 };
    for (const chunk of snapshot.scrollback) applyChunk(chunk);
    notifyState(snapshot.terminal);
  }

  /** Runs in the capture phase because xterm stops propagation of paste events on its textarea. */
  function handlePaste(event: ClipboardEvent): void {
    event.preventDefault();
    event.stopPropagation();
    const { text } = sanitizeTerminalPaste(event.clipboardData?.getData("text/plain") ?? "");
    if (!text) return;
    if (terminalPasteNeedsConfirmation(
      text,
      chat.settings?.configuration.behavior.confirmMultilineTerminalPaste ?? true,
    ) && !window.confirm(t("chat.inspector.confirmMultilinePaste") + "\n\n" + text)) return;
    xterm?.paste(text);
  }

  function terminalMessage(reason: unknown): string {
    return terminalErrorMessage(
      reason,
      t("common.viewLoadFailed", t("chat.inspector.terminal")),
    );
  }

  $effect(() => {
    if (xterm) xterm.options.disableStdin = !(terminalRead?.running ?? false);
  });

  $effect(() => {
    const fontSize = terminalFontSize;
    if (!xterm) return;
    xterm.options.fontSize = fontSize;
    refitTerminal?.();
  });
</script>

<div class="terminal-view" data-terminal-capture>
  {#if error}<Notice class="border-b border-border p-2 text-xs" message={error} />{/if}
  <div class="terminal-host" bind:this={host} onpastecapture={handlePaste}></div>
</div>

<style>
  .terminal-view { contain: layout paint; container-type: inline-size; display: flex; height: 100%; min-height: 0; flex-direction: column; background: var(--cal-bg); }
  .terminal-host { min-height: 0; flex: 1; overflow: hidden; color: var(--foreground); background: var(--cal-bg); }
  :global(.xterm) { height: 100%; padding: 0.45rem 0.55rem; }
  :global(.xterm-viewport), :global(.xterm-screen) { background: var(--cal-bg) !important; }
  /* xterm.js draws its own scrollbar, so it is restyled to match the shared app scrollbar and stays visible while the buffer overflows. */
  .terminal-host :global(.xterm-viewport) { scrollbar-width: none; }
  .terminal-host :global(.xterm-viewport::-webkit-scrollbar) { display: none; }
  .terminal-host :global(.xterm-scrollable-element > .scrollbar.vertical) { width: var(--app-scrollbar-size) !important; background: transparent !important; }
  .terminal-host :global(.xterm-scrollable-element > .scrollbar.invisible.fade) { opacity: 1; pointer-events: auto; transition: none; }
  .terminal-host :global(.xterm-scrollable-element > .scrollbar > .slider) { left: 0 !important; width: 100% !important; box-sizing: border-box; border: 2px solid transparent; border-radius: 9999px; background: var(--app-scrollbar-thumb) !important; background-clip: content-box !important; }
  .terminal-host :global(.xterm-scrollable-element > .scrollbar > .slider:hover), .terminal-host :global(.xterm-scrollable-element > .scrollbar > .slider.active) { background: var(--app-scrollbar-thumb-hover) !important; background-clip: content-box !important; }
</style>
