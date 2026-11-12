export interface ChatDiagnosticPreferences {
  captureEnabled: boolean;
  retentionDays: number;
}

export interface ChatDiagnosticCounts {
  retainedEvents: number;
  retainedBytes: number;
  attachmentCount: number;
  attachmentBytes: number;
  pendingAttachmentCleanup: number;
  failedAttachmentCleanup: number;
  commandOutputEvents: number;
  commandOutputBytes: number;
  checkpointFailures: number;
  pendingCheckpointCleanup: number;
  failedCheckpointCleanup: number;
}

export interface ChatDiagnosticsRead {
  preferences: ChatDiagnosticPreferences;
  capturedFields: string[];
  excludedFields: string[];
  storageLocation: string;
  projectionHealthy: boolean;
  inconsistentProjectionCount: number;
  credentialStoreAvailable: boolean;
  healthyProviderProbes: number;
  unhealthyProviderProbes: number;
  unknownProviderProbes: number;
  liveProviderProcesses: number;
  activeTurns: number;
  liveTerminals: number;
  counts: ChatDiagnosticCounts;
}

export interface ChatStopAllResult {
  providerProcessesStopped: number;
  terminalsStopped: number;
}

export interface ChatRebuildResult {
  rebuiltThreads: number;
}

export const CHAT_PROCESS_PURPOSES = ["session", "probe", "local_server", "terminal"] as const;
/** Why a listed Chat process runs. */
export type ChatProcessPurpose = (typeof CHAT_PROCESS_PURPOSES)[number];

/** Whether a Chat process runs in a Linux user resource scope with CPU, IO, task, and memory ceilings. */
export const CHAT_PROCESS_RESOURCE_SCOPES = ["scoped", "unavailable"] as const;

export type ChatProcessResourceScope = (typeof CHAT_PROCESS_RESOURCE_SCOPES)[number];

/** One process Chat started. */
export interface ChatProcessRecord {
  pid: number;
  /** Kernel start time that tells this process apart from a later one with the same ID; null where unsupported. */
  startTime: number | null;
  executableName: string;
  providerInstanceLabel: string | null;
  purpose: ChatProcessPurpose;
  startedAt: string;
  /** Whether this platform can stop the process individually. */
  stoppable: boolean;
  /** Resource scope of the process; null for processes that are never scoped, such as terminals and probes. */
  resourceScope: ChatProcessResourceScope | null;
}

/** A process started by a listed Chat process or one of its descendants. */
export interface ChatProcessDescendantRead {
  pid: number;
  parentPid: number;
  rootPid: number;
  executableName: string;
}

/** Running Chat processes, their bounded descendants, and how many descendants were left out. */
export interface ChatProcessListRead {
  roots: ChatProcessRecord[];
  descendants: ChatProcessDescendantRead[];
  omitted: number;
}
