import { describe, expect, it } from "vitest";
import { translate as t } from "$lib/i18n/translator.svelte";
import { SyncPresentationRegistry } from "$lib/sync/presentation-registry";
import { MUSIC_SYNC_TABLES } from "./library/replication";
import { MUSIC_SYNC_PRESENTATION } from "./sync-presentation";

describe("music sync presentation", () => {
  const registry = new SyncPresentationRegistry().register(MUSIC_SYNC_PRESENTATION);

  it("registers every replicated Music table once", () => {
    expect(Object.keys(MUSIC_SYNC_PRESENTATION.tables).sort()).toEqual([...MUSIC_SYNC_TABLES].sort());
  });

  it("restores playlists as new playlists and offers tracks for discard only", () => {
    expect(registry.recoveryRestorable("music_playlists")).toBe(true);
    expect(registry.recoveryRestorable("music_library_items")).toBe(false);
    expect(registry.recoveryRestorable("music_playlist_memberships")).toBe(true);
  });

  it("describes playlist and track offers with their own copy", () => {
    expect(registry.recoveryUntitled(t, "music_playlists")).toBe(t("music.sync.playlistUntitled"));
    expect(registry.recoveryNote(t, "music_playlists")).toBe(t("music.sync.playlistRecoveryNote"));
    expect(registry.recoveryRestored(t, "music_playlists")).toBe(t("music.sync.playlistRestored"));
    expect(registry.recoveryUntitled(t, "music_library_items")).toBe(t("music.sync.itemUntitled"));
    expect(registry.recoveryNote(t, "music_library_items")).toBe(t("music.sync.itemRecoveryNote"));
  });

  it("notifies the Music views when its tables change", () => {
    expect(registry.changedNotices(["music_playlist_memberships", "music_snoozes"])).toEqual([MUSIC_SYNC_PRESENTATION.changed]);
  });
});
