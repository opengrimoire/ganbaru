import {
  ACTIVITY_STATUSES,
  APPROVAL_DECISION_KINDS,
  CANONICAL_ITEM_KINDS,
  CANONICAL_REQUEST_KINDS,
  CHAT_THREAD_STATES,
  CHAT_TURN_STATES,
  CONTENT_STREAM_KINDS,
  PROVIDER_SESSION_STATES,
  REQUEST_RESOLUTION_STATES,
  type AccountStatusEvent,
  type ApprovalDecision,
  type ApprovalDecisionOption,
  type AuthenticationStatusEvent,
  type CanonicalEvent,
  type CanonicalRuntimeEvent,
  type CanonicalStoredEvent,
  type ChangedFileSummary,
  type ContentDeltaEvent,
  type DiffUpdatedEvent,
  type FilesPersistedEvent,
  type HookLifecycleEvent,
  type ItemLifecycleEvent,
  type McpOauthCompletedEvent,
  type McpStatusEvent,
  type ModelReroutedEvent,
  type NotificationEvent,
  type PlanStep,
  type PlanUpdatedEvent,
  type ProviderAttributedCost,
  type RateLimitStatusEvent,
  type RequestOpenedEvent,
  type RequestResolvedEvent,
  type RuntimeErrorEvent,
  type SessionConfiguredEvent,
  type SessionExitedEvent,
  type SessionStartedEvent,
  type SessionStateChangedEvent,
  type TaskLifecycleEvent,
  type ThreadMetadataUpdatedEvent,
  type ThreadRevertedEvent,
  type ThreadStartedEvent,
  type ThreadStateChangedEvent,
  type ThreadUsageUpdatedEvent,
  type ToolProgressEvent,
  type TurnAbortedEvent,
  type TurnCompletedEvent,
  type TurnStartedEvent,
  type UnknownEvent,
  type UserInputAnswer,
  type UserInputOption,
  type UserInputQuestion,
  type UserInputRequestedEvent,
  type UserInputResolvedEvent,
} from "../contracts";
import { normalizeChangedFileSummaries } from "$lib/chat/review/changed-files";
import { parseModelOptionSelection, parseProviderCapabilities } from "./provider";
import {
  readArray,
  readBoolean,
  readEnum,
  readFiniteNumber,
  readIdentifier,
  readNullable,
  readNonNegativeSafeInteger,
  readRecord,
  readSafeInteger,
  readString,
  readStringArray,
  readTurnModeSnapshot,
  readUtcTimestamp,
  readVersionedJson,
} from "./readers";

function parseApprovalDecision(value: unknown, label: string): ApprovalDecision {
  const record = readRecord(value, label);
  return {
    kind: readEnum(record.kind, APPROVAL_DECISION_KINDS, `${label}.kind`),
    providerOptionId: readNullable(record.providerOptionId, `${label}.providerOptionId`, readString),
    updatedToolInput: readNullable(record.updatedToolInput, `${label}.updatedToolInput`, readVersionedJson),
  };
}

function parseUserInputAnswer(value: unknown, label: string): UserInputAnswer {
  const record = readRecord(value, label);
  return {
    questionId: readString(record.questionId, `${label}.questionId`),
    selectedOptionIds: readStringArray(record.selectedOptionIds, `${label}.selectedOptionIds`),
    freeFormText: readNullable(record.freeFormText, `${label}.freeFormText`, readString),
  };
}

function parseSessionStarted(value: unknown, label: string): SessionStartedEvent {
  const record = readRecord(value, label);
  return {
    sessionId: readIdentifier(record.sessionId, `${label}.sessionId`),
    state: readEnum(record.state, PROVIDER_SESSION_STATES, `${label}.state`),
    providerThreadId: readNullable(record.providerThreadId, `${label}.providerThreadId`, readIdentifier),
    resumeCursor: readNullable(record.resumeCursor, `${label}.resumeCursor`, readVersionedJson),
    effectiveModes: readTurnModeSnapshot(record.effectiveModes, `${label}.effectiveModes`),
    capabilityOverrides: parseProviderCapabilities(record.capabilityOverrides, `${label}.capabilityOverrides`),
  };
}

