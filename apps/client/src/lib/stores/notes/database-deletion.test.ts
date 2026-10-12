import { beforeEach, describe, expect, it, vi } from "vitest";
import { createBlockWrite } from "$lib/notes/blocks/factory";
import type { NotesBlock, NotesDatabaseReference } from "$lib/notes/types";
import { createNotesDatabaseDeletionController } from "./database-deletion.svelte";

const api = vi.hoisted(() => ({ getNotesDatabaseReference: vi.fn<(id: string) => Promise<NotesDatabaseReference>>() }));
vi.mock("$lib/api/notes", () => api);

/** Build a live database block without materializing its rows. */
function database(id: string): NotesBlock {
  return { ...createBlockWrite(id, "child_database", "Tasks"), object: "block", parent: { type: "page_id", page_id: "page" },
    created_time: "", last_edited_time: "", has_children: false, in_trash: false,
    source_provider: null, source_object_id: null, source_last_edited_time: null,
    type: "child_database", child_database: { title: "Tasks", database_id: id } };
}

function reference(id: string, count: number): NotesDatabaseReference {
  return { block_id: id, page_id: "page", source_block_id: "original", source_page_id: "source-page",
    title: "Tasks", owned_data_source_count: count, is_linked: count === 0, editing_locked: false };
}

describe("Notes database deletion confirmation", () => {
  beforeEach(() => api.getNotesDatabaseReference.mockReset());

  it("counts only owned sources and keeps linked views safe", async () => {
    const blocks = new Map(["owner", "linked"].map((id) => [id, database(id)]));
    api.getNotesDatabaseReference.mockImplementation(async (id) => reference(id, id === "owner" ? 2 : 0));
    const controller = createNotesDatabaseDeletionController({ readPageId: () => "page", blockById: (id) => blocks.get(id) });
    const decision = controller.request(["owner", "linked", "owner"]);
    await vi.waitFor(() => expect(controller.prompt?.loading).toBe(false));
    expect(controller.prompt?.dataSourceCount).toBe(2);
    expect(api.getNotesDatabaseReference).toHaveBeenCalledTimes(2);
    controller.confirm();
    await expect(decision).resolves.toBe(true);
    const linked = controller.request(["linked"]);
    await vi.waitFor(() => expect(controller.prompt?.loading).toBe(false));
    expect(controller.prompt?.dataSourceCount).toBe(0);
    controller.cancel();
    await expect(linked).resolves.toBe(false);
  });

  it("cancels without a decision when navigation changes and ignores a late metadata response", async () => {
    let page = "page";
    let resolve: (value: NotesDatabaseReference) => void = () => {};
    api.getNotesDatabaseReference.mockReturnValue(new Promise((ready) => { resolve = ready; }));
    const controller = createNotesDatabaseDeletionController({ readPageId: () => page, blockById: () => database("owner") });
    const decision = controller.request(["owner"]);
    controller.confirm();
    expect(controller.prompt?.loading).toBe(true);
    page = "another-page";
    controller.cancel();
    await expect(decision).resolves.toBe(false);
    resolve(reference("owner", 1));
    await Promise.resolve();
    expect(controller.prompt).toBeNull();
  });

  it("requires a successful ownership read before confirmation and supports retry", async () => {
    api.getNotesDatabaseReference.mockRejectedValueOnce(new Error("Unavailable")).mockResolvedValue(reference("owner", 1));
    const controller = createNotesDatabaseDeletionController({ readPageId: () => "page", blockById: () => database("owner") });
    const decision = controller.request(["owner"]);
    await vi.waitFor(() => expect(controller.prompt?.error).toBe("Unavailable"));
    controller.confirm();
    expect(controller.prompt).not.toBeNull();
    controller.retry();
    await vi.waitFor(() => expect(controller.prompt?.loading).toBe(false));
    expect(controller.prompt?.error).toBeNull();
    controller.confirm();
    await expect(decision).resolves.toBe(true);
  });
});
