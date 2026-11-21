import type { ChatChannelId, ChatTerminalOutputChunk, ProjectWorkingFolderId } from "$lib/chat/contracts";
import { chatErrorMessage } from "$lib/chat/error-presentation";
import { safeExternalUrl } from "$lib/chat/timeline/markdown";

export interface TerminalOutputState {
  generation: number;
  lastSequence: number;
}

export type TerminalOutputDecision =
  | { kind: "accept"; state: TerminalOutputState; bytes: Uint8Array }
  | { kind: "duplicate"; state: TerminalOutputState }
  | { kind: "gap"; state: TerminalOutputState; expectedSequence: number };

/**
 * Returns the output state a view starts from before writing a snapshot's scrollback. Bounded replay drops its
 * oldest chunks, so the first retained chunk sets the expected sequence instead of sequence one.
 *
 * @param generation The terminal generation the snapshot describes.
 * @param scrollback The snapshot's replay chunks in order.
 */
export function replayStartState(
  generation: number,
  scrollback: readonly ChatTerminalOutputChunk[],
): TerminalOutputState {
  const first = scrollback.find((chunk) => chunk.generation === generation);
  return { generation, lastSequence: first ? Math.max(0, first.sequence - 1) : 0 };
}

export function applyTerminalOutput(
  state: TerminalOutputState,
  chunk: ChatTerminalOutputChunk,
): TerminalOutputDecision {
  if (chunk.generation < state.generation) return { kind: "duplicate", state };
  const generationChanged = chunk.generation > state.generation;
  const expected = generationChanged ? 1 : state.lastSequence + 1;
  if (chunk.sequence < expected) return { kind: "duplicate", state };
  if (chunk.sequence > expected) return { kind: "gap", state, expectedSequence: expected };
  return {
    kind: "accept",
    state: { generation: chunk.generation, lastSequence: chunk.sequence },
    bytes: decodeBase64(chunk.dataBase64),
  };
}

/** Live output bytes a terminal view writes before returning them as delivery credit, well below the native window. */
export const TERMINAL_OUTPUT_ACK_INTERVAL_BYTES = 64 * 1024;

/** Batches delivery credit for live terminal output so acknowledgements stay infrequent. */
export class TerminalOutputAcknowledger {
  #pending = 0;

  constructor(private readonly intervalBytes = TERMINAL_OUTPUT_ACK_INTERVAL_BYTES) {}