function parseSessionConfigured(value: unknown, label: string): SessionConfiguredEvent {
  const record = readRecord(value, label);
  return {
    sessionId: readIdentifier(record.sessionId, `${label}.sessionId`),
    effectiveModes: readTurnModeSnapshot(record.effectiveModes, `${label}.effectiveModes`),
    effectiveModelId: readNullable(record.effectiveModelId, `${label}.effectiveModelId`, readIdentifier),
    effectiveModelOptions: readArray(
      record.effectiveModelOptions,
      `${label}.effectiveModelOptions`,
      parseModelOptionSelection,
    ),
  };
}

function parseSessionStateChanged(value: unknown, label: string): SessionStateChangedEvent {
  const record = readRecord(value, label);
  return {
    sessionId: readIdentifier(record.sessionId, `${label}.sessionId`),
    previousState: readEnum(record.previousState, PROVIDER_SESSION_STATES, `${label}.previousState`),
    state: readEnum(record.state, PROVIDER_SESSION_STATES, `${label}.state`),
    reason: readNullable(record.reason, `${label}.reason`, readString),
  };
}

function parseSessionExited(value: unknown, label: string): SessionExitedEvent {
  const record = readRecord(value, label);
  return {
    sessionId: readIdentifier(record.sessionId, `${label}.sessionId`),
    expected: readBoolean(record.expected, `${label}.expected`),
    exitCode: readNullable(record.exitCode, `${label}.exitCode`, readSafeInteger),
    reason: readNullable(record.reason, `${label}.reason`, readString),
  };
}

function parseThreadStarted(value: unknown, label: string): ThreadStartedEvent {
  const record = readRecord(value, label);
  return {
    providerThreadId: readIdentifier(record.providerThreadId, `${label}.providerThreadId`),
    title: readNullable(record.title, `${label}.title`, readString),
  };
}

function parseThreadStateChanged(value: unknown, label: string): ThreadStateChangedEvent {
  const record = readRecord(value, label);
  return {
    previousState: readEnum(record.previousState, CHAT_THREAD_STATES, `${label}.previousState`),
    state: readEnum(record.state, CHAT_THREAD_STATES, `${label}.state`),
    reason: readNullable(record.reason, `${label}.reason`, readString),
  };
}

function parseThreadMetadataUpdated(value: unknown, label: string): ThreadMetadataUpdatedEvent {
  const record = readRecord(value, label);
  return {
    title: readNullable(record.title, `${label}.title`, readString),
    providerThreadId: readNullable(record.providerThreadId, `${label}.providerThreadId`, readIdentifier),
    resumeCursor: readNullable(record.resumeCursor, `${label}.resumeCursor`, readVersionedJson),
    metadata: readNullable(record.metadata, `${label}.metadata`, readVersionedJson),
  };
}

function parseThreadReverted(value: unknown, label: string): ThreadRevertedEvent {
  const record = readRecord(value, label);
  return {
    checkpointId: readIdentifier(record.checkpointId, `${label}.checkpointId`),
    revertedTurnIds: readArray(record.revertedTurnIds, `${label}.revertedTurnIds`, readIdentifier),
    providerHistoryAction: readEnum(
      record.providerHistoryAction,
      ["rolled_back", "fork_required"] as const,
      `${label}.providerHistoryAction`,
    ),
  };
}

function parseProviderCost(value: unknown, label: string): ProviderAttributedCost {
  const record = readRecord(value, label);
  return {
    amount: readFiniteNumber(record.amount, `${label}.amount`),
    currency: readString(record.currency, `${label}.currency`),
    providerReported: readBoolean(record.providerReported, `${label}.providerReported`),
  };
}

