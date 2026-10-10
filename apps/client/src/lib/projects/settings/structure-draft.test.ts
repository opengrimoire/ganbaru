import { describe, expect, it, vi } from "vitest";
import { createProjectSettingsStructureDraft } from "./structure-draft.svelte";
import { createProjectSettingsSession } from "./session.svelte";
import type { ProjectSettingsSessionCollections } from "./session.svelte";
import type { getProjects } from "$lib/stores/projects.svelte";

/** Provide ordered canonical fixtures for each structurally editable settings collection. */
function collections(): ProjectSettingsSessionCollections {
  const common = { projectId: "project", createdAt: "2026-01-01T00:00:00Z", updatedAt: "2026-01-01T00:00:00Z" };
  return {
    statuses: ["a", "b"].map((id, index) => ({ ...common, id, name: id, sortOrder: (index + 1) * 1000, category: "active", terminal: false, color: 8 })),
    priorities: ["low", "high"].map((id, index) => ({ ...common, id, name: id, sortOrder: (index + 1) * 1000, color: 8 })),
    tags: ["first", "second"].map((id, index) => ({ ...common, id, name: id, sortOrder: (index + 1) * 1000, color: 8 })),
    customFields: ["field", "other"].map((id, index) => ({ ...common, id, name: id, sortOrder: (index + 1) * 1000, fieldType: "select", icon: null })),
    optionsForField: (fieldId) => ["one", "two"].map((id, index) => ({ ...common, id: `${fieldId}-${id}`, fieldId, name: id, sortOrder: (index + 1) * 1000 })),
  };
}

/** Observe persistence without requiring Tauri or the canonical project store. */
function setup() {
  const ports = {
    addStatus: vi.fn(async () => {}), updateStatus: vi.fn(async () => {}), removeStatus: vi.fn(async () => {}),
    addPriority: vi.fn(async () => {}), updatePriority: vi.fn(async () => {}), removePriority: vi.fn(async () => {}),
    addTag: vi.fn(async () => undefined), updateTag: vi.fn(async () => {}), removeTag: vi.fn(async () => {}),
    updateCustomField: vi.fn(async () => {}), removeCustomField: vi.fn(async () => {}),
    updateCustomFieldOption: vi.fn(async () => {}), removeCustomFieldOption: vi.fn(async () => {}),
  };
  const draft = createProjectSettingsStructureDraft(ports);
  const canonical = collections();
  draft.load(canonical);
  const session = createProjectSettingsSession({ projects: {} as ReturnType<typeof getProjects>, translate: (() => "error") as Parameters<typeof createProjectSettingsSession>[0]["translate"], onRevealInactive: () => {} });
  return { draft, ports, canonical, state: session.state };
}

describe("Project settings structural drafts", () => {
  it("keeps additions, deletions and ordering local until Save, and restores them on Discard", async () => {
    const { draft, ports, canonical } = setup();
    draft.addStatus("project", "New", "active", 8);
    draft.addPriority("project", "Medium", 8);
    draft.addTag("project", "New tag", 8);
    draft.removeStatus("a"); draft.removePriority("low"); draft.removeTag("first");
    await draft.moveCustomField(draft.customFields[1], -1);
    await draft.moveCustomFieldOption(draft.optionsForField("field")[1], -1);
    draft.removeCustomFieldOption("other-one");
    expect(draft.dirty).toBe(true);
    for (const spy of Object.values(ports)) expect(spy).not.toHaveBeenCalled();
    expect(canonical.statuses.map((entry) => entry.id)).toEqual(["a", "b"]);
    draft.load(canonical);
    expect(draft.dirty).toBe(false);
    expect(draft.statuses).toEqual(canonical.statuses);
    expect(draft.optionsForField("other")).toHaveLength(2);
  });

  it("creates stable draft identities before deleting the old last status or priority", async () => {
    const { draft, ports, state } = setup();
    draft.addStatus("project", "New", "done", 8);
    const created = draft.statuses.at(-1)!;
    draft.removeStatus("a"); draft.removeStatus("b");
    await draft.save(state);
    expect(ports.addStatus).toHaveBeenCalledWith("project", "New", "done", 8, expect.objectContaining({ id: created.id }));
    expect(ports.addStatus.mock.invocationCallOrder[0]).toBeLessThan(ports.removeStatus.mock.invocationCallOrder[0]);
    expect(draft.dirty).toBe(false);
  });

  it("updates the draft's saved ranks so later name saves preserve reordered positions", async () => {
    const { draft, ports, state } = setup();
    await draft.moveStatus(draft.statuses[1], -1);
    await draft.moveCustomFieldOption(draft.optionsForField("field")[1], -1);
    const nameSaveEntry = draft.statuses[0];
    await draft.save(state);
    expect(ports.updateStatus).toHaveBeenCalledWith(expect.objectContaining({ id: "b" }), { sortOrder: 1 });
    expect(nameSaveEntry.sortOrder).toBe(1);
    expect(draft.optionsForField("field").map((entry) => [entry.id, entry.sortOrder])).toEqual([["field-two", 1], ["field-one", 2]]);
    expect(draft.dirty).toBe(false);
  });

  it("resumes after a failure without recreating an already saved row", async () => {
    const { draft, ports, state } = setup();
    draft.addStatus("project", "New", "active", 8);
    draft.removeTag("first");
    ports.removeTag.mockRejectedValueOnce(new Error("Tag removal failed"));
    await expect(draft.save(state)).rejects.toThrow("Tag removal failed");
    expect(draft.dirty).toBe(true);
    await draft.save(state);
    expect(ports.addStatus).toHaveBeenCalledOnce();
    expect(ports.removeTag).toHaveBeenCalledTimes(2);
    expect(draft.dirty).toBe(false);
  });

  it("removes a deleted tag before creating its same-name replacement", async () => {
    const { draft, ports, state } = setup();
    draft.removeTag("first");
    draft.addTag("project", "first", 8);
    await draft.save(state);
    expect(ports.removeTag.mock.invocationCallOrder[0]).toBeLessThan(ports.addTag.mock.invocationCallOrder[0]);
    expect(draft.dirty).toBe(false);
  });

  it("clears dirty when draft-only rows are removed and ordering is reversed", async () => {
    const { draft } = setup();
    draft.addTag("project", "New", 8);
    draft.removeTag(draft.tags.at(-1)!.id);
    await draft.movePriority(draft.priorities[1], -1);
    await draft.movePriority(draft.priorities[0], 1);
    expect(draft.dirty).toBe(false);
  });
});
