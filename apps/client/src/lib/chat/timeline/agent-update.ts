import {
  CHAT_ERROR_CODES,
  CHAT_WORK_ASSIGNMENT_STATES,
  type ChatErrorCode,
  type ChatMessageRead,
  type ChatPushRequest,
  type ChatScheduleProposal,
  type ChatWorkAssignmentRead,
  type ChatWorkAssignmentState,
  type VersionedJson,
} from "$lib/chat/contracts";
import { parseChatPushRequest, parseChatScheduleProposal } from "$lib/chat/validation";
import { compareOrderKeyThenId } from "$lib/utils/order-key";

/** Payload source a host tool call writes on the agent updates it posts. */
export const HOST_TOOL_UPDATE_SOURCE = "host_tool";

/** Kinds of report a teammate posts through the report host tool. */
const HOST_REPORT_KINDS = ["plan", "replan", "result"] as const;

/** A report kind posted through the report host tool. */
export type HostReportKind = (typeof HOST_REPORT_KINDS)[number];

/** What an agent update message carries, as far as the reply thread presents it. */
export type ChatAgentUpdatePresentation =
  | { kind: "host_question"; assignmentId: string; choices: string[] }
  | { kind: "push_request"; assignmentId: string; push: ChatPushRequest }
  | { kind: "host_report"; assignmentId: string; reportKind: HostReportKind }
  | { kind: "schedule_proposal"; assignmentId: string; proposal: ChatScheduleProposal }
  | { kind: "note_proposal"; assignmentId: string }
  | {
    kind: "delegation_outcome";
    parentAssignmentId: string;
    childAssignmentId: string;
    state: ChatWorkAssignmentState;
  }
  | {
    kind: "failure";
    assignmentId: string;
    /** The run that failed, or `null` when the work failed before a run started. */
    agentRunId: string | null;
    cause: ChatAgentFailureCause;
    /** The stored code, such as `timeout` or `provider_usage_limit`, for the technical detail only. */
    code: string | null;
  };

/** Why work failed, as far as the reply thread explains it in localized text. */
export type ChatAgentFailureCause =
  | { kind: "error"; code: ChatErrorCode }
  | { kind: "provider_limit" }
  | { kind: "launch" }
  | { kind: "unknown" };

/**
 * Reads the presentation of an agent update from a message's replicated rich content. Rich content
 * is written by other devices, so any shape it does not recognize returns `null` and the message
 * renders as plain text.
 */
export function chatAgentUpdatePresentation(richContent: VersionedJson): ChatAgentUpdatePresentation | null {
  const value = objectRecord(richContent.value);
  if (value?.type !== "agent_update") return null;
  const payload = objectRecord(value.payload);
  if (!payload) return null;
  const updateKind = value.updateKind;
  if (updateKind === "delegation") return delegationOutcome(payload);
  const assignmentId = nonEmptyString(value.assignmentId);
  if (updateKind === "failure") {
    return assignmentId === null ? null : failure(assignmentId, nonEmptyString(value.agentRunId), payload);
  }
  if (assignmentId === null || payload.source !== HOST_TOOL_UPDATE_SOURCE) return null;
  if (updateKind === "question") {
    if (payload.push !== undefined) {
      const push = pushRequest(payload.push);
      return push ? { kind: "push_request", assignmentId, push } : null;
    }
    return { kind: "host_question", assignmentId, choices: questionChoices(payload.choices) };
  }
  if (updateKind === "note_proposal") {
    return payload.kind === "note_proposal" ? { kind: "note_proposal", assignmentId } : null;
  }
  if (updateKind === "schedule_proposal") {
    const proposal = scheduleProposal(payload);
    return proposal ? { kind: "schedule_proposal", assignmentId, proposal } : null;
  }
  const reportKind = HOST_REPORT_KINDS.find((candidate) => candidate === updateKind);
  return reportKind ? { kind: "host_report", assignmentId, reportKind } : null;
}

/** Returns whether the rich content is an update a host tool call posted. */
export function isHostToolUpdate(richContent: VersionedJson): boolean {
  const value = objectRecord(richContent.value);
  return value?.type === "agent_update"
    && value.updateKind !== "delegation"
    && objectRecord(value.payload)?.source === HOST_TOOL_UPDATE_SOURCE;
}