export function parseThreadUsage(value: unknown, label: string): ThreadUsageUpdatedEvent {
  const record = readRecord(value, label);
  return {
    inputTokens: readNullable(record.inputTokens, `${label}.inputTokens`, readNonNegativeSafeInteger),
    outputTokens: readNullable(record.outputTokens, `${label}.outputTokens`, readNonNegativeSafeInteger),
    cachedInputTokens: readNullable(record.cachedInputTokens, `${label}.cachedInputTokens`, readNonNegativeSafeInteger),
    contextTokens: readNullable(record.contextTokens, `${label}.contextTokens`, readNonNegativeSafeInteger),
    contextLimit: readNullable(record.contextLimit, `${label}.contextLimit`, readNonNegativeSafeInteger),
    cost: readNullable(record.cost, `${label}.cost`, parseProviderCost),
  };
}

function parseTurnStarted(value: unknown, label: string): TurnStartedEvent {
  const record = readRecord(value, label);
  return {
    providerTurnId: readNullable(record.providerTurnId, `${label}.providerTurnId`, readIdentifier),
    state: readEnum(record.state, CHAT_TURN_STATES, `${label}.state`),
    modes: readTurnModeSnapshot(record.modes, `${label}.modes`),
    modelId: readNullable(record.modelId, `${label}.modelId`, readIdentifier),
    modelOptions: readArray(record.modelOptions, `${label}.modelOptions`, parseModelOptionSelection),
  };
}

export function parseChangedFile(value: unknown, label: string): ChangedFileSummary {
  const record = readRecord(value, label);
  return {
    relativePath: readString(record.relativePath, `${label}.relativePath`),
    previousRelativePath: readNullable(record.previousRelativePath, `${label}.previousRelativePath`, readString),
    additions: readNullable(record.additions, `${label}.additions`, readNonNegativeSafeInteger),
    deletions: readNullable(record.deletions, `${label}.deletions`, readNonNegativeSafeInteger),
    binary: readBoolean(record.binary, `${label}.binary`),
    status: readString(record.status, `${label}.status`),
  };
}

function parseTurnCompleted(value: unknown, label: string): TurnCompletedEvent {
  const record = readRecord(value, label);
  return {
    state: readEnum(record.state, CHAT_TURN_STATES, `${label}.state`),
    stopReason: readNullable(record.stopReason, `${label}.stopReason`, readString),
    usage: readNullable(record.usage, `${label}.usage`, parseThreadUsage),
    changedFiles: normalizeChangedFileSummaries(
      readArray(record.changedFiles, `${label}.changedFiles`, parseChangedFile),
    ),
  };
}

function parseTurnAborted(value: unknown, label: string): TurnAbortedEvent {
  const record = readRecord(value, label);
  return {
    state: readEnum(record.state, CHAT_TURN_STATES, `${label}.state`),
    reason: readString(record.reason, `${label}.reason`),
    recoverable: readBoolean(record.recoverable, `${label}.recoverable`),
  };
}

function parsePlanStep(value: unknown, label: string): PlanStep {
  const record = readRecord(value, label);
  return {
    id: readString(record.id, `${label}.id`),
    text: readString(record.text, `${label}.text`),
    status: readEnum(record.status, ACTIVITY_STATUSES, `${label}.status`),
  };
}

function parsePlanUpdated(value: unknown, label: string): PlanUpdatedEvent {
  const record = readRecord(value, label);
  return {
    markdown: readString(record.markdown, `${label}.markdown`),
    steps: readArray(record.steps, `${label}.steps`, parsePlanStep),
  };
}

function parseDiffUpdated(value: unknown, label: string): DiffUpdatedEvent {
  const record = readRecord(value, label);
  return {
    source: readString(record.source, `${label}.source`),
    files: normalizeChangedFileSummaries(
      readArray(record.files, `${label}.files`, parseChangedFile),
    ),
    providerDiff: readNullable(record.providerDiff, `${label}.providerDiff`, readString),
  };
}

