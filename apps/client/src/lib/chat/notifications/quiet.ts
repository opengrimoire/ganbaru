import { Temporal } from "@js-temporal/polyfill";
import type { ChatQuietStateRead } from "$lib/chat/contracts";
import type { ChatQuietState } from "./gate";

/**
 * Maps the native quiet state read, which already folds in quiet periods and the Focus phase, to the gate's
 * quiet state.
 *
 * @param read The validated `chat_read_quiet_state` result.
 */
export function chatQuietState(read: ChatQuietStateRead): ChatQuietState {
  return {
    quiet: read.quiet,
    endsAtMs: read.quiet && read.endsAt !== null ? Temporal.Instant.from(read.endsAt).epochMilliseconds : null,
  };
}
