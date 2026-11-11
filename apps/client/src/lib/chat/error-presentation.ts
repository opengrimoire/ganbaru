import { CHAT_ERROR_CODES, type ChatErrorCode } from "./contracts";

const FALLBACK_CHAT_ERROR = "Chat operation failed";

/** Extracts a useful bounded message from browser, Tauri, and provider failures. */
export function chatErrorMessage(error: unknown, fallback = FALLBACK_CHAT_ERROR): string {
  if (error instanceof Error && error.message.trim()) return error.message.trim();
  if (typeof error === "string" && error.trim()) return error.trim();
  const record = objectRecord(error);
  if (!record) return fallback;
  const direct = stringValue(record.message) ?? stringValue(record.error) ?? stringValue(record.detail);
  if (direct) return direct;
  const nested = objectRecord(record.error);
  return stringValue(nested?.message) ?? stringValue(nested?.detail) ?? fallback;
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
    ?? stringValue(objectRecord(objectRecord(record.error)?.details)?.reason);
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