function parseItemLifecycle(value: unknown, label: string): ItemLifecycleEvent {
  const record = readRecord(value, label);
  return {
    itemId: readString(record.itemId, `${label}.itemId`),
    kind: readEnum(record.kind, CANONICAL_ITEM_KINDS, `${label}.kind`),
    status: readEnum(record.status, ACTIVITY_STATUSES, `${label}.status`),
    title: readNullable(record.title, `${label}.title`, readString),
    detail: readNullable(record.detail, `${label}.detail`, readString),
    safeMetadata: readNullable(record.safeMetadata, `${label}.safeMetadata`, readVersionedJson),
  };
}

function parseContentDelta(value: unknown, label: string): ContentDeltaEvent {
  const record = readRecord(value, label);
  return {
    itemId: readString(record.itemId, `${label}.itemId`),
    streamKind: readEnum(record.streamKind, CONTENT_STREAM_KINDS, `${label}.streamKind`),
    contentIndex: readNonNegativeSafeInteger(record.contentIndex, `${label}.contentIndex`),
    delta: readString(record.delta, `${label}.delta`),
  };
}

function parseApprovalOption(value: unknown, label: string): ApprovalDecisionOption {
  const record = readRecord(value, label);
  return {
    id: readString(record.id, `${label}.id`),
    label: readString(record.label, `${label}.label`),
    decisionKind: readEnum(record.decisionKind, APPROVAL_DECISION_KINDS, `${label}.decisionKind`),
    description: readNullable(record.description, `${label}.description`, readString),
  };
}

function parseRequestOpened(value: unknown, label: string): RequestOpenedEvent {
  const record = readRecord(value, label);
  return {
    requestId: readIdentifier(record.requestId, `${label}.requestId`),
    kind: readEnum(record.kind, CANONICAL_REQUEST_KINDS, `${label}.kind`),
    title: readString(record.title, `${label}.title`),
    detail: readNullable(record.detail, `${label}.detail`, readString),
    allowedDecisions: readArray(record.allowedDecisions, `${label}.allowedDecisions`, parseApprovalOption),
    safePayload: readVersionedJson(record.safePayload, `${label}.safePayload`),
  };
}

function parseRequestResolved(value: unknown, label: string): RequestResolvedEvent {
  const record = readRecord(value, label);
  return {
    requestId: readIdentifier(record.requestId, `${label}.requestId`),
    state: readEnum(record.state, REQUEST_RESOLUTION_STATES, `${label}.state`),
    decision: readNullable(record.decision, `${label}.decision`, parseApprovalDecision),
  };
}

function parseUserInputOption(value: unknown, label: string): UserInputOption {
  const record = readRecord(value, label);
  return {
    id: readString(record.id, `${label}.id`),
    label: readString(record.label, `${label}.label`),
    description: readNullable(record.description, `${label}.description`, readString),
  };
}

function parseUserInputQuestion(value: unknown, label: string): UserInputQuestion {
  const record = readRecord(value, label);
  return {
    id: readString(record.id, `${label}.id`),
    header: readNullable(record.header, `${label}.header`, readString),
    question: readString(record.question, `${label}.question`),
    options: readArray(record.options, `${label}.options`, parseUserInputOption),
    multiple: readBoolean(record.multiple, `${label}.multiple`),
    freeFormAllowed: readBoolean(record.freeFormAllowed, `${label}.freeFormAllowed`),
    required: readBoolean(record.required, `${label}.required`),
  };
}

function parseUserInputRequested(value: unknown, label: string): UserInputRequestedEvent {
  const record = readRecord(value, label);
  return {
    requestId: readIdentifier(record.requestId, `${label}.requestId`),
    questions: readArray(record.questions, `${label}.questions`, parseUserInputQuestion),
  };
}

function parseUserInputResolved(value: unknown, label: string): UserInputResolvedEvent {
  const record = readRecord(value, label);
  return {
    requestId: readIdentifier(record.requestId, `${label}.requestId`),
    state: readEnum(record.state, REQUEST_RESOLUTION_STATES, `${label}.state`),
    answers: readArray(record.answers, `${label}.answers`, parseUserInputAnswer),
  };
}