function delegationOutcome(payload: Record<string, unknown>): ChatAgentUpdatePresentation | null {
  const parentAssignmentId = nonEmptyString(payload.parentAssignmentId);
  const childAssignmentId = nonEmptyString(payload.childAssignmentId);
  const state = CHAT_WORK_ASSIGNMENT_STATES.find((candidate) => candidate === payload.state);
  if (payload.phase !== "settled" || parentAssignmentId === null || childAssignmentId === null || !state) {
    return null;
  }
  return { kind: "delegation_outcome", parentAssignmentId, childAssignmentId, state };
}

function failure(
  assignmentId: string,
  agentRunId: string | null,
  payload: Record<string, unknown>,
): ChatAgentUpdatePresentation {
  const code = nonEmptyString(payload.code);
  return { kind: "failure", assignmentId, agentRunId, cause: failureCause(code, payload), code };
}

function failureCause(code: string | null, payload: Record<string, unknown>): ChatAgentFailureCause {
  if (payload.launchFailure === true) return { kind: "launch" };
  if (code === "provider_usage_limit") return { kind: "provider_limit" };
  const errorCode = CHAT_ERROR_CODES.find((candidate) => candidate === code);
  return errorCode ? { kind: "error", code: errorCode } : { kind: "unknown" };
}

/**
 * The technical detail of a failure for the copy button: the stored code, when there is one, then the diagnostic
 * text. The diagnostic text is untranslated provider or runtime output, so it is never the visible message.
 *
 * @param code The stored error code, such as `timeout`.
 * @param detail The diagnostic text, such as a failure message's body or a turn's stop reason.
 * @returns The detail to copy, or `null` when there is neither a code nor text.
 */
export function chatFailureTechnicalDetail(code: string | null, detail: string | null): string | null {
  const text = detail?.trim() ?? "";
  if (code && text && text !== code) return `${code}: ${text}`;
  return code ?? (text || null);
}

/** Reads a proposal payload another device wrote, or `null` when any field is malformed. */
function scheduleProposal(payload: Record<string, unknown>): ChatScheduleProposal | null {
  try {
    return parseChatScheduleProposal(payload);
  } catch {
    return null;
  }
}

/** Reads a push target another device wrote, or `null` when any field is malformed. */
function pushRequest(value: unknown): ChatPushRequest | null {
  try {
    return parseChatPushRequest(value);
  } catch {
    return null;
  }
}

/** Keeps the distinct, nonblank text choices, in order. */
function questionChoices(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const choices: string[] = [];
  for (const entry of value) {
    const choice = nonEmptyString(entry);
    if (choice !== null && !choices.includes(choice)) choices.push(choice);
  }
  return choices;
}

function objectRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function nonEmptyString(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

/** The question a person can still answer with a suggested choice. */
export interface AnswerableHostQuestion {
  messageItemId: string;
  choices: string[];
}

type QuestionReply = Pick<ChatMessageRead, "itemId" | "orderKey" | "richContent" | "author">;

/**
 * Returns the newest host question of the thread's work while that work waits for an answer, when it
 * offers choices and no person replied after it. Answered questions and newer push requests get no chips.
 */
export function answerableHostQuestion(
  replies: readonly QuestionReply[],
  assignment: Pick<ChatWorkAssignmentRead, "id" | "state"> | null,
): AnswerableHostQuestion | null {
  if (assignment?.state !== "waiting_for_answer") return null;
  const ordered = [...replies].sort((left, right) => (
    compareOrderKeyThenId(left.orderKey, left.itemId, right.orderKey, right.itemId)
  ));
  for (let index = ordered.length - 1; index >= 0; index -= 1) {
    const reply = ordered[index];
    if (!reply) continue;
    const presentation = chatAgentUpdatePresentation(reply.richContent);
    if (presentation?.kind === "host_question" && presentation.assignmentId === assignment.id) {
      return presentation.choices.length > 0 ? { messageItemId: reply.itemId, choices: presentation.choices } : null;
    }
    if (presentation?.kind === "push_request" && presentation.assignmentId === assignment.id) return null;
    if (reply.author && reply.author.kind !== "ai_teammate") return null;
  }
  return null;
}
