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