function parseTaskLifecycle(value: unknown, label: string): TaskLifecycleEvent {
  const record = readRecord(value, label);
  return {
    taskId: readString(record.taskId, `${label}.taskId`),
    parentTaskId: readNullable(record.parentTaskId, `${label}.parentTaskId`, readString),
    status: readEnum(record.status, ACTIVITY_STATUSES, `${label}.status`),
    title: readString(record.title, `${label}.title`),
    detail: readNullable(record.detail, `${label}.detail`, readString),
    safeMetadata: readNullable(record.safeMetadata, `${label}.safeMetadata`, readVersionedJson),
  };
}

function parseHookLifecycle(value: unknown, label: string): HookLifecycleEvent {
  const record = readRecord(value, label);
  return {
    hookId: readString(record.hookId, `${label}.hookId`),
    status: readEnum(record.status, ACTIVITY_STATUSES, `${label}.status`),
    title: readString(record.title, `${label}.title`),
    detail: readNullable(record.detail, `${label}.detail`, readString),
  };
}

function parseToolProgress(value: unknown, label: string): ToolProgressEvent {
  const record = readRecord(value, label);
  return {
    toolId: readString(record.toolId, `${label}.toolId`),
    status: readEnum(record.status, ACTIVITY_STATUSES, `${label}.status`),
    title: readString(record.title, `${label}.title`),
    progress: readNullable(record.progress, `${label}.progress`, readFiniteNumber),
    summary: readNullable(record.summary, `${label}.summary`, readString),
  };
}

function parseAuthenticationStatus(value: unknown, label: string): AuthenticationStatusEvent {
  const record = readRecord(value, label);
  return {
    authenticated: readBoolean(record.authenticated, `${label}.authenticated`),
    accountLabel: readNullable(record.accountLabel, `${label}.accountLabel`, readString),
    actionRequired: readBoolean(record.actionRequired, `${label}.actionRequired`),
    detail: readNullable(record.detail, `${label}.detail`, readString),
  };
}

export function parseAccountStatus(value: unknown, label: string): AccountStatusEvent {
  const record = readRecord(value, label);
  return {
    accountLabel: readNullable(record.accountLabel, `${label}.accountLabel`, readString),
    planLabel: readNullable(record.planLabel, `${label}.planLabel`, readString),
    usage: readNullable(record.usage, `${label}.usage`, readVersionedJson),
  };
}

export function parseRateLimitStatus(value: unknown, label: string): RateLimitStatusEvent {
  const record = readRecord(value, label);
  return {
    limited: readBoolean(record.limited, `${label}.limited`),
    resetsAt: readNullable(record.resetsAt, `${label}.resetsAt`, readUtcTimestamp),
    detail: readNullable(record.detail, `${label}.detail`, readString),
    providerData: readNullable(record.providerData, `${label}.providerData`, readVersionedJson),
  };
}

function parseMcpStatus(value: unknown, label: string): McpStatusEvent {
  const record = readRecord(value, label);
  return {
    serverId: readString(record.serverId, `${label}.serverId`),
    status: readEnum(record.status, ACTIVITY_STATUSES, `${label}.status`),
    detail: readNullable(record.detail, `${label}.detail`, readString),
  };
}

function parseMcpOauthCompleted(value: unknown, label: string): McpOauthCompletedEvent {
  const record = readRecord(value, label);
  return {
    serverId: readString(record.serverId, `${label}.serverId`),
    successful: readBoolean(record.successful, `${label}.successful`),
    detail: readNullable(record.detail, `${label}.detail`, readString),
  };
}

function parseModelRerouted(value: unknown, label: string): ModelReroutedEvent {
  const record = readRecord(value, label);
  return {
    requestedModelId: readIdentifier(record.requestedModelId, `${label}.requestedModelId`),
    effectiveModelId: readIdentifier(record.effectiveModelId, `${label}.effectiveModelId`),
    reason: readNullable(record.reason, `${label}.reason`, readString),
  };
}

