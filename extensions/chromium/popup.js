const connectionEl = document.getElementById("connection");
const modeEl = document.getElementById("mode");
const remainingEl = document.getElementById("remaining");
const lastBlockedEl = document.getElementById("last-blocked");
const reasonEl = document.getElementById("reason");
const recheckButton = document.getElementById("recheck");

const FALLBACK_STATUS = {
  connected: false,
  active: false,
  phase: "inactive",
};

function phaseLabel(phase) {
  if (phase === "focus") return "Focus";
  if (phase === "short_break") return "Short break";
  if (phase === "long_break") return "Long break";
  return "Inactive";
}

function render(state) {
  connectionEl.textContent = state.connected ? "Connected" : "Disconnected";
  modeEl.textContent = state.active ? phaseLabel(state.phase) : "Inactive";
  remainingEl.textContent = typeof state.remainingSeconds === "number"
    ? `${Math.ceil(state.remainingSeconds / 60)} min`
    : "-";
  lastBlockedEl.textContent = state.lastBlockedHost ?? "-";
  reasonEl.textContent = state.reason ?? "";
}

function requestStatus(type) {
  return new Promise((resolve) => {
    chrome.runtime.sendMessage({ type }, (response) => {
      if (chrome.runtime.lastError) {
        resolve(null);
        return;
      }
      resolve(response ?? null);
    });
  });
}

async function renderStoredStatus() {
  const stored = await chrome.storage.local.get("status");
  render(stored.status ?? FALLBACK_STATUS);
}

async function refreshStatus(type = "get_status") {
  const fresh = await requestStatus(type);
  if (fresh) {
    render(fresh);
    return;
  }
  await renderStoredStatus();
}

recheckButton.addEventListener("click", async () => {
  recheckButton.disabled = true;
  recheckButton.textContent = "Rechecking";
  await refreshStatus("force_recheck");
  recheckButton.textContent = "Recheck now";
  recheckButton.disabled = false;
});

await renderStoredStatus();
await refreshStatus();