  /**
   * Records written output bytes.
   *
   * @returns The bytes to acknowledge now, or zero while the batch is below the interval.
   */
  record(bytes: number): number {
    this.#pending += Math.max(0, bytes);
    if (this.#pending < this.intervalBytes) return 0;
    const released = this.#pending;
    this.#pending = 0;
    return released;
  }
}

/** Returns the decoded size of a terminal output chunk without decoding it. */
export function terminalOutputByteLength(chunk: ChatTerminalOutputChunk): number {
  const encoded = chunk.dataBase64;
  const padding = encoded.endsWith("==") ? 2 : encoded.endsWith("=") ? 1 : 0;
  return Math.max(0, Math.floor(encoded.length * 3 / 4) - padding);
}

/** C0 controls except tab, line feed, and carriage return, plus DEL and the C1 range (8-bit CSI included). */
const TERMINAL_PASTE_CONTROLS = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/g;

/**
 * Removes escape and control characters from clipboard text before it reaches the shell.
 *
 * Removing ESC itself, rather than matching a bracketed-paste end sequence, means no removal can
 * form a new escape sequence from the remaining text.
 *
 * @param text - Clipboard text.
 * @returns The text to paste and whether any character was removed.
 */
export function sanitizeTerminalPaste(text: string): { text: string; removedControls: boolean } {
  const clean = text.replace(TERMINAL_PASTE_CONTROLS, "");
  return { text: clean, removedControls: clean.length !== text.length };
}

/**
 * Resolves the target of a terminal hyperlink (OSC 8). Terminal output is untrusted, so only
 * credential-free HTTP and HTTPS targets are opened; anything else is ignored.
 *
 * @param uri - The link target the output declared, which can differ from the visible text.
 * @returns The normalized target to show and open, or null when the link must not open.
 */
export function terminalLinkTarget(uri: string): string | null {
  return safeExternalUrl(uri) ? new URL(uri).href : null;
}

export function terminalPasteNeedsConfirmation(text: string, confirmationEnabled: boolean): boolean {
  return confirmationEnabled && /[\r\n]/.test(text);
}

export function boundTerminalContext(text: string, byteLimit: number): {
  text: string;
  byteSize: number;
  lineCount: number;
  truncated: boolean;
} {
  const bytes = new TextEncoder().encode(text);
  if (bytes.byteLength <= byteLimit) {
    return {
      text,
      byteSize: bytes.byteLength,
      lineCount: text.length === 0 ? 0 : text.split(/\r\n|\r|\n/).length,
      truncated: false,
    };
  }
  let boundary = byteLimit;
  while (boundary > 0 && (bytes[boundary] & 0xc0) === 0x80) boundary -= 1;
  const bounded = new TextDecoder().decode(bytes.slice(0, boundary));
  return {
    text: bounded,
    byteSize: new TextEncoder().encode(bounded).byteLength,
    lineCount: bounded.length === 0 ? 0 : bounded.split(/\r\n|\r|\n/).length,
    truncated: true,
  };
}

export type TerminalPanelPlacement = "inspector" | "bottom";

/**
 * Names the scope that owns a set of terminals: one channel in one working folder.
 *
 * @param channelId - Channel whose panels show the terminals.
 * @param workingFolderId - Folder the terminals start in.
 * @returns A key that differs for every channel and folder pair, or null when either is missing.
 */
export function terminalScopeKey(
  channelId: ChatChannelId | null,
  workingFolderId: ProjectWorkingFolderId | null,
): string | null {
  return channelId && workingFolderId ? `${channelId}\n${workingFolderId}` : null;
}

/** Keeps terminal ownership and selection independent between workspace panels. */
export class TerminalPanelRegistry {
  readonly #placementByTerminal = new Map<string, TerminalPanelPlacement>();
  readonly #selectionByPanelScope = new Map<string, string>();

  /**
   * Claims unassigned terminals for a panel and returns only that panel's terminals.
   *
   * @param terminals - Terminals available for the current channel and working folder.
   * @param placement - Panel requesting its terminal collection.
   * @returns Terminals owned by the requesting panel.
   */
  claimAvailable<T extends { id: string }>(
    terminals: readonly T[],
    placement: TerminalPanelPlacement,
  ): T[] {
    const claimed: T[] = [];
    for (const terminal of terminals) {
      const owner = this.#placementByTerminal.get(terminal.id);
      if (owner === placement) claimed.push(terminal);
      else if (owner === undefined) {
        this.#placementByTerminal.set(terminal.id, placement);
        claimed.push(terminal);
      }
    }
    return claimed;
  }

  /** Assigns a newly created terminal to one panel. */
  assign(terminalId: string, placement: TerminalPanelPlacement): void {
    this.#placementByTerminal.set(terminalId, placement);
  }

  /** Releases ownership after a terminal is closed. */
  release(terminalId: string): void {
    this.#placementByTerminal.delete(terminalId);
  }

  /** Returns the selected terminal for one panel in one channel and working folder. */
  selected(scopeKey: string, placement: TerminalPanelPlacement): string | null {
    return this.#selectionByPanelScope.get(this.#selectionKey(scopeKey, placement)) ?? null;
  }

  /** Updates the selected terminal for one panel in one channel and working folder. */
  select(
    scopeKey: string,
    placement: TerminalPanelPlacement,
    terminalId: string | null,
  ): void {
    const key = this.#selectionKey(scopeKey, placement);
    if (terminalId) this.#selectionByPanelScope.set(key, terminalId);
    else this.#selectionByPanelScope.delete(key);
  }

  #selectionKey(scopeKey: string, placement: TerminalPanelPlacement): string {
    return `${placement}:${scopeKey}`;
  }
}

/**
 * Converts an unknown terminal API rejection into a localized message.
 *
 * @param reason - The rejected value from the terminal API boundary.
 * @param fallback - Localized text naming the terminal action that failed.
 * @returns The localized error message.
 */
export function terminalErrorMessage(reason: unknown, fallback: string): string {
  return chatErrorMessage(reason, fallback);
}

function decodeBase64(value: string): Uint8Array {
  const decoded = atob(value);
  return Uint8Array.from(decoded, (character) => character.charCodeAt(0));
}