function parseNotification(value: unknown, label: string): NotificationEvent {
  const record = readRecord(value, label);
  return {
    code: readString(record.code, `${label}.code`),
    title: readString(record.title, `${label}.title`),
    detail: readNullable(record.detail, `${label}.detail`, readString),
  };
}

function parseFilesPersisted(value: unknown, label: string): FilesPersistedEvent {
  const record = readRecord(value, label);
  return { relativePaths: readStringArray(record.relativePaths, `${label}.relativePaths`) };
}

function parseRuntimeError(value: unknown, label: string): RuntimeErrorEvent {
  const record = readRecord(value, label);
  return {
    code: readString(record.code, `${label}.code`),
    message: readString(record.message, `${label}.message`),
    recoverable: readBoolean(record.recoverable, `${label}.recoverable`),
    safeDetails: readNullable(record.safeDetails, `${label}.safeDetails`, readVersionedJson),
  };
}

function parseUnknownEvent(value: unknown, label: string): UnknownEvent {
  const record = readRecord(value, label);
  return {
    sourceType: readString(record.sourceType, `${label}.sourceType`),
    summary: readString(record.summary, `${label}.summary`),
    safePayload: readNullable(record.safePayload, `${label}.safePayload`, readVersionedJson),
  };
}

export function parseCanonicalEvent(value: unknown, label = "canonical event"): CanonicalEvent {
  const record = readRecord(value, label);
  const type = readString(record.type, `${label}.type`);
  const payloadLabel = `${label}.payload`;
  switch (type) {
    case "session_started": return { type, payload: parseSessionStarted(record.payload, payloadLabel) };
    case "session_configured": return { type, payload: parseSessionConfigured(record.payload, payloadLabel) };
    case "session_state_changed": return { type, payload: parseSessionStateChanged(record.payload, payloadLabel) };
    case "session_exited": return { type, payload: parseSessionExited(record.payload, payloadLabel) };
    case "thread_started": return { type, payload: parseThreadStarted(record.payload, payloadLabel) };
    case "thread_state_changed": return { type, payload: parseThreadStateChanged(record.payload, payloadLabel) };
    case "thread_metadata_updated": return { type, payload: parseThreadMetadataUpdated(record.payload, payloadLabel) };
    case "thread_reverted": return { type, payload: parseThreadReverted(record.payload, payloadLabel) };
    case "thread_usage_updated": return { type, payload: parseThreadUsage(record.payload, payloadLabel) };
    case "turn_started": return { type, payload: parseTurnStarted(record.payload, payloadLabel) };
    case "turn_completed": return { type, payload: parseTurnCompleted(record.payload, payloadLabel) };
    case "turn_aborted": return { type, payload: parseTurnAborted(record.payload, payloadLabel) };
    case "plan_updated": return { type, payload: parsePlanUpdated(record.payload, payloadLabel) };
    case "proposed_plan_delta": {
      const payload = readRecord(record.payload, payloadLabel);
      return { type, payload: {
        planId: readString(payload.planId, `${payloadLabel}.planId`),
        delta: readString(payload.delta, `${payloadLabel}.delta`),
        contentIndex: readNonNegativeSafeInteger(payload.contentIndex, `${payloadLabel}.contentIndex`),
      } };
    }
    case "proposed_plan_completed": {
      const payload = readRecord(record.payload, payloadLabel);
      return { type, payload: {
        planId: readString(payload.planId, `${payloadLabel}.planId`),
        markdown: readString(payload.markdown, `${payloadLabel}.markdown`),
      } };
    }
    case "diff_updated": return { type, payload: parseDiffUpdated(record.payload, payloadLabel) };
    case "item_started": return { type, payload: parseItemLifecycle(record.payload, payloadLabel) };
    case "item_updated": return { type, payload: parseItemLifecycle(record.payload, payloadLabel) };
    case "item_completed": return { type, payload: parseItemLifecycle(record.payload, payloadLabel) };
    case "content_delta": return { type, payload: parseContentDelta(record.payload, payloadLabel) };
    case "request_opened": return { type, payload: parseRequestOpened(record.payload, payloadLabel) };
    case "request_resolved": return { type, payload: parseRequestResolved(record.payload, payloadLabel) };
    case "user_input_requested": return { type, payload: parseUserInputRequested(record.payload, payloadLabel) };
    case "user_input_resolved": return { type, payload: parseUserInputResolved(record.payload, payloadLabel) };
    case "task_lifecycle": return { type, payload: parseTaskLifecycle(record.payload, payloadLabel) };
    case "hook_lifecycle": return { type, payload: parseHookLifecycle(record.payload, payloadLabel) };
    case "tool_progress": return { type, payload: parseToolProgress(record.payload, payloadLabel) };
    case "authentication_status": return { type, payload: parseAuthenticationStatus(record.payload, payloadLabel) };
    case "account_status": return { type, payload: parseAccountStatus(record.payload, payloadLabel) };
    case "rate_limit_status": return { type, payload: parseRateLimitStatus(record.payload, payloadLabel) };
    case "mcp_status": return { type, payload: parseMcpStatus(record.payload, payloadLabel) };
    case "mcp_oauth_completed": return { type, payload: parseMcpOauthCompleted(record.payload, payloadLabel) };
    case "model_rerouted": return { type, payload: parseModelRerouted(record.payload, payloadLabel) };
    case "configuration_warning": return { type, payload: parseNotification(record.payload, payloadLabel) };
    case "deprecation_notice": return { type, payload: parseNotification(record.payload, payloadLabel) };
    case "files_persisted": return { type, payload: parseFilesPersisted(record.payload, payloadLabel) };
    case "runtime_warning": return { type, payload: parseNotification(record.payload, payloadLabel) };
    case "runtime_error": return { type, payload: parseRuntimeError(record.payload, payloadLabel) };
    case "unknown": return { type, payload: parseUnknownEvent(record.payload, payloadLabel) };
    default: throw new Error(`${label}.type has an unsupported value`);
  }
}

