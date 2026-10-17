import type { ChatChannelId, ChatThreadId } from "$lib/chat/contracts";
import { createVaultChangeChannel } from "$lib/windows/vault-change-channel";

/** Chat records another window changed: provider-thread shells, and channels with their messages. */
export interface ChatWindowChanges {
  threadIds: ReadonlySet<ChatThreadId>;
  channelIds: ReadonlySet<ChatChannelId>;
}

const THREAD_KEY_PREFIX = "thread:";
const CHANNEL_KEY_PREFIX = "channel:";

const chatChanges = createVaultChangeChannel("chat-changed");

/** Keys naming the changed threads and channels on the change channel. */
export function chatChangeKeys(changes: { threadIds?: Iterable<ChatThreadId>; channelIds?: Iterable<ChatChannelId> }): string[] {
  return [
    ...[...changes.threadIds ?? []].map((id) => `${THREAD_KEY_PREFIX}${id}`),
    ...[...changes.channelIds ?? []].map((id) => `${CHANNEL_KEY_PREFIX}${id}`),
  ];
}

/** Sorts received keys back into thread and channel ids, ignoring keys this version does not know. */
export function parseChatChangeKeys(keys: Iterable<string>): ChatWindowChanges {
  const threadIds = new Set<ChatThreadId>();
  const channelIds = new Set<ChatChannelId>();
  for (const key of keys) {
    if (key.startsWith(THREAD_KEY_PREFIX) && key.length > THREAD_KEY_PREFIX.length) {
      threadIds.add(key.slice(THREAD_KEY_PREFIX.length));
    } else if (key.startsWith(CHANNEL_KEY_PREFIX) && key.length > CHANNEL_KEY_PREFIX.length) {
      channelIds.add(key.slice(CHANNEL_KEY_PREFIX.length));
    }
  }
  return { threadIds, channelIds };
}

/** Tell other windows of this vault that this window changed Chat threads or channels. */
export function publishChatChanges(changes: { threadIds?: Iterable<ChatThreadId>; channelIds?: Iterable<ChatChannelId> }): void {
  chatChanges.publish(chatChangeKeys(changes));
}

/** Receive the Chat threads and channels other windows changed, gathered per pause in their activity. */
export function listenForChatChanges(onChange: (changes: ChatWindowChanges) => void): Promise<() => void> {
  return chatChanges.listen((keys) => onChange(parseChatChangeKeys(keys)));
}
