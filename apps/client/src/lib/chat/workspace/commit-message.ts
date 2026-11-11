/**
 * Longest commit message, in UTF-8 bytes, that the backend accepts. Mirrors the Rust bound in
 * the Chat Git commit path.
 */
export const COMMIT_MESSAGE_MAX_BYTES = 4_000;

const utf8Encoder = new TextEncoder();

/**
 * Normalizes `message` as the backend does before committing: line endings become LF and
 * surrounding whitespace is trimmed.
 */
export function normalizeCommitMessage(message: string): string {
  return message.replace(/\r\n?/g, "\n").trim();
}

/**
 * Whether `message`, normalized as committing normalizes it, passes the backend byte bound.
 */
export function commitMessageTooLong(message: string): boolean {
  return utf8Encoder.encode(normalizeCommitMessage(message)).length > COMMIT_MESSAGE_MAX_BYTES;
}
