import type { ChatTerminalOutputChunk } from "$lib/chat/contracts";

export interface TerminalOutputState {
  generation: number;
  lastSequence: number;
}

export type TerminalOutputDecision =
  | { kind: "accept"; state: TerminalOutputState; bytes: Uint8Array }
  | { kind: "duplicate"; state: TerminalOutputState }
  | { kind: "gap"; state: TerminalOutputState; expectedSequence: number };

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

/** Keeps terminal ownership and selection independent between workspace panels. */
export class TerminalPanelRegistry {
  readonly #placementByTerminal = new Map<string, TerminalPanelPlacement>();
  readonly #selectionByPanelThread = new Map<string, string>();

  /**
   * Claims unassigned terminals for a panel and returns only that panel's terminals.
   *
   * @param terminals - Terminals available for the current thread and workspace.
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

  /** Returns the selected terminal for one panel and thread. */
  selected(threadId: string, placement: TerminalPanelPlacement): string | null {
    return this.#selectionByPanelThread.get(this.#selectionKey(threadId, placement)) ?? null;
  }

  /** Updates the selected terminal for one panel and thread. */
  select(
    threadId: string,
    placement: TerminalPanelPlacement,
    terminalId: string | null,
  ): void {
    const key = this.#selectionKey(threadId, placement);
    if (terminalId) this.#selectionByPanelThread.set(key, terminalId);
    else this.#selectionByPanelThread.delete(key);
  }

  #selectionKey(threadId: string, placement: TerminalPanelPlacement): string {
    return `${placement}:${threadId}`;
  }
}

/**
 * Converts an unknown terminal API rejection into a useful user-facing message.
 *
 * @param reason - The rejected value from the terminal API boundary.
 * @param fallback - Localized text used when the rejection has no readable message.
 * @returns The validated error message.
 */
export function terminalErrorMessage(reason: unknown, fallback: string): string {
  if (reason instanceof Error) return reason.message;
  if (typeof reason === "string") return reason;
  if (typeof reason !== "object" || reason === null || Array.isArray(reason)) return fallback;
  const record = reason as Record<string, unknown>;
  if (typeof record.message !== "string" || record.message.length === 0) return fallback;
  return typeof record.field === "string" && record.field.length > 0
    ? `${record.field}: ${record.message}`
    : record.message;
}

function decodeBase64(value: string): Uint8Array {
  const decoded = atob(value);
  return Uint8Array.from(decoded, (character) => character.charCodeAt(0));
}
