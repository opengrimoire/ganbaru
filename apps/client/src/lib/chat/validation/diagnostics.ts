import type {
  ChatDiagnosticCounts,
  ChatDiagnosticPreferences,
  ChatDiagnosticsRead,
  ChatRebuildResult,
  ChatStopAllResult,
} from "../contracts";
import { readArray, readBoolean, readNonNegativeSafeInteger, readRecord, readString } from "./readers";

export function parseChatDiagnosticPreferences(value: unknown, label = "diagnostic preferences"): ChatDiagnosticPreferences {
  const record = readRecord(value, label);
  const retentionDays = readNonNegativeSafeInteger(record.retentionDays, `${label}.retentionDays`);
  if (retentionDays < 1 || retentionDays > 30) throw new Error(`${label}.retentionDays must be between 1 and 30`);
  return { captureEnabled: readBoolean(record.captureEnabled, `${label}.captureEnabled`), retentionDays };
}

function parseCounts(value: unknown, label: string): ChatDiagnosticCounts {
  const record = readRecord(value, label);
  const count = (field: keyof ChatDiagnosticCounts): number => readNonNegativeSafeInteger(record[field], `${label}.${field}`);
  return {
    retainedEvents: count("retainedEvents"), retainedBytes: count("retainedBytes"),
    attachmentCount: count("attachmentCount"), attachmentBytes: count("attachmentBytes"),
    pendingAttachmentCleanup: count("pendingAttachmentCleanup"), failedAttachmentCleanup: count("failedAttachmentCleanup"),
    commandOutputEvents: count("commandOutputEvents"), commandOutputBytes: count("commandOutputBytes"),
    checkpointFailures: count("checkpointFailures"), pendingCheckpointCleanup: count("pendingCheckpointCleanup"),
    failedCheckpointCleanup: count("failedCheckpointCleanup"),
  };
}

export function parseChatDiagnosticsRead(value: unknown): ChatDiagnosticsRead {
  const record = readRecord(value, "Chat diagnostics");
  const count = (field: keyof ChatDiagnosticsRead): number => readNonNegativeSafeInteger(record[field], `Chat diagnostics.${field}`);
  return {
    preferences: parseChatDiagnosticPreferences(record.preferences),
    capturedFields: readArray(record.capturedFields, "Chat diagnostics.capturedFields", readString),
    excludedFields: readArray(record.excludedFields, "Chat diagnostics.excludedFields", readString),
    storageLocation: readString(record.storageLocation, "Chat diagnostics.storageLocation"),
    projectionHealthy: readBoolean(record.projectionHealthy, "Chat diagnostics.projectionHealthy"),
    inconsistentProjectionCount: count("inconsistentProjectionCount"),
    credentialStoreAvailable: readBoolean(record.credentialStoreAvailable, "Chat diagnostics.credentialStoreAvailable"),
    healthyProviderProbes: count("healthyProviderProbes"), unhealthyProviderProbes: count("unhealthyProviderProbes"),
    unknownProviderProbes: count("unknownProviderProbes"), liveProviderProcesses: count("liveProviderProcesses"),
    activeTurns: count("activeTurns"), liveTerminals: count("liveTerminals"),
    counts: parseCounts(record.counts, "Chat diagnostics.counts"),
  };
}

export function parseChatStopAllResult(value: unknown): ChatStopAllResult {
  const record = readRecord(value, "stop all Chat result");
  return {
    providerProcessesStopped: readNonNegativeSafeInteger(record.providerProcessesStopped, "stop all Chat result.providerProcessesStopped"),
    terminalsStopped: readNonNegativeSafeInteger(record.terminalsStopped, "stop all Chat result.terminalsStopped"),
  };
}

export function parseChatRebuildResult(value: unknown): ChatRebuildResult {
  const record = readRecord(value, "Chat rebuild result");
  return { rebuiltThreads: readNonNegativeSafeInteger(record.rebuiltThreads, "Chat rebuild result.rebuiltThreads") };
}
