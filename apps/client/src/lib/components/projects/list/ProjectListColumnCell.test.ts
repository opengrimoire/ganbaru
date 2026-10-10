// @vitest-environment jsdom

import { mount, tick, unmount, type ComponentProps } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import ProjectListColumnCell from "./ProjectListColumnCell.svelte";
import ProjectListNumberCellHarness from "./ProjectListNumberCellHarness.test.svelte";

vi.mock("$lib/stores/theme.svelte", async () => {
  const { lightTheme } = await import("$lib/themes");
  return { getTheme: () => ({ current: lightTheme }) };
});

type CellProps = ComponentProps<typeof ProjectListColumnCell>;
let component: ReturnType<typeof mount> | undefined;

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
});

/** Mount the real numeric editor with a canonical value that changes only after successful writes. */
function renderNumber() {
  const field: CellProps["projectCustomFields"][number] = {id: "points", projectId: "project", name: "Points", fieldType: "number", sortOrder: 100, icon: null, createdAt: "", updatedAt: ""};
  const save = vi.fn<CellProps["onSaveCustomFieldValue"]>().mockResolvedValue(undefined);
  const noop = () => undefined;
  const props: CellProps = {
    column: "custom:points",
    task: {id: "task", projectId: "project", sectionId: "section", statusId: "status", title: "Task", description: "", priority: "none", taskType: "task", sectionSortOrder: 0, statusSortOrder: 0, milestone: false, createdAt: "", updatedAt: ""},
    status: undefined, statuses: [], priorities: [], statusMenuOpen: false, priorityMenuOpen: false, startDateMenuOpen: false, dueDateMenuOpen: false,
    projectCustomFields: [field], scheduled: null, blockedByCount: 0, blocksCount: 0,
    estimateLabel: String, customFieldDisplayValue: () => "1234.5", customFieldOptions: () => [], customFieldOptionValues: () => [],
    customFieldValue: () => ({taskId: "task", fieldId: "points", numberValue: 1234.5, updatedAt: ""}),
    onSaveCustomFieldValue: save, onToggleStatusMenu: noop, onSetStatus: noop, onTogglePriorityMenu: noop, onSetPriority: noop,
    onToggleStartDateMenu: noop, onCloseStartDateMenu: noop, onSetStartDate: noop, onClearStartDate: noop, onSetStartTime: noop, onClearStartTime: noop,
    onToggleDueDateMenu: noop, onCloseDueDateMenu: noop, onSetDueDate: noop, onClearDueDate: noop, onSetDueTime: noop, onClearDueTime: noop,
  };
  component = mount(ProjectListNumberCellHarness, {target: document.body, props: {props}});
  const input = document.querySelector<HTMLInputElement>("input")!;
  return {input, save};
}

/** Enter a scalar using the same input event as a user edit. */
async function enter(input: HTMLInputElement, value: string): Promise<void> {
  input.value = value;
  input.dispatchEvent(new Event("input", {bubbles: true}));
  await tick();
}

describe("Projects numeric cell drafts", () => {
  it("keeps failed and invalid corrections when refocusing and commits a later retry", async () => {
    const {input, save} = renderNumber();
    save.mockRejectedValueOnce(new Error("Disk full"));
    input.focus();
    await tick();
    await enter(input, "27.5");
    input.blur();
    await vi.waitFor(() => expect(document.querySelector('[role="alert"]')?.textContent).toContain("Disk full"));
    expect(input.value).toBe("27.5");
    input.focus();
    await tick();
    expect(input.value).toBe("27.5");
    await enter(input, "invalid");
    input.blur();
    await vi.waitFor(() => expect(input.validity.valid).toBe(false));
    expect(save).toHaveBeenCalledOnce();
    input.focus();
    await tick();
    expect(input.value).toBe("invalid");
    await enter(input, "31.5");
    input.blur();
    await vi.waitFor(() => expect(input.disabled).toBe(false));
    expect(save).toHaveBeenCalledTimes(2);
    expect(save).toHaveBeenLastCalledWith(expect.objectContaining({id: "task"}), expect.objectContaining({id: "points"}), expect.objectContaining({numberValue: 31.5}));
    expect(input.value).toBe("31.5");
    expect(document.querySelector('[role="alert"]')).toBeNull();
  });

  it("blocks refocusing while a blur write is pending and unlocks the committed value", async () => {
    const {input, save} = renderNumber();
    let finish!: () => void;
    save.mockImplementationOnce(() => new Promise<void>((resolve) => { finish = resolve; }));
    input.focus();
    await tick();
    await enter(input, "45.5");
    input.blur();
    await vi.waitFor(() => expect(input.disabled).toBe(true));
    input.focus();
    expect(document.activeElement).not.toBe(input);
    expect(input.value).toBe("45.5");
    expect(save).toHaveBeenCalledOnce();
    finish();
    await vi.waitFor(() => expect(input.disabled).toBe(false));
    expect(input.value).toBe("45.5");
    input.focus();
    await tick();
    expect(input.value).toBe("45.5");
  });
});
