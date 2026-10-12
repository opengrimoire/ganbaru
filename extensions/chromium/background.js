import { HOST_NAME } from "./host-config.js";

const STATUS_STORAGE_KEY = "status";
const BLOCKED_PAGE_STORAGE_PREFIX = "blockedPage:";
const ACTIVE_USAGE_STORAGE_KEY = "activeUsage";
const USAGE_IDLE_THRESHOLD_SECONDS = 60;
const BLOCKED_PAGE_RELEASE_CONFIRMATION_MS = 3000;
const recentRedirects = new Map();
let lastEnforcementStateKey = "";

function sendNativeMessage(message) {
  return new Promise((resolve) => {
    chrome.runtime.sendNativeMessage(HOST_NAME, message, (response) => {
      const error = chrome.runtime.lastError;
      if (error) {
        resolve({
          type: "error",
          connected: false,
          active: false,
          paused: false,
          pauseReason: null,
          blocked: false,
          reason: error.message,
        });
        return;
      }
      resolve(response);
    });
  });
}

function isSupportedPageUrl(url) {
  return typeof url === "string" && (url.startsWith("http://") || url.startsWith("https://"));
}

function delay(ms) {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

function hostFromUrl(url) {
  try {
    return new URL(url).hostname.toLowerCase();
  } catch {
    return null;
  }
}

function isActiveEnforcementPhase(status) {
  return status?.active === true
    && (
      status?.phase === "focus" ||
      status?.phase === "short_break" ||
      status?.phase === "long_break"
    );
}

function activeUsageStore() {
  return chrome.storage.session ?? chrome.storage.local;
}

async function readActiveUsage() {
  const stored = await activeUsageStore().get(ACTIVE_USAGE_STORAGE_KEY);
  const usage = stored[ACTIVE_USAGE_STORAGE_KEY];
  if (!usage || typeof usage !== "object") return null;
  if (typeof usage.host !== "string" || typeof usage.startedAtMs !== "number") return null;
  return {
    tabId: typeof usage.tabId === "number" ? usage.tabId : null,
    host: usage.host,
    startedAtMs: usage.startedAtMs,
  };
}

async function writeActiveUsage(usage) {
  await activeUsageStore().set({ [ACTIVE_USAGE_STORAGE_KEY]: usage });
}

async function clearActiveUsage() {
  await activeUsageStore().remove(ACTIVE_USAGE_STORAGE_KEY);
}

async function updateStatus(status) {
  const nextEnforcementStateKey = [
    status?.active === true ? "1" : "0",
    status?.paused === true ? "1" : "0",
    typeof status?.pauseReason === "string" ? status.pauseReason : "none",
    typeof status?.phase === "string" ? status.phase : "inactive",
    typeof status?.rulesFingerprint === "string" ? status.rulesFingerprint : "unknown",
  ].join("|");
  const shouldRecheckOpenTabs = nextEnforcementStateKey !== lastEnforcementStateKey
    && (status?.connected === true || isActiveEnforcementPhase(status));
  lastEnforcementStateKey = nextEnforcementStateKey;

  await chrome.storage.local.set({
    [STATUS_STORAGE_KEY]: {
      connected: status?.connected === true,
      active: status?.active === true,
      paused: status?.paused === true,
      pauseReason: typeof status?.pauseReason === "string" ? status.pauseReason : null,
      phase: typeof status?.phase === "string" ? status.phase : "inactive",
      remainingSeconds: typeof status?.remainingSeconds === "number" ? status.remainingSeconds : null,
      reason: typeof status?.reason === "string" ? status.reason : null,
      rulesFingerprint: typeof status?.rulesFingerprint === "string" ? status.rulesFingerprint : null,
      lastBlockedHost: typeof status?.lastBlockedHost === "string" ? status.lastBlockedHost : null,
      matchedRuleName: typeof status?.matchedRuleName === "string" ? status.matchedRuleName : null,
      updatedAt: new Date().toISOString(),
    },
  });
  if (shouldRecheckOpenTabs) {
    void recheckOpenTabs();
    void recheckBlockedPages();
  }
}

async function forceRecheckStatus() {
  await refreshActiveUsage({ forceFlush: true });
  const state = await sendNativeMessage({ type: "get_state" });
  await updateStatus(state);
  await Promise.all([
    recheckOpenTabs(),
    recheckBlockedPages(),
  ]);
  return state;
}

function localDateString(timestampMs) {
  const date = new Date(timestampMs);
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function queryIdleState() {
  return new Promise((resolve) => {
    if (!chrome.idle?.queryState) {
      resolve("active");
      return;
    }
    chrome.idle.queryState(USAGE_IDLE_THRESHOLD_SECONDS, (state) => {
      resolve(chrome.runtime.lastError ? "active" : state);
    });
  });
}

async function queryFocusedActiveTab() {
  const windows = await chrome.windows.getAll({ populate: false, windowTypes: ["normal"] });
  const focused = windows.find((window) => window.focused);
  if (!focused || typeof focused.id !== "number") return null;
  const tabs = await chrome.tabs.query({ active: true, windowId: focused.id });
  return tabs[0] ?? null;
}

async function queryTrackableFocusedTab() {
  const idleState = await queryIdleState();
  if (idleState === "locked") return null;
  const tab = await queryFocusedActiveTab();
  if (typeof tab?.url !== "string" || !isSupportedPageUrl(tab.url)) return null;
  const host = hostFromUrl(tab.url);
  if (!host) return null;
  return {
    tabId: typeof tab.id === "number" ? tab.id : null,
    host,
  };
}

async function recordUsageInterval(usage, endedAt) {
  if (!usage) return;
  const elapsedSeconds = Math.floor((endedAt - usage.startedAtMs) / 1000);
  if (elapsedSeconds > 0) {
    await sendNativeMessage({
      type: "record_usage",
      sourceType: "website",
      sourceKey: usage.host,
      displayName: usage.host,
      elapsedSeconds,
      startedAtMs: usage.startedAtMs,
      localDate: localDateString(usage.startedAtMs),
    });
  }
}

async function refreshActiveUsage({ forceFlush = false } = {}) {
  const now = Date.now();
  const usage = await readActiveUsage();
  const target = await queryTrackableFocusedTab();

  if (!usage) {
    if (target) await writeActiveUsage({ ...target, startedAtMs: now });
    return;
  }

  const sameHost = target?.host === usage.host;
  if (forceFlush || !sameHost) {
    await recordUsageInterval(usage, now);
    if (target) {
      await writeActiveUsage({ ...target, startedAtMs: now });
    } else {
      await clearActiveUsage();
    }
    return;
  }
}

function blockedPageStore() {
  return chrome.storage.session ?? chrome.storage.local;
}

async function storeBlockedPageState(originalUrl, decision) {
  const host = decision.host ?? hostFromUrl(originalUrl);
  const id = `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  await blockedPageStore().set({
    [`${BLOCKED_PAGE_STORAGE_PREFIX}${id}`]: {
      host,
      originalUrl,
      remainingSeconds: typeof decision.remainingSeconds === "number"
        ? decision.remainingSeconds
        : null,
      matchedRuleName: typeof decision.matchedRuleName === "string" ? decision.matchedRuleName : null,
      createdAt: Date.now(),
    },
  });
  return id;
}

async function readBlockedPageState(blockedPageId) {
  if (typeof blockedPageId !== "string" || !blockedPageId) return null;
  const key = `${BLOCKED_PAGE_STORAGE_PREFIX}${blockedPageId}`;
  const stored = await blockedPageStore().get(key);
  const value = stored[key];
  if (!value || typeof value !== "object") return null;
  return { key, value };
}

async function clearBlockedPageState(blockedPageId) {
  if (typeof blockedPageId !== "string" || !blockedPageId) return { ok: false };
  await blockedPageStore().remove(`${BLOCKED_PAGE_STORAGE_PREFIX}${blockedPageId}`);
  return { ok: true };
}

async function refreshBlockedPage(blockedPageId) {
  const stored = await readBlockedPageState(blockedPageId);
  if (!stored) {
    return { ok: false, blocked: false, reason: "blocked page state not found" };
  }

  const originalUrl = typeof stored.value.originalUrl === "string" ? stored.value.originalUrl : "";
  const host = typeof stored.value.host === "string" ? stored.value.host : hostFromUrl(originalUrl);
  if (!originalUrl || !host) {
    return { ok: false, blocked: false, reason: "blocked page state is incomplete" };
  }

  const decision = await sendNativeMessage({
    type: "decide_url",
    url: originalUrl,
    host,
    logEvent: false,
  });
  await updateStatus({
    ...decision,
    lastBlockedHost: decision?.blocked ? host : null,
  });

  const remainingSeconds = typeof decision?.remainingSeconds === "number"
    ? decision.remainingSeconds
    : null;
  const blocked = decision?.blocked === true;
  const now = Date.now();
  const previousUnblockedSince = typeof stored.value.unblockedSince === "number"
    ? stored.value.unblockedSince
    : null;
  const unblockedSince = blocked ? null : previousUnblockedSince ?? now;
  const unblockedStableMs = unblockedSince === null ? 0 : now - unblockedSince;
  const nextState = {
    ...stored.value,
    host,
    remainingSeconds,
    matchedRuleName: typeof decision?.matchedRuleName === "string" ? decision.matchedRuleName : null,
    blocked,
    active: decision?.active === true,
    paused: decision?.paused === true,
    pauseReason: typeof decision?.pauseReason === "string" ? decision.pauseReason : null,
    phase: typeof decision?.phase === "string" ? decision.phase : "inactive",
    unblockedSince,
    updatedAt: now,
  };
  await blockedPageStore().set({ [stored.key]: nextState });

  return {
    ok: true,
    blocked,
    active: decision?.active === true,
    paused: decision?.paused === true,
    pauseReason: typeof decision?.pauseReason === "string" ? decision.pauseReason : null,
    phase: typeof decision?.phase === "string" ? decision.phase : "inactive",
    remainingSeconds,
    matchedRuleName: typeof decision?.matchedRuleName === "string" ? decision.matchedRuleName : null,
    unblockedStableMs,
    host,
    originalUrl,
  };
}

async function confirmBlockedPageRelease(blockedPageId) {
  let state = await refreshBlockedPage(blockedPageId);
  while (
    state?.ok === true &&
    state.blocked === false &&
    typeof state.originalUrl === "string" &&
    typeof state.unblockedStableMs === "number" &&
    state.unblockedStableMs < BLOCKED_PAGE_RELEASE_CONFIRMATION_MS
  ) {
    await delay(BLOCKED_PAGE_RELEASE_CONFIRMATION_MS - state.unblockedStableMs);
    state = await refreshBlockedPage(blockedPageId);
  }

  return state;
}

function blockedPageUrl(blockedPageId) {
  const params = new URLSearchParams();
  params.set("blocked", blockedPageId);
  return chrome.runtime.getURL(`blocked.html?${params.toString()}`);
}

async function decideAndRedirect(tabId, url) {
  if (!isSupportedPageUrl(url)) return;
  const host = hostFromUrl(url);
  if (!host) return;
  const redirectKey = `${tabId}:${url}`;
  const lastRedirectAtMs = recentRedirects.get(redirectKey);
  if (typeof lastRedirectAtMs === "number" && Date.now() - lastRedirectAtMs < 3000) return;

  const decision = await sendNativeMessage({ type: "decide_url", url, host });
  await updateStatus({
    ...decision,
    lastBlockedHost: decision?.blocked ? host : null,
  });

  if (!decision || decision.blocked !== true) return;
  recentRedirects.set(redirectKey, Date.now());
  if (recentRedirects.size > 100) {
    const cutoff = Date.now() - 10_000;
    for (const [key, value] of recentRedirects.entries()) {
      if (value < cutoff) recentRedirects.delete(key);
    }
  }
  const blockedPageId = await storeBlockedPageState(url, decision);
  await chrome.tabs.update(tabId, { url: blockedPageUrl(blockedPageId) });
}

async function recheckOpenTabs() {
  const tabs = await chrome.tabs.query({ url: ["http://*/*", "https://*/*"] });
  for (const tab of tabs) {
    if (typeof tab.id !== "number" || typeof tab.url !== "string") continue;
    void decideAndRedirect(tab.id, tab.url);
  }
}

function blockedPageIdFromUrl(url) {
  if (typeof url !== "string") return null;
  try {
    const pageUrl = new URL(url);
    if (pageUrl.href.startsWith(chrome.runtime.getURL("blocked.html")) === false) return null;
    return pageUrl.searchParams.get("blocked");
  } catch {
    return null;
  }
}

async function recheckBlockedPages() {
  const tabs = await chrome.tabs.query({ url: chrome.runtime.getURL("blocked.html*") });
  for (const tab of tabs) {
    if (typeof tab.id !== "number" || typeof tab.url !== "string") continue;
    const blockedPageId = blockedPageIdFromUrl(tab.url);
    if (!blockedPageId) continue;
    void confirmBlockedPageRelease(blockedPageId).then(async (state) => {
      if (state?.ok !== true || state.blocked !== false || typeof state.originalUrl !== "string") {
        return;
      }
      await clearBlockedPageState(blockedPageId);
      await chrome.tabs.update(tab.id, { url: state.originalUrl });
    });
  }
}

chrome.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
  const url = changeInfo.url ?? tab.url;
  if (!url) return;
  void refreshActiveUsage();
  void decideAndRedirect(tabId, url);
});

chrome.webNavigation.onCommitted.addListener((details) => {
  if (details.frameId !== 0) return;
  void refreshActiveUsage();
  void decideAndRedirect(details.tabId, details.url);
});

chrome.tabs.onActivated.addListener(() => {
  void refreshActiveUsage();
});

chrome.windows.onFocusChanged.addListener(() => {
  void refreshActiveUsage();
});

if (chrome.idle?.onStateChanged) {
  chrome.idle.onStateChanged.addListener(() => {
    void refreshActiveUsage();
  });
}

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (!message || typeof message !== "object") return false;

  if (message.type === "get_status") {
    sendNativeMessage({ type: "get_state" })
      .then(async (state) => {
        await updateStatus(state);
        sendResponse(state);
      });
    return true;
  }

  if (message.type === "force_recheck") {
    forceRecheckStatus()
      .then((state) => sendResponse(state));
    return true;
  }

  if (message.type === "refresh_blocked_page") {
    refreshBlockedPage(message.id)
      .then((state) => sendResponse(state));
    return true;
  }

  if (message.type === "clear_blocked_page") {
    clearBlockedPageState(message.id)
      .then((state) => sendResponse(state));
    return true;
  }

  if (message.type === "close_tab") {
    if (sender.tab?.id) {
      chrome.tabs.remove(sender.tab.id);
      sendResponse({ ok: true });
      return false;
    }
  }

  return false;
});

void sendNativeMessage({ type: "get_state" }).then(updateStatus);
void refreshActiveUsage();
chrome.alarms.create("ganbaru-ai-state-poll", { periodInMinutes: 0.5 });
chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name !== "ganbaru-ai-state-poll") return;
  void refreshActiveUsage({ forceFlush: true });
  void sendNativeMessage({ type: "get_state" }).then(updateStatus);
});
