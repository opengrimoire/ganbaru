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
  /** The full id of the commit the push sends, or of the parent of the commit Ganbaru makes. */
  objectId: string;
  /** Digest of the folder's whole effective Git configuration when the target was read. */
  gitConfigDigest: string;
  /** The commit Ganbaru makes from the working tree on `objectId` when a person confirms; absent to push `objectId` itself. */
  commit?: ChatPushRequestCommit;
}

/** A commit a push request asks Ganbaru to make at confirmation, as Ganbaru read the working tree when the teammate asked. */
export interface ChatPushRequestCommit {
  message: string;
  treeId: string;
  /** `Name <email>` the commit records as author and committer. */
  author: string;
  /** Whether the person's Git configuration signs the commit; a commit that cannot be signed is not made. */
  signed: boolean;
  /** The first changed paths, at most {@link CHAT_PUSH_COMMIT_MAX_LISTED_PATHS}. */
  changedPaths: string[];
  changedPathCount: number;
}

/** Most changed paths a push request lists. */
export const CHAT_PUSH_COMMIT_MAX_LISTED_PATHS = 50;

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
  "push_repository_connection",
] as const;
export type ChatPushRequestRefusal = (typeof CHAT_PUSH_REQUEST_REFUSALS)[number];
