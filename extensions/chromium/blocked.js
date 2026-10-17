const params = new URLSearchParams(location.search);
const blockedPageId = params.get("blocked") ?? "";

const titleEl = document.getElementById("blocked-title");
const copyEl = document.querySelector(".blocked-copy");
const remainingEl = document.getElementById("remaining");
const actionButton = document.getElementById("close");
const recheckButton = document.getElementById("recheck");
const faviconEl = document.getElementById("favicon");

const RELEASE_CONFIRMATION_MS = 3000;
let latestOriginalUrl = "";
let latestBlocked = true;
let refreshTimeoutId = null;
let releaseTimeoutId = null;
let isRestoringOriginalUrl = false;

titleEl.textContent = "This site is blocked";
copyEl.textContent = "Stay strong and keep moving forward.";
remainingEl.textContent = "Focus session active";

function faviconUrl(siteHost) {
  const pageUrl = `https://${siteHost}/`;
  const encoded = encodeURIComponent(pageUrl);
  return chrome.runtime.getURL(`/_favicon/?pageUrl=${encoded}&size=64`);
}

function displayHost(siteHost) {
  return siteHost.replace(/^www\./i, "");
}

function requestBlockedPageState() {
  return new Promise((resolve) => {
    chrome.runtime.sendMessage({ type: "refresh_blocked_page", id: blockedPageId }, (response) => {
      if (chrome.runtime.lastError) {
        resolve(null);
        return;
      }
      resolve(response ?? null);
    });
  });
}

function requestForceRecheck() {
  return new Promise((resolve) => {
    chrome.runtime.sendMessage({ type: "force_recheck" }, (response) => {
      if (chrome.runtime.lastError) {
        resolve(null);
        return;
      }
      resolve(response ?? null);
    });
  });
}

function renderBlockedState(state) {
  const host = typeof state?.host === "string" ? state.host : "";
  const remaining = typeof state?.remainingSeconds === "number" ? state.remainingSeconds : 0;
  const matchedRuleName = typeof state?.matchedRuleName === "string" ? state.matchedRuleName : "";
  const blockedByDailyLimit = matchedRuleName.startsWith("daily limit:");
  const blockedByWeeklyLimit = matchedRuleName.startsWith("weekly limit:");
  const blockedByUsageLimit = blockedByDailyLimit || blockedByWeeklyLimit;
  latestOriginalUrl = typeof state?.originalUrl === "string" ? state.originalUrl : latestOriginalUrl;
  latestBlocked = state?.blocked !== false;

  if (latestBlocked) {
    cancelReleaseConfirmation();
    titleEl.textContent = blockedByDailyLimit
      ? `${host ? displayHost(host) : "This site"} reached today's limit`
      : blockedByWeeklyLimit
      ? `${host ? displayHost(host) : "This site"} reached this week's limit`
      : `${host ? displayHost(host) : "This site"} is blocked`;
    copyEl.textContent = blockedByUsageLimit
      ? "Come back after the limit resets."
      : "Stay strong and keep moving forward.";
    remainingEl.textContent = blockedByUsageLimit
      ? blockedByWeeklyLimit ? "Weekly limit reached" : "Daily limit reached"
      : Number.isFinite(remaining) && remaining > 0
      ? `${Math.ceil(remaining / 60)} min left in focus`
      : "Focus session active";
    actionButton.textContent = "Close tab";
  } else {
    const unblockedStableMs =
      typeof state?.unblockedStableMs === "number" ? state.unblockedStableMs : 0;
    scheduleReleaseConfirmation(unblockedStableMs);
  }

  if (host && !faviconEl.src) faviconEl.src = faviconUrl(host);
}

faviconEl.addEventListener("load", () => {
  faviconEl.hidden = false;
});
faviconEl.addEventListener("error", () => {
  faviconEl.hidden = true;
});

async function refreshBlockedPage() {
  if (refreshTimeoutId !== null) {
    clearTimeout(refreshTimeoutId);
    refreshTimeoutId = null;
  }
  const state = await requestBlockedPageState();
  if (state?.ok) renderBlockedState(state);
  if (state?.blocked === false) return;

  const delayMs = document.hidden ? 30_000 : 5_000;
  refreshTimeoutId = setTimeout(refreshBlockedPage, delayMs);
}

function resetRefreshTimer() {
  if (refreshTimeoutId !== null) clearTimeout(refreshTimeoutId);
  refreshTimeoutId = setTimeout(refreshBlockedPage, document.hidden ? 30_000 : 1_000);
}

function cancelReleaseConfirmation() {
  if (releaseTimeoutId === null) return;
  clearTimeout(releaseTimeoutId);
  releaseTimeoutId = null;
}

function scheduleReleaseConfirmation(unblockedStableMs = 0) {
  if (releaseTimeoutId !== null || isRestoringOriginalUrl) return;
  const delayMs = Math.max(0, RELEASE_CONFIRMATION_MS - unblockedStableMs);
  releaseTimeoutId = setTimeout(async () => {
    releaseTimeoutId = null;
    const state = await requestBlockedPageState();
    if (state?.ok !== true) return;
    const stableMs =
      typeof state.unblockedStableMs === "number" ? state.unblockedStableMs : 0;
    if (state.blocked === false && stableMs >= RELEASE_CONFIRMATION_MS) {
      latestOriginalUrl = typeof state.originalUrl === "string" ? state.originalUrl : latestOriginalUrl;
      restoreOriginalUrl();
      return;
    }
    renderBlockedState(state);
  }, delayMs);
}

function clearBlockedPageState() {
  if (!blockedPageId) return;
  chrome.runtime.sendMessage({ type: "clear_blocked_page", id: blockedPageId });
}

function restoreOriginalUrl() {
  if (isRestoringOriginalUrl || !latestOriginalUrl) return;
  isRestoringOriginalUrl = true;
  cancelReleaseConfirmation();
  clearBlockedPageState();
  chrome.tabs.getCurrent((tab) => {
    if (tab?.id) {
      chrome.tabs.update(tab.id, { url: latestOriginalUrl });
      return;
    }
    location.replace(latestOriginalUrl);
  });
}

document.addEventListener("visibilitychange", resetRefreshTimer);

actionButton.addEventListener("click", () => {
  chrome.tabs.getCurrent((tab) => {
    if (tab?.id) {
      if (!latestBlocked && latestOriginalUrl) {
        restoreOriginalUrl();
        return;
      }
      clearBlockedPageState();
      chrome.tabs.remove(tab.id);
      return;
    }
    chrome.runtime.sendMessage({ type: "close_tab" });
  });
});

recheckButton.addEventListener("click", async () => {
  recheckButton.disabled = true;
  recheckButton.textContent = "Rechecking";
  try {
    await requestForceRecheck();
    await refreshBlockedPage();
  } finally {
    recheckButton.textContent = "Recheck";
    recheckButton.disabled = false;
  }
});

await refreshBlockedPage();
