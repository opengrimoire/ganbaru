import type { Translate } from "$lib/i18n/translator.svelte";
import type {
  SyncDomainPresentation,
  SyncRecoveryPresentation,
  SyncTablePresentation,
} from "$lib/sync/presentation-registry";
import { notifyChatSyncChanged } from "./sync-events";

/** Replicated Chat tables, as sync names them in applied notices, conflicts, and recovery offers. */
export const CHAT_SYNC_TABLES: readonly string[] = [
  "chat_participants",
  "chat_teammates",
  "chat_teammate_policy_revisions",
  "chat_access_profiles",
  "chat_access_profile_revisions",
  "chat_conversations",
  "chat_conversation_memberships",
  "chat_teammate_channel_memberships",
  "chat_teammate_working_folder_grants",
  "chat_channels",
  "chat_reply_threads",
  "chat_conversation_items",
  "chat_communication_messages",
  "chat_communication_message_revisions",
  "chat_message_references",
  "chat_channel_reference_targets",
  "chat_execution_environment_reference_targets",
  "chat_participant_reference_targets",
  "chat_working_folder_reference_targets",
  "chat_workspace_path_reference_targets",
  "chat_conversation_read_cursors",
  "chat_reply_thread_read_cursors",
  "chat_scheduled_messages",
  "chat_work_assignments",
  "chat_work_assignment_inputs",
  "chat_work_semantic_updates",
];

/**
 * Chat archives its rows instead of deleting them, so a Chat row is deleted only with its project. Its offer is
 * only discarded: restoring the project restores no Chat rows and starts a new, empty default channel. The
 * project's offer lists the text of posts written unseen by the deletion so it can be copied.
 */
function discardOnly(untitled: (t: Translate) => string): SyncRecoveryPresentation {
  return { untitled, note: (t) => t("chat.sync.recoveryNote"), restorable: false };
}

/** Names, roles, channel details, and message edits made on two devices are chosen in the Chat settings. */
const TABLE_PRESENTATIONS: Readonly<Record<string, SyncTablePresentation>> = {
  chat_participants: {
    conflicts: (t, count) => t("chat.sync.participantConflicts", count),
    recovery: discardOnly((t) => t("chat.sync.participantUntitled")),
  },
  chat_teammates: {
    conflicts: (t, count) => t("chat.sync.teammateConflicts", count),
    recovery: discardOnly((t) => t("chat.sync.teammateUntitled")),
  },
  chat_conversations: { recovery: discardOnly((t) => t("chat.sync.channelUntitled")) },
  chat_channels: {
    conflicts: (t, count) => t("chat.sync.channelConflicts", count),
    recovery: discardOnly((t) => t("chat.sync.channelUntitled")),
  },
  chat_communication_messages: {
    conflicts: (t, count) => t("chat.sync.messageConflicts", count),
    recovery: discardOnly((t) => t("chat.sync.messageUntitled")),
  },
};

/** How the sync settings present Chat. Tables without their own copy use neutral sync copy. */
export const CHAT_SYNC_PRESENTATION: SyncDomainPresentation = {
  label: (t) => t("titleBar.tab.chat"),
  tables: Object.fromEntries(CHAT_SYNC_TABLES.map((table) => [table, TABLE_PRESENTATIONS[table] ?? {}])),
  changed: notifyChatSyncChanged,
};
