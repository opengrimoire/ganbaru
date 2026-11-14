import type { ChatConversationItemId } from "./common";

/** What a teammate asks to push, as Ganbaru read it from the working folder when the teammate asked. */
export interface ChatPushRequest {
  workingFolderId: string;
  /** The execution environment the target was read in, `null` for the folder itself. */
  executionEnvironmentId: string | null;
  localBranch: string;
  remote: string;
  destinationBranch: string;
  /** Where the push goes; never carries a password. */
  pushUrl: string;
  /** The full commit id the push sends. */
  objectId: string;
  /** Digest of the folder's command-bearing Git configuration when the target was read. */
  gitConfigDigest: string;
}

/**
 * Where a push request stands: `waiting` while the asking turn still runs, `open` once it ended and a
 * person may confirm, `pushed` after a confirmation, and `closed` when a reply or a failed turn ended it.
 */
export const CHAT_PUSH_REQUEST_STATES = ["waiting", "open", "pushed", "closed"] as const;
export type ChatPushRequestState = (typeof CHAT_PUSH_REQUEST_STATES)[number];

/** Where one push request stands. */
export interface ChatPushRequestStateRead {
  requestItemId: ChatConversationItemId;
  state: ChatPushRequestState;
}

/** A person's confirmation of a push request, with the reply text posted after the teammate mention. */
export interface ConfirmPushRequestRequest {
  requestItemId: ChatConversationItemId;
  normalizedMarkdown: string;
}

/** Reasons Rust gives when a push request cannot be confirmed. */
export const CHAT_PUSH_REQUEST_REFUSALS = [
  "push_request_pending",
  "push_request_closed",
  "push_target_changed",
] as const;
export type ChatPushRequestRefusal = (typeof CHAT_PUSH_REQUEST_REFUSALS)[number];
