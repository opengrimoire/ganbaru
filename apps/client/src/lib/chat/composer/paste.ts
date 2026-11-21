/**
 * Pasted or dropped text of at least this many UTF-8 bytes becomes a text attachment instead of
 * draft text. Below it a paste stays readable inline; above it the message would dominate the
 * thread.
 */
export const LARGE_PASTE_ATTACHMENT_BYTES = 32 * 1024;

/**
 * The largest text attachment, in UTF-8 bytes. It mirrors `MAX_TEXT_SNIPPET_BYTES` in the Tauri
 * attachment commands, which refuse anything larger, so a pasted log fits.
 */
export const MAX_TEXT_SNIPPET_BYTES = 1024 * 1024;

/** Where incoming text goes: into the draft, into a text attachment, or nowhere because it is too large. */
export type PastedTextPlacement = "inline" | "attachment" | "too_large";

const encoder = new TextEncoder();

/**
 * Measures text the way the attachment bound does.
 *
 * @param text The text to measure.
 * @returns Its length in UTF-8 bytes.
 */
export function utf8ByteLength(text: string): number {
  return encoder.encode(text).byteLength;
}

/**
 * Decides where pasted or dropped text goes.
 *
 * @param text The incoming text, after hidden characters are removed.
 * @returns `inline` below `LARGE_PASTE_ATTACHMENT_BYTES`, `attachment` from it up to
 *   `MAX_TEXT_SNIPPET_BYTES`, and `too_large` above that, which is refused rather than truncated.
 */
export function classifyPastedText(text: string): PastedTextPlacement {
  const bytes = utf8ByteLength(text);
  if (bytes < LARGE_PASTE_ATTACHMENT_BYTES) return "inline";
  return bytes <= MAX_TEXT_SNIPPET_BYTES ? "attachment" : "too_large";
}

/**
 * Counts the lines of pasted text for its attachment name. A final newline does not start a line.
 *
 * @param text The pasted text.
 * @returns The number of lines, at least 1.
 */
export function pastedTextLineCount(text: string): number {
  const body = text.endsWith("\n") ? text.slice(0, -1) : text;
  return body.split("\n").length;
}