export function parseCanonicalRuntimeEvent(value: unknown, label = "canonical runtime event"): CanonicalRuntimeEvent {
  const record = readRecord(value, label);
  return {
    schemaVersion: readNonNegativeSafeInteger(record.schemaVersion, `${label}.schemaVersion`),
    eventId: readIdentifier(record.eventId, `${label}.eventId`),
    providerFamilyId: readIdentifier(record.providerFamilyId, `${label}.providerFamilyId`),
    providerInstanceId: readIdentifier(record.providerInstanceId, `${label}.providerInstanceId`),
    threadId: readIdentifier(record.threadId, `${label}.threadId`),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
    turnId: readNullable(record.turnId, `${label}.turnId`, readIdentifier),
    providerTurnId: readNullable(record.providerTurnId, `${label}.providerTurnId`, readIdentifier),
    providerItemId: readNullable(record.providerItemId, `${label}.providerItemId`, readIdentifier),
    providerRequestId: readNullable(record.providerRequestId, `${label}.providerRequestId`, readIdentifier),
    providerTaskId: readNullable(record.providerTaskId, `${label}.providerTaskId`, readIdentifier),
    providerReference: readNullable(record.providerReference, `${label}.providerReference`, readVersionedJson),
    event: parseCanonicalEvent(record.event, `${label}.event`),
    redactedDiagnostic: readNullable(record.redactedDiagnostic, `${label}.redactedDiagnostic`, readVersionedJson),
  };
}

export function parseCanonicalStoredEvent(value: unknown, label = "canonical stored event"): CanonicalStoredEvent {
  const record = readRecord(value, label);
  return {
    ...parseCanonicalRuntimeEvent(record, label),
    sequence: readNonNegativeSafeInteger(record.sequence, `${label}.sequence`),
    ingestedAt: readUtcTimestamp(record.ingestedAt, `${label}.ingestedAt`),
  };
}
