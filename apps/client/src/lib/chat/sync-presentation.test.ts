import { describe, expect, it, vi } from "vitest";
import { en } from "$lib/i18n/messages/en";
import { es } from "$lib/i18n/messages/es";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import { syncPresentations } from "$lib/sync/presentations";
import { onChatSyncChanged } from "./sync-events";
import { CHAT_SYNC_PRESENTATION, CHAT_SYNC_TABLES } from "./sync-presentation";

vi.mock("$lib/projects/window-sync", () => ({ publishProjectsSyncChange: vi.fn() }));

const tEn: Translate = ((key, ...args) => translateFromPartialCatalog(en, key, ...args)) as Translate;
const tEs: Translate = ((key, ...args) => translateFromPartialCatalog(es, key, ...args)) as Translate;

const CONFLICT_TABLES = ["chat_participants", "chat_teammates", "chat_channels", "chat_communication_messages"];
const RECOVERY_ROOT_TABLES = [
  "chat_participants",
  "chat_teammates",
  "chat_conversations",
  "chat_channels",
  "chat_communication_messages",
];

describe("chat sync presentation", () => {
  it("registers every replicated Chat table once", () => {
    expect(Object.keys(CHAT_SYNC_PRESENTATION.tables).sort()).toEqual([...CHAT_SYNC_TABLES].sort());
    expect(new Set(CHAT_SYNC_TABLES).size).toBe(CHAT_SYNC_TABLES.length);
    expect(syncPresentations.labels(tEn)).toContain("Chat");
  });

  it("points conflicts to the Chat settings, with plural forms in both languages", () => {
    for (const t of [tEn, tEs]) {
      for (const table of CONFLICT_TABLES) {
        const one = syncPresentations.conflictLine(t, table, 1);
        const many = syncPresentations.conflictLine(t, table, 3);
        expect(one).not.toBe(t("sync.conflicts", 1));
        expect(many).not.toBe(one);
        expect(many).toContain("3");
      }
    }
  });

  it("offers deleted Chat rows only for discarding, with a title and an explanation", () => {
    for (const t of [tEn, tEs]) {
      for (const table of RECOVERY_ROOT_TABLES) {
        expect(syncPresentations.recoveryRestorable(table)).toBe(false);
        expect(syncPresentations.recoveryUntitled(t, table)).not.toBe(t("sync.recovery.untitled"));
        expect(syncPresentations.recoveryNote(t, table)).toBeTruthy();
      }
    }
  });

  it("uses neutral copy for tables without their own", () => {
    expect(syncPresentations.conflictLine(tEn, "chat_work_assignments", 2)).toBe(tEn("sync.conflicts", 2));
    expect(syncPresentations.recoveryNote(tEn, "chat_conversation_items")).toBeNull();
  });

  it("tells this window's Chat views that rows changed", () => {
    vi.stubGlobal("window", new EventTarget());
    const listener = vi.fn();
    const stop = onChatSyncChanged(listener);
    try {
      CHAT_SYNC_PRESENTATION.changed();
    } finally {
      stop();
      vi.unstubAllGlobals();
    }
    expect(listener).toHaveBeenCalledTimes(1);
  });
});
