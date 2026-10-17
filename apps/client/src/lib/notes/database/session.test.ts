import { describe, expect, it, vi } from "vitest";
import { createNotesDatabaseSession, databaseResource, notesDatabaseSession } from "./session.svelte";
import { setActiveVaultIdentity } from "$lib/vault/active-vault";
import type { NotesDataSourceTemplate } from "$lib/notes/types";

function templates(name = "Default"): NotesDataSourceTemplate[] {
  return [{ object: "data_source_template", id: "template", data_source_id: "source",
    source_page_id: null, name, properties: {}, is_default: true, block_count: 0,
    created_time: "2026-09-29T00:00:00Z", last_edited_time: "2026-09-29T00:00:00Z" }];
}

describe("Notes database sessions", () => {
  it("reuses loaded data and shares pending reads without sharing mutable snapshots", async () => {
    const session = createNotesDatabaseSession();
    const resource = databaseResource("templates", "source");
    const loader = vi.fn(async () => templates());
    const [first, second] = await Promise.all([session.load(resource, loader), session.load(resource, loader)]);
    expect(loader).toHaveBeenCalledOnce();
    first[0].name = "Local edit";
    expect(second[0].name).toBe("Default");
    expect(session.read(resource)?.[0].name).toBe("Default");
    await session.load(resource, loader);
    expect(loader).toHaveBeenCalledOnce();
    await session.load(resource, loader, true);
    expect(loader).toHaveBeenCalledTimes(2);
  });

  it("keeps the last snapshot while invalidating every linked view after a mutation", async () => {
    const session = createNotesDatabaseSession();
    const resource = databaseResource("templates", "source");
    const linked = databaseResource("views", "source", { databaseId: "linked" });
    session.write(resource, templates());
    session.write(linked, []);
    session.invalidate();
    expect(session.read(resource)?.[0].name).toBe("Default");
    const loader = vi.fn(async () => templates("Updated"));
    const linkedLoader = vi.fn(async () => []);
    await session.load(resource, loader);
    await session.load(linked, linkedLoader);
    expect(session.read(resource)?.[0].name).toBe("Updated");
    expect(linkedLoader).toHaveBeenCalledOnce();
  });

  it("never makes a response from before a mutation current", async () => {
    const session = createNotesDatabaseSession();
    const resource = databaseResource("templates", "source");
    let finish: (value: NotesDataSourceTemplate[]) => void = () => {};
    const loader = vi.fn<() => Promise<NotesDataSourceTemplate[]>>()
      .mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }))
      .mockResolvedValue(templates("Updated"));
    const pending = session.load(resource, loader);
    await Promise.resolve();
    session.invalidate();
    finish(templates("Old"));
    expect((await pending)[0].name).toBe("Updated");
    expect(loader).toHaveBeenCalledTimes(2);
    expect(session.read(resource)?.[0].name).toBe("Updated");
  });

  it("rejects old-vault responses and releases presentation on a vault change", async () => {
    const session = createNotesDatabaseSession();
    const resource = databaseResource("templates", "source");
    let finish: (value: NotesDataSourceTemplate[]) => void = () => {};
    const pending = session.load(resource, () => new Promise((resolve) => { finish = resolve; }));
    const rejected = expect(pending).rejects.toThrow("session changed");
    await Promise.resolve();
    session.remember("database", { viewId: "board", scrollLeft: 120 });
    const epoch = session.epoch;
    session.clear();
    finish(templates());
    await rejected;
    expect(session.read(resource)).toBeNull();
    expect(session.recall("database")).toBeUndefined();
    session.write(resource, templates("Late mutation"), epoch);
    expect(session.read(resource)).toBeNull();
  });

  it("bounds residency by recent resources and serialized size", () => {
    const session = createNotesDatabaseSession(2, 1024);
    const first = databaseResource("templates", "first");
    const second = databaseResource("templates", "second");
    const third = databaseResource("templates", "third");
    session.write(first, templates());
    session.write(second, templates());
    session.read(first);
    session.write(third, templates());
    expect(session.read(first)).not.toBeNull();
    expect(session.read(second)).toBeNull();
    expect(session.read(third)).not.toBeNull();
    session.write(third, templates("x".repeat(1024)));
    expect(session.read(third)).toBeNull();
    expect(session.read(first)).not.toBeNull();
    for (const key of ["first", "second", "third"]) session.remember(key, { viewId: key, scrollLeft: 0 });
    expect(session.recall("first")).toBeUndefined();
  });

  it("separates view scopes and clears the resident singleton when the vault changes", () => {
    const first = databaseResource("views", "source", { databaseId: "first" });
    const second = databaseResource("views", "source", { databaseId: "second" });
    expect(first.key).not.toBe(second.key);
    setActiveVaultIdentity("first-vault");
    notesDatabaseSession.write(first, []);
    setActiveVaultIdentity("second-vault");
    expect(notesDatabaseSession.read(first)).toBeNull();
    setActiveVaultIdentity(null);
  });

  it("allows retry after a failed refresh while preserving the previous snapshot", async () => {
    const session = createNotesDatabaseSession();
    const resource = databaseResource("templates", "source");
    session.write(resource, templates());
    session.invalidate();
    await expect(session.load(resource, async () => { throw new Error("Offline"); })).rejects.toThrow("Offline");
    expect(session.read(resource)?.[0].name).toBe("Default");
    await session.load(resource, async () => templates("Retry"));
    expect(session.read(resource)?.[0].name).toBe("Retry");
  });
});
