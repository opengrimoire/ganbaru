<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Camera from "@lucide/svelte/icons/camera";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Monitor from "@lucide/svelte/icons/monitor";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Search from "@lucide/svelte/icons/search";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import ShieldOff from "@lucide/svelte/icons/shield-off";
  import Smartphone from "@lucide/svelte/icons/smartphone";
  import Tablet from "@lucide/svelte/icons/tablet";
  import Video from "@lucide/svelte/icons/video";
  import Plus from "@lucide/svelte/icons/plus";
  import X from "@lucide/svelte/icons/x";
  import * as chatApi from "$lib/api/chat";
  import type { BrowserTabBounds, BrowserTabRead } from "$lib/chat/contracts";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChat } from "$lib/stores/chat.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  type ViewportPreset = "responsive" | "mobile" | "tablet" | "desktop";

  const { t } = getLocalization();
  const chat = useChat();
  let viewport: HTMLDivElement | undefined = $state();
  let browserTab: BrowserTabRead | null = $state(null);
  let tabs = $state<BrowserTabRead[]>([]);
  let tabId: string | null = $state(null);
  let loadedThreadId: string | null = null;
  let url = $state("");
  let loading = $state(false);
  let recording = $state(false);
  let artifactStatus: string | null = $state(null);
  let discoveredServers: chatApi.DiscoveredPreviewServer[] = $state([]);
  let modelOrigins = $state<string[]>([]);
  let approval = $state<chatApi.PreviewApproval | null>(null);
  let error: string | null = $state(null);
  let preset: ViewportPreset = $state("responsive");
  let resizeObserver: ResizeObserver | null = null;
  let resizeFrame: number | null = null;
  const threadId = $derived(chat.selectedThreadId ?? chat.draftThreadId);
  const currentOrigin = $derived(externalOrigin(browserTab));
  const modelAllowed = $derived(currentOrigin !== null && modelOrigins.includes(currentOrigin));

  onMount(() => {
    resizeObserver = new ResizeObserver(() => queueResize());
    if (viewport) resizeObserver.observe(viewport);
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void listen<unknown>(chatApi.CHAT_BROWSER_APPROVAL_EVENT, (event) => {
      const payload = event.payload;
      if (typeof payload !== "object" || payload === null) return;
      const notified = (payload as Record<string, unknown>).threadId;
      if (typeof notified === "string" && notified === loadedThreadId) void loadApproval(notified);
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    }).catch((reason: unknown) => {
      error = message(reason);
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  async function loadApproval(selectedThreadId: string): Promise<void> {
    try {
      const pending = await chatApi.readPendingBrowserApproval(selectedThreadId);
      if (selectedThreadId === loadedThreadId) approval = pending;
    } catch (reason) {
      error = message(reason);
    }
  }

  async function resolveApproval(allow: boolean): Promise<void> {
    if (!threadId || !approval) return;
    const selectedThread = threadId;
    approval = null;
    error = null;
    try {
      const navigated = await chatApi.resolveBrowserApproval(selectedThread, allow);
      if (navigated && selectedThread === threadId) {
        tabs = tabs.map((tab) => tab.tabId === navigated.tabId ? navigated : tab);
        if (navigated.tabId === tabId) {
          browserTab = navigated;
          url = navigated.currentUrl;
        }
      }
    } catch (reason) {
      error = message(reason);
    }
  }

  $effect(() => {
    const selectedThreadId = threadId;
    if (selectedThreadId === loadedThreadId) return;
    const previousThreadId = loadedThreadId;
    const previousTabId = tabId;
    const previousTabVisible = browserTab?.visible ?? false;
    if (recording && previousThreadId && previousTabId) {
      recording = false;
      void finishRecording(previousThreadId, previousTabId, false);
    }
    loadedThreadId = selectedThreadId;
    if (previousThreadId && previousTabId && previousTabVisible) {
      void chatApi.setBrowserTabVisible(previousThreadId, previousTabId, false).catch(() => undefined);
    }
    void loadStoredBrowserTab(selectedThreadId);
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
    if (resizeFrame !== null) cancelAnimationFrame(resizeFrame);
    if (threadId && tabId && browserTab?.visible) {
      void chatApi.setBrowserTabVisible(threadId, tabId, false);
    }
    if (recording && threadId && tabId) {
      recording = false;
      void finishRecording(threadId, tabId, false);
    }
  });

  function message(reason: unknown): string {
    if (reason instanceof Error) return reason.message;
    if (typeof reason === "object" && reason !== null && "message" in reason) {
      const value = (reason as { message?: unknown }).message;
      if (typeof value === "string") return value;
    }
    return t("chat.browser.unavailable");
  }

  async function loadStoredBrowserTab(selectedThreadId: string | null): Promise<void> {
    browserTab = null;
    tabs = [];
    url = "";
    modelOrigins = [];
    approval = null;
    tabId = selectedThreadId ? `preview:${selectedThreadId}:main` : null;
    if (!selectedThreadId) return;
    void loadApproval(selectedThreadId);
    try {
      const [stored, grants] = await Promise.all([
        chatApi.readBrowserStatus(selectedThreadId),
        chatApi.readPreviewModelOrigins(),
      ]);
      if (selectedThreadId !== loadedThreadId) return;
      tabs = stored;
      modelOrigins = grants.filter((grant) => grant.threadId === selectedThreadId).map((grant) => grant.origin);
      const selected = stored.find((tab) => tab.visible) ?? stored[0] ?? null;
      if (selected) {
        tabId = selected.tabId;
        browserTab = selected;
        url = selected.currentUrl;
      }
    } catch (reason) {
      error = message(reason);
    }
  }

  function normalizedUrl(value: string): string {
    const trimmed = value.trim();
    return /^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed) ? trimmed : `http://${trimmed}`;
  }

  function isExternal(value: string): boolean {
    try {
      const host = new URL(value).hostname;
      return host !== "localhost" && host !== "127.0.0.1" && host !== "::1";
    } catch {
      return false;
    }
  }

  function externalOrigin(tab: BrowserTabRead | null): string | null {
    return tab?.visible && isExternal(tab.currentUrl) ? originOf(tab.currentUrl) : null;
  }

  function originOf(value: string): string | null {
    try {
      return new URL(value).origin;
    } catch {
      return null;
    }
  }

  async function toggleModelAccess(): Promise<void> {
    if (!threadId || !tabId || !currentOrigin || loading) return;
    const selectedThread = threadId;
    loading = true;
    error = null;
    try {
      if (modelAllowed) {
        await chatApi.revokePreviewModelOrigin(selectedThread, currentOrigin);
        modelOrigins = modelOrigins.filter((origin) => origin !== currentOrigin);
      } else {
        const origin = await chatApi.allowPreviewModelOrigin(selectedThread, tabId);
        if (selectedThread === threadId && !modelOrigins.includes(origin)) modelOrigins = [...modelOrigins, origin];
      }
    } catch (reason) {
      error = message(reason);
    } finally {
      loading = false;
    }
  }

  async function navigate(): Promise<void> {
    if (!threadId || !tabId || !url.trim() || loading) return;
    const destination = normalizedUrl(url);
    const externalConfirmed = !isExternal(destination)
      || window.confirm(t("chat.browser.confirmExternal", destination));
    if (!externalConfirmed) return;
    loading = true;
    error = null;
    try {
      if (browserTab?.visible) {
        browserTab = await chatApi.navigateBrowserTab(threadId, tabId, destination, externalConfirmed);
      } else {
        await tick();
        browserTab = await chatApi.openBrowserTab({
          threadId,
          tabId,
          url: destination,
          bounds: currentBounds(),
          externalNavigationConfirmed: externalConfirmed,
        });
      }
      url = browserTab.currentUrl;
      upsertTab(browserTab);
    } catch (reason) {
      error = message(reason);
    } finally {
      loading = false;
    }
  }

  function upsertTab(tab: BrowserTabRead): void {
    tabs = [...tabs.filter((entry) => entry.tabId !== tab.tabId), tab];
  }

  async function selectTab(tab: BrowserTabRead): Promise<void> {
    if (!threadId || tab.tabId === tabId) return;
    if (recording && tabId) await finishRecording(threadId, tabId, true);
    if (browserTab?.visible && tabId) {
      await chatApi.setBrowserTabVisible(threadId, tabId, false).catch(() => undefined);
      browserTab = { ...browserTab, visible: false };
      upsertTab(browserTab);
    }
    tabId = tab.tabId;
    browserTab = tab;
    url = tab.currentUrl;
    if (tab.visible) return;
    try {
      browserTab = await chatApi.setBrowserTabVisible(threadId, tab.tabId, true);
      upsertTab(browserTab);
    } catch {
      browserTab = { ...tab, visible: false };
    }
  }

  async function newTab(): Promise<void> {
    if (!threadId) return;
    if (recording && tabId) await finishRecording(threadId, tabId, true);
    if (browserTab?.visible && tabId) {
      await chatApi.setBrowserTabVisible(threadId, tabId, false).catch(() => undefined);
      browserTab = { ...browserTab, visible: false };
      upsertTab(browserTab);
    }
    tabId = `preview:${threadId}:${crypto.randomUUID()}`;
    browserTab = null;
    url = "";
  }

  async function closeTab(event: MouseEvent, tab: BrowserTabRead): Promise<void> {
    event.stopPropagation();
    if (!threadId) return;
    try {
      if (recording && tab.tabId === tabId) await finishRecording(threadId, tab.tabId, true);
      await chatApi.closeBrowserTab(threadId, tab.tabId);
      tabs = tabs.filter((entry) => entry.tabId !== tab.tabId);
      if (tab.tabId === tabId) {
        const next = tabs[0] ?? null;
        browserTab = null;
        tabId = next ? null : `preview:${threadId}:${crypto.randomUUID()}`;
        url = next?.currentUrl ?? "";
        if (next) await selectTab(next);
      }
    } catch (reason) {
      error = message(reason);
    }
  }

  function currentBounds(): BrowserTabBounds {
    if (!viewport) throw new Error(t("chat.browser.unavailable"));
    const container = viewport.getBoundingClientRect();
    const target = presetSize(container.width, container.height, preset);
    return {
      x: container.left + (container.width - target.width) / 2,
      y: container.top + (container.height - target.height) / 2,
      width: Math.max(1, target.width),
      height: Math.max(1, target.height),
    };
  }

  function presetSize(width: number, height: number, value: ViewportPreset): { width: number; height: number } {
    if (value === "mobile") return { width: Math.min(width, 390), height: Math.min(height, 844) };
    if (value === "tablet") return { width: Math.min(width, 768), height: Math.min(height, 1024) };
    if (value === "desktop") return { width: Math.min(width, 1440), height: Math.min(height, 900) };
    return { width, height };
  }

  function queueResize(): void {
    if (resizeFrame !== null) cancelAnimationFrame(resizeFrame);
    resizeFrame = requestAnimationFrame(() => {
      resizeFrame = null;
      void resize();
    });
  }

  async function resize(): Promise<void> {
    if (!browserTab?.visible || !threadId || !tabId || !viewport) return;
    try {
      browserTab = await chatApi.resizeBrowserTab(threadId, tabId, currentBounds());
    } catch (reason) {
      error = message(reason);
    }
  }

  function setPreset(value: ViewportPreset): void {
    preset = value;
    queueResize();
  }

  async function control(action: () => Promise<void>): Promise<void> {
    error = null;
    try {
      await action();
    } catch (reason) {
      error = message(reason);
    }
  }

  async function captureScreenshot(): Promise<void> {
    if (!threadId || !tabId || !browserTab?.visible || loading) return;
    loading = true;
    error = null;
    try {
      const artifact = await chatApi.captureBrowserScreenshot(threadId, tabId);
      artifactStatus = t("chat.browser.artifactSaved", artifact.displayName);
    } catch (reason) {
      error = message(reason);
    } finally {
      loading = false;
    }
  }

  async function toggleRecording(): Promise<void> {
    if (!threadId || !tabId || !browserTab?.visible || loading) return;
    const selectedThread = threadId;
    const selectedTab = tabId;
    if (!recording && !window.confirm(t("chat.browser.recordingConfirm"))) return;
    loading = true;
    error = null;
    try {
      if (recording) {
        await finishRecording(selectedThread, selectedTab, true);
      } else {
        await chatApi.startBrowserRecording(selectedThread, selectedTab);
        recording = true;
      }
    } catch (reason) {
      error = message(reason);
    } finally {
      loading = false;
    }
  }

  async function finishRecording(
    selectedThread: string,
    selectedTab: string,
    reportResult: boolean,
  ): Promise<void> {
    recording = false;
    try {
      const artifact = await chatApi.stopBrowserRecording(selectedThread, selectedTab);
      if (reportResult) artifactStatus = t("chat.browser.artifactSaved", artifact.displayName);
    } catch (reason) {
      if (reportResult) error = message(reason);
    }
  }

  async function discoverServers(): Promise<void> {
    if (!threadId || loading) return;
    loading = true;
    error = null;
    try {
      discoveredServers = await chatApi.discoverPreviewServers(threadId);
      if (discoveredServers.length === 0) artifactStatus = t("chat.browser.noServers");
    } catch (reason) {
      error = message(reason);
    } finally {
      loading = false;
    }
  }
</script>

<section class="browser-panel" aria-label={t("chat.inspector.browser")}>
  <div class="browser-tabs" role="tablist" aria-label={t("chat.browser.tabs")}>
    {#each tabs as tab (tab.tabId)}
      <div class="browser-tab" class:active={tab.tabId === tabId}>
        <button type="button" role="tab" aria-selected={tab.tabId === tabId} onclick={() => void selectTab(tab)}><span>{tab.title || tab.currentUrl}</span></button>
        <button type="button" class="close-tab" aria-label={t("chat.browser.closeTab")} onclick={(event) => void closeTab(event, tab)}><X size={11} /></button>
      </div>
    {/each}
    <button type="button" class="new-tab" aria-label={t("chat.browser.newTab")} title={t("chat.browser.newTab")} onclick={() => void newTab()}><Plus size={13} /></button>
  </div>
  <form class="browser-toolbar" onsubmit={(event) => { event.preventDefault(); void navigate(); }}>
    <button type="button" class="chat-icon-button" disabled={!browserTab?.visible} aria-label={t("chat.browser.back")} title={t("chat.browser.back")} onclick={() => { const selectedThread = threadId; const selectedTab = tabId; if (selectedThread && selectedTab) void control(() => chatApi.browserBack(selectedThread, selectedTab)); }}><ArrowLeft size={14} /></button>
    <button type="button" class="chat-icon-button" disabled={!browserTab?.visible} aria-label={t("chat.browser.forward")} title={t("chat.browser.forward")} onclick={() => { const selectedThread = threadId; const selectedTab = tabId; if (selectedThread && selectedTab) void control(() => chatApi.browserForward(selectedThread, selectedTab)); }}><ArrowRight size={14} /></button>
    <button type="button" class="chat-icon-button" disabled={!browserTab?.visible} aria-label={t("chat.browser.refresh")} title={t("chat.browser.refresh")} onclick={() => { const selectedThread = threadId; const selectedTab = tabId; if (selectedThread && selectedTab) void control(() => chatApi.refreshBrowserTab(selectedThread, selectedTab)); }}><RefreshCw size={14} /></button>
    <input class="field" bind:value={url} autocomplete="url" spellcheck="false" aria-label={t("chat.browser.url")} placeholder={t("chat.browser.urlPlaceholder")} />
    <button type="button" class="chat-icon-button" disabled={!threadId || loading} aria-label={t("chat.browser.discoverServers")} title={t("chat.browser.discoverServers")} onclick={() => void discoverServers()}><Search size={14} /></button>
    <button type="submit" class="chat-icon-button" disabled={!url.trim() || loading} aria-label={t("chat.browser.open")} title={t("chat.browser.open")}><ExternalLink size={14} /></button>
  </form>
  {#if discoveredServers.length > 0}
    <div class="server-list">
      {#each discoveredServers as server (server.url)}
        <button type="button" title={server.sourceLabel} onclick={() => { url = server.url; void navigate(); }}>{server.url}</button>
      {/each}
    </div>
  {/if}
  <div class="viewport-toolbar" role="group" aria-label={t("chat.browser.viewport")}>
    <button type="button" class:active={preset === "responsive"} onclick={() => setPreset("responsive")}>{t("chat.browser.responsive")}</button>
    <button type="button" class:active={preset === "mobile"} aria-label={t("chat.browser.mobile")} title={t("chat.browser.mobile")} onclick={() => setPreset("mobile")}><Smartphone size={13} /></button>
    <button type="button" class:active={preset === "tablet"} aria-label={t("chat.browser.tablet")} title={t("chat.browser.tablet")} onclick={() => setPreset("tablet")}><Tablet size={13} /></button>
    <button type="button" class:active={preset === "desktop"} aria-label={t("chat.browser.desktop")} title={t("chat.browser.desktop")} onclick={() => setPreset("desktop")}><Monitor size={13} /></button>
    <button type="button" disabled={!browserTab?.visible || loading} aria-label={t("chat.browser.screenshot")} title={t("chat.browser.screenshot")} onclick={() => void captureScreenshot()}><Camera size={13} /></button>
    <button type="button" class:active={recording} disabled={!browserTab?.visible || loading} aria-label={recording ? t("chat.browser.recordingStop") : t("chat.browser.recordingStart")} title={recording ? t("chat.browser.recordingStop") : t("chat.browser.recordingStart")} onclick={() => void toggleRecording()}><Video size={13} /></button>
    {#if currentOrigin}<button type="button" class:active={modelAllowed} disabled={loading} aria-pressed={modelAllowed} aria-label={modelAllowed ? t("chat.browser.modelAccessRevoke", currentOrigin) : t("chat.browser.modelAccessAllow", currentOrigin)} title={modelAllowed ? t("chat.browser.modelAccessRevoke", currentOrigin) : t("chat.browser.modelAccessAllow", currentOrigin)} onclick={() => void toggleModelAccess()}>{#if modelAllowed}<ShieldCheck size={13} />{:else}<ShieldOff size={13} />{/if}</button>{/if}
    {#if browserTab}<span>{browserTab.viewportWidth} × {browserTab.viewportHeight}</span>{/if}
  </div>
  {#if approval}
    <div class="approval" role="alert">
      <p>{approval.kind === "navigation" ? t("chat.browser.approvalNavigation", approval.origin) : t("chat.browser.approvalNetwork")}</p>
      <div>
        <button type="button" class="chat-secondary-button" onclick={() => void resolveApproval(false)}>{t("chat.browser.approvalDismiss")}</button>
        <button type="button" class="chat-primary-button" onclick={() => void resolveApproval(true)}>{t("chat.browser.approvalAllow")}</button>
      </div>
    </div>
  {/if}
  {#if artifactStatus}<p role="status" class="artifact-status">{artifactStatus}</p>{/if}
  {#if error}<Notice class="error" message={error} />{/if}
  <div bind:this={viewport} class="browser-viewport">
    {#if !browserTab?.visible}
      <div class="empty">
        <Monitor size={24} />
        <p>{browserTab ? t("chat.browser.reopen") : t("chat.inspector.browserUnavailable")}</p>
        {#if browserTab}<button type="button" class="chat-secondary-button" onclick={() => void navigate()}>{t("chat.browser.open")}</button>{/if}
      </div>
    {/if}
  </div>
</section>

<style>
  .browser-panel { display: flex; height: 100%; min-height: 0; flex-direction: column; overflow: hidden; background: var(--background); }
  .browser-tabs { display: flex; min-width: 0; align-items: center; gap: 0.15rem; overflow-x: auto; border-bottom: 1px solid var(--border); padding: 0.2rem 0.35rem; }
  .browser-tab { display: flex; max-width: 12rem; min-width: 0; flex: 0 1 9rem; align-items: center; border-radius: 0.35rem; color: var(--muted-foreground); font-size: calc(0.64rem * var(--type-scale)); }
  .browser-tab.active { background: var(--accent); color: var(--foreground); }
  .browser-tab > button:first-child { min-width: 0; flex: 1; padding: 0.25rem 0.1rem 0.25rem 0.35rem; }
  .browser-tab > button:first-child span { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align: left; }
  .browser-tab .close-tab { display: grid; flex: 0 0 auto; place-items: center; border-radius: 0.2rem; padding: 0.2rem; }
  .browser-tab .close-tab:hover { background: color-mix(in srgb, var(--foreground) 10%, transparent); }
  .browser-tabs > button.new-tab { flex: 0 0 auto; justify-content: center; }
  .browser-toolbar { display: flex; align-items: center; gap: 0.2rem; border-bottom: 1px solid var(--border); padding: 0.35rem 0.45rem; }
  .browser-toolbar input { min-width: 0; flex: 1; font-size: var(--panel-font-size); }
  .server-list { display: flex; gap: 0.25rem; overflow-x: auto; border-bottom: 1px solid var(--border); padding: 0.3rem 0.45rem; }
  .server-list button { flex: 0 0 auto; border-radius: 0.35rem; background: var(--accent); padding: 0.2rem 0.4rem; color: var(--muted-foreground); font-size: calc(0.64rem * var(--type-scale)); }
  .viewport-toolbar { display: flex; align-items: center; gap: 0.2rem; border-bottom: 1px solid var(--border); padding: 0.28rem 0.45rem; }
  .viewport-toolbar button { display: inline-flex; min-height: 1.6rem; align-items: center; justify-content: center; border-radius: 0.35rem; padding: 0.2rem 0.4rem; color: var(--foreground); font-size: calc(0.66rem * var(--type-scale)); }
  .viewport-toolbar button:hover, .viewport-toolbar button.active { background: var(--accent); }
  .viewport-toolbar span { margin-left: auto; color: var(--muted-foreground); font-size: calc(0.62rem * var(--type-scale)); }
  .browser-viewport { position: relative; min-height: 0; flex: 1; overflow: hidden; background: color-mix(in srgb, var(--muted) 28%, var(--background)); }
  .empty { display: grid; height: 100%; place-items: center; align-content: center; gap: 0.55rem; padding: 1rem; color: var(--muted-foreground); text-align: center; font-size: calc(0.72rem * var(--type-scale)); }
  .browser-panel > :global(.error) { border-bottom: 1px solid var(--border); padding: 0.45rem 0.65rem; font-size: calc(0.68rem * var(--type-scale)); }
  .approval { display: flex; flex-wrap: wrap; align-items: center; gap: 0.4rem 0.6rem; border-bottom: 1px solid var(--border); padding: 0.45rem 0.65rem; font-size: calc(0.68rem * var(--type-scale)); }
  .approval p { min-width: 12rem; flex: 1; }
  .approval div { display: flex; margin-left: auto; gap: 0.35rem; }
  .artifact-status { border-bottom: 1px solid var(--border); padding: 0.35rem 0.65rem; color: var(--muted-foreground); font-size: calc(0.66rem * var(--type-scale)); }
</style>
