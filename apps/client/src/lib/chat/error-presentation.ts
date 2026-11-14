import { translate, type TranslationKey } from "$lib/i18n/translator.svelte";
import { localizedErrorText, technicalErrorDetail } from "$lib/i18n/user-errors";
import { CHAT_ERROR_CODES, type ChatErrorCode, type ProbeState } from "./contracts";

/**
 * `details.reason` values the backend attaches to Chat failures a person can trigger, and the
 * catalog message that explains each. Push request refusals are explained by their own card.
 */
export const CHAT_ERROR_REASON_MESSAGES = {
  input_already_delivered: "chat.composer.pendingInputs.alreadyDelivered",
  input_starts_work: "chat.composer.pendingInputs.startsWork",
  pending_replies: "chat.organization.reviewPendingReplies",
  no_executing_device: "chat.organization.reviewNoDevice",
  checkpoint_pair: "chatErrors.reasons.checkpointPair",
  provider_turn: "chatErrors.reasons.providerTurn",
  executed_elsewhere: "chatErrors.reasons.executedElsewhere",
  assignment_checkpoints: "chatErrors.reasons.assignmentCheckpoints",
  assignment_repository_changed: "chatErrors.reasons.assignmentRepositoryChanged",
} as const satisfies Record<string, TranslationKey>;

export type ChatErrorReason = keyof typeof CHAT_ERROR_REASON_MESSAGES;

/**
 * Codes whose own message says less than the action the caller names, so a caller's localized
 * action message wins over them.
 */
const ACTION_FIRST_CODES: ReadonlySet<ChatErrorCode> = new Set(["validation", "protocol", "persistence", "internal"]);

/**
 * The localized message a person sees for a Chat failure. Backend text is diagnostic English and
 * never shown; the message comes from, in order, localized text the failure carries, a known
 * `details.reason`, the error code, the caller's localized `fallback`, and a generic Chat message.
 * For codes that only say something went wrong, the caller's `fallback` wins over the code.
 *
 * @param error Any rejected Tauri, browser, or provider value.
 * @param fallback Localized message naming the action that failed.
 */
export function chatErrorMessage(error: unknown, fallback?: string): string {
  const localized = localizedErrorText(error);
  if (localized) return localized;
  const reason = chatErrorReason(error);
  if (reason && isChatErrorReason(reason)) return translate(CHAT_ERROR_REASON_MESSAGES[reason]);
  const code = chatErrorCode(error);
  if (code && !(fallback && ACTION_FIRST_CODES.has(code))) return translate(`chatErrors.codes.${code}`);
  return fallback ?? translate("chatErrors.generic");
}

/**
 * The backend's own diagnostic text of a Chat failure, bounded, for secondary detail such as a
 * provider's stderr behind a copy button. Never the primary message.
 *
 * @param error Any rejected value.
 */
export function chatErrorDetail(error: unknown): string | null {
  return technicalErrorDetail(error);
}

/**
 * The localized explanation of a provider that is not ready to run.
 *
 * @param state The last probe state, which shares its wire names with Chat error codes, or null when unchecked.
 * @returns The probe state's message, or the general unavailable message when the probe passed or never ran.
 */
export function providerProbeMessage(state: ProbeState | null | undefined): string {
  if (!state || state === "healthy") return translate("chat.status.providerUnavailable");
  return translate(`chatErrors.codes.${state}`);
}

/** Extracts the field associated with a structured Chat validation failure. */
export function chatErrorField(error: unknown): string | null {
  const record = objectRecord(error);
  if (!record) return null;
  const direct = stringValue(record.field);
  if (direct) return direct;
  return stringValue(objectRecord(record.error)?.field);
}

/** Extracts a recognized Chat error code from direct and nested failures. */
export function chatErrorCode(error: unknown): ChatErrorCode | null {
  const record = objectRecord(error);
  if (!record) return null;
  const code = stringValue(record.code)
    ?? stringValue(objectRecord(record.error)?.code)
    ?? stringValue(objectRecord(record.cause)?.code);
  return code && isChatErrorCode(code) ? code : null;
}

/** Extracts the `details.reason` a structured Chat failure names, from direct and nested failures. */
export function chatErrorReason(error: unknown): string | null {
  const record = objectRecord(error);
  if (!record) return null;
  return stringValue(objectRecord(record.details)?.reason)
    ?? stringValue(objectRecord(objectRecord(record.error)?.details)?.reason)
    ?? stringValue(objectRecord(objectRecord(record.cause)?.details)?.reason);
}

function objectRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function stringValue(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function isChatErrorCode(value: string): value is ChatErrorCode {
  return (CHAT_ERROR_CODES as readonly string[]).includes(value);
}

function isChatErrorReason(value: string): value is ChatErrorReason {
  return Object.hasOwn(CHAT_ERROR_REASON_MESSAGES, value);
}
