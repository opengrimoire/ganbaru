import { createVaultChangeChannel } from "$lib/windows/vault-change-channel";

/** Tells other windows which Chat drafts were saved, so open composers can show the latest text. */
export interface ChatDraftWindowSync {
  /** Announce that this window saved or removed a draft. */
  publish(draftId: string): void;
  /** Receive the ids of drafts other windows changed. Resolves to a function that stops listening. */
  listen(onChange: (draftIds: ReadonlySet<string>) => void): Promise<() => void>;
}

const draftChanges = createVaultChangeChannel("chat-drafts-changed");

/** Draft change channel shared by every window of the active vault. */
export const chatDraftWindowSync: ChatDraftWindowSync = {
  publish: (draftId) => draftChanges.publish([draftId]),
  listen: (onChange) => draftChanges.listen(onChange),
};
