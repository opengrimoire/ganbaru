import { describe, expect, it, vi } from "vitest";
import type { getLocalization } from "$lib/i18n/translator.svelte";
import type { getProjects } from "$lib/stores/projects.svelte";
import { createProjectSettingsCustomFieldController } from "./custom-field-controller.svelte";
import { createProjectSettingsSession } from "./session.svelte";
import type { ProjectCustomField, ProjectCustomFieldOption } from "$lib/projects/types";

const field: ProjectCustomField = {
  id: "field-1", projectId: "project-1", name: "Stage", fieldType: "select",
  sortOrder: 1000, createdAt: "2026-01-01T00:00:00Z", updatedAt: "2026-01-01T00:00:00Z", icon: null,
};
const option: ProjectCustomFieldOption = {
  id: "option-1", fieldId: field.id, name: "Ready", sortOrder: 1000,
  createdAt: "2026-01-01T00:00:00Z", updatedAt: "2026-01-01T00:00:00Z",
};

function translate(): ReturnType<typeof getLocalization>["t"] {
  return ((key: string, ...args: Array<string | number>) => [key, ...args].join(":")) as ReturnType<typeof getLocalization>["t"];
}

function setup(overrides: Record<string, unknown> = {}) {
  const collections = {
    optionsForField: () => [option],
    removeCustomField: vi.fn(() => undefined),
    removeCustomFieldOption: vi.fn(() => undefined),
    ...overrides,
  };
  const session = createProjectSettingsSession({ projects: {} as ReturnType<typeof getProjects>, translate: translate(), onRevealInactive: () => undefined });
  session.state.customFieldNameDrafts = { [field.id]: field.name };
  session.state.customFieldOptionNameDrafts = { [option.id]: option.name };
  let id = 0;
  const controller = createProjectSettingsCustomFieldController({
    state: session.state, fields: () => [field], collections, translate: translate(),
    setDraftError: session.setCustomFieldDraftError, afterCreateDraft: () => undefined,
    createId: () => `draft-${++id}`,
  });
  return { controller, session, collections };
}

describe("project settings custom field controller", () => {
  it("rejects conflicting stored and staged option names case-insensitively", () => {
    const { controller, session } = setup();
    session.state.newCustomFieldOptionDrafts[field.id] = " ready ";
    controller.submitOption(field);
    expect(session.state.customFieldOptionDraftRowsByField[field.id]).toBeUndefined();
    expect(session.state.projectSettingsError).toBe("projects.customFields.optionNameExists");

    session.state.newCustomFieldOptionDrafts[field.id] = "Review";
    controller.submitOption(field);
    session.state.newCustomFieldOptionDrafts[field.id] = "review";
    controller.submitOption(field);
    expect(session.state.customFieldOptionDraftRowsByField[field.id]).toHaveLength(1);
    expect(session.state.projectSettingsError).toBe("projects.customFields.optionNameExists");
  });

  it("keeps option drafts when staging deletion is rejected", async () => {
    const { controller, session } = setup({
      removeCustomFieldOption: vi.fn(() => { throw new Error("option is used by tasks"); }),
    });
    session.state.customFieldOptionNameDrafts[option.id] = "Renamed";
    controller.requestDeleteOption(option);
    await controller.removeOption();
    expect(session.state.customFieldOptionNameDrafts[option.id]).toBe("Renamed");
    expect(session.state.projectSettingsError).toContain("option is used by tasks");
  });

  it("cleans every field-scoped draft when deletion is staged", async () => {
    const { controller, session, collections } = setup();
    session.state.customFieldNameDrafts[field.id] = "Renamed";
    session.state.newCustomFieldOptionDrafts[field.id] = "Pending";
    session.state.customFieldOptionDraftRowsByField[field.id] = [{ id: "draft", name: "Draft" }];
    controller.requestDeleteField(field);
    await controller.removeField();
    expect(collections.removeCustomField).toHaveBeenCalledWith(field.id);
    expect(session.state.customFieldNameDrafts[field.id]).toBeUndefined();
    expect(session.state.newCustomFieldOptionDrafts[field.id]).toBeUndefined();
    expect(session.state.customFieldOptionDraftRowsByField[field.id]).toBeUndefined();
  });
});
