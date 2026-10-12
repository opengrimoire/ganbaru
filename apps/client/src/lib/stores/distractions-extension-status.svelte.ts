import {
  getDistractionsExtensionStatus,
  type DistractionsExtensionStatus,
} from "$lib/api/distractions";
import {
  createLifecycleScheduler,
  type SchedulerRunContext,
} from "$lib/scheduling/lifecycle-scheduler";
import { appSessionStartedAt } from "$lib/stores/app-session";

const EXTENSION_STATUS_POLL_MS = 15_000;

let status = $state<DistractionsExtensionStatus | null>(null);
let loading = $state(true);
let error = $state<string | null>(null);
let refreshPromise: Promise<DistractionsExtensionStatus> | null = null;
let subscriberCount = 0;

async function refreshExtensionStatus(context?: SchedulerRunContext): Promise<void> {
  if (!status) loading = true;
  const request = refreshPromise
    ?? getDistractionsExtensionStatus(appSessionStartedAt).finally(() => {
      refreshPromise = null;
    });
  refreshPromise = request;
  try {
    const nextStatus = await request;
    if (context && !context.isCurrent()) return;
    status = nextStatus;
    error = null;
  } catch (err) {
    if (context && !context.isCurrent()) return;
    console.warn("Failed to read browser extension connection status:", err);
    error = err instanceof Error ? err.message : String(err);
  } finally {
    if (!context || context.isCurrent()) loading = false;
  }
}

const extensionStatusScheduler = createLifecycleScheduler({
  run: async (context) => {
    await refreshExtensionStatus(context);
    return context.isCurrent() ? context.now() + EXTENSION_STATUS_POLL_MS : null;
  },
  errorRetryMs: 60_000,
  onError: (schedulerError) => {
    console.warn("Failed to schedule browser extension connection status:", schedulerError);
  },
});

function startExtensionStatusPolling(): () => void {
  subscriberCount += 1;
  extensionStatusScheduler.setEnabled(true);
  return () => {
    subscriberCount = Math.max(0, subscriberCount - 1);
    if (subscriberCount > 0) return;
    extensionStatusScheduler.setEnabled(false);
  };
}

export function getDistractionsExtensionConnection() {
  return {
    get status(): DistractionsExtensionStatus | null {
      return status;
    },
    get loading(): boolean {
      return loading;
    },
    get error(): string | null {
      return error;
    },
    refresh(): Promise<void> {
      return refreshExtensionStatus();
    },
    start(): () => void {
      return startExtensionStatusPolling();
    },
    resume(): void {
      extensionStatusScheduler.resume();
    },
  };
}
