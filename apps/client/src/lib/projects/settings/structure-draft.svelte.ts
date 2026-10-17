import type { EventColor } from "$lib/calendar/types";
import type { getProjects } from "$lib/stores/projects.svelte";
import type { ProjectSettingsSessionCollections, ProjectSettingsSessionState } from "./session.svelte";
import type { ProjectCustomField, ProjectCustomFieldOption, ProjectPriorityConfig, ProjectStatus, ProjectStatusCategory, ProjectTag } from "$lib/projects/types";

interface DraftCollections {
  statuses: ProjectStatus[];
  priorities: ProjectPriorityConfig[];
  tags: ProjectTag[];
  customFields: ProjectCustomField[];
  customFieldOptions: ProjectCustomFieldOption[];
}

type OrderedEntry = { id: string; sortOrder: number };

/** Compare visible identity order independently of editable labels and persisted rank values. */
function sameOrder(left: readonly OrderedEntry[], right: readonly OrderedEntry[]): boolean {
  return left.length === right.length && left.every((entry, index) => entry.id === right[index]?.id);
}

/** Move one draft row without mutating canonical store objects. */
function moved<T extends OrderedEntry>(entries: readonly T[], entry: T, direction: -1 | 1): T[] {
  const index = entries.findIndex((candidate) => candidate.id === entry.id);
  const target = index + direction;
  if (index < 0 || target < 0 || target >= entries.length) return [...entries];
  const result = [...entries];
  const [removed] = result.splice(index, 1);
  result.splice(target, 0, removed);
  return result;
}

/** Own collection additions, removals, and ordering until the settings session commits. */
export function createProjectSettingsStructureDraft(projects: Pick<ReturnType<typeof getProjects>,
  "addStatus" | "updateStatus" | "removeStatus" | "addPriority" | "updatePriority" | "removePriority"
  | "addTag" | "updateTag" | "removeTag" | "updateCustomField" | "removeCustomField"
  | "updateCustomFieldOption" | "removeCustomFieldOption">) {
  let rows = $state<DraftCollections>({ statuses: [], priorities: [], tags: [], customFields: [], customFieldOptions: [] });
  let saved = $state<DraftCollections>({ statuses: [], priorities: [], tags: [], customFields: [], customFieldOptions: [] });

  /** Snapshot canonical rows when opening or discarding the panel. */
  function load(collections: ProjectSettingsSessionCollections): void {
    rows = {
      statuses: collections.statuses.map((entry) => ({ ...entry })),
      priorities: collections.priorities.map((entry) => ({ ...entry })),
      tags: collections.tags.map((entry) => ({ ...entry })),
      customFields: collections.customFields.map((entry) => ({ ...entry })),
      customFieldOptions: collections.customFields.flatMap((field) => collections.optionsForField(field.id).map((entry) => ({ ...entry }))),
    };
    saved = {
      statuses: rows.statuses.map((entry) => ({ ...entry })),
      priorities: rows.priorities.map((entry) => ({ ...entry })),
      tags: rows.tags.map((entry) => ({ ...entry })),
      customFields: rows.customFields.map((entry) => ({ ...entry })),
      customFieldOptions: rows.customFieldOptions.map((entry) => ({ ...entry })),
    };
  }

  /** Allocate a stable draft identity so a failed Save can safely retry creation. */
  function identity(entries: readonly OrderedEntry[]) {
    const now = new Date().toISOString();
    return { id: crypto.randomUUID(), sortOrder: Math.max(0, ...entries.map((entry) => entry.sortOrder)) + 1, createdAt: now, updatedAt: now };
  }

  /** Stage a new status without creating a database row. */
  function addStatus(projectId: string, name: string, category: ProjectStatusCategory, color: EventColor): void {
    rows.statuses = [...rows.statuses, { ...identity(rows.statuses), projectId, name, category, color, terminal: category === "done" }];
  }

  /** Stage a new priority without creating a database row. */
  function addPriority(projectId: string, name: string, color: EventColor): void {
    rows.priorities = [...rows.priorities, { ...identity(rows.priorities), projectId, name, color }];
  }

  /** Stage a new tag without creating a database row. */
  function addTag(projectId: string, name: string, color: EventColor): void {
    rows.tags = [...rows.tags, { ...identity(rows.tags), projectId, name, color }];
  }

  /** Remove a field and its options from the draft while retaining canonical data. */
  function removeCustomField(id: string): void {
    rows.customFields = rows.customFields.filter((entry) => entry.id !== id);
    rows.customFieldOptions = rows.customFieldOptions.filter((entry) => entry.fieldId !== id);
  }

  /** Reorder options only within their owning field. */
  async function moveCustomFieldOption(option: ProjectCustomFieldOption, direction: -1 | 1): Promise<void> {
    const siblings = rows.customFieldOptions.filter((entry) => entry.fieldId === option.fieldId);
    rows.customFieldOptions = [
      ...rows.customFieldOptions.filter((entry) => entry.fieldId !== option.fieldId),
      ...moved(siblings, option, direction),
    ];
  }

  /** Persist one collection, acknowledging each successful operation before retryable failures. */
  async function commit<T extends OrderedEntry>(
    current: readonly T[], baseline: readonly T[], acknowledge: (entries: T[]) => void,
    create: ((entry: T) => Promise<void>) | null,
    updateOrder: (entry: T, sortOrder: number) => Promise<void>,
    remove: (entry: T) => Promise<void>,
    createBeforeRemove = false,
  ): Promise<void> {
    let completed = [...baseline];
    const createEntries = async () => {
      for (const entry of current) {
        if (completed.some((candidate) => candidate.id === entry.id)) continue;
        if (!create) throw new Error("Settings draft contains an unsupported collection addition");
        await create(entry);
        completed = [...completed, { ...entry }];
        acknowledge(completed);
      }
    };
    const removeEntries = async () => {
      for (const entry of baseline) {
        if (current.some((candidate) => candidate.id === entry.id)) continue;
        await remove(entry);
        completed = completed.filter((candidate) => candidate.id !== entry.id);
        acknowledge(completed);
      }
    };
    if (createBeforeRemove) { await createEntries(); await removeEntries(); }
    else { await removeEntries(); await createEntries(); }
    if (!sameOrder(current, completed)) {
      for (const [index, entry] of current.entries()) {
        const rank = index + 1;
        const original = completed.find((candidate) => candidate.id === entry.id);
        if (!original || original.sortOrder === rank) continue;
        await updateOrder(original, rank);
        entry.sortOrder = rank;
        completed = completed.map((candidate) => candidate.id === entry.id ? { ...candidate, sortOrder: rank } : candidate);
        acknowledge(completed);
      }
      acknowledge(current.map((entry, index) => ({ ...entry, sortOrder: index + 1 })));
    }
  }

  /** Apply structural changes only from the explicit Save settings handler. */
  async function save(state: ProjectSettingsSessionState): Promise<void> {
    await commit(rows.statuses, saved.statuses, (entries) => { saved.statuses = entries; },
      (entry) => projects.addStatus(entry.projectId, state.statusNameDrafts[entry.id] ?? entry.name,
        state.statusCategoryDrafts[entry.id] ?? entry.category, state.statusColorDrafts[entry.id] ?? entry.color, entry),
      (entry, sortOrder) => projects.updateStatus(entry, { sortOrder }),
      (entry) => projects.removeStatus(entry.id), true);
    await commit(rows.priorities, saved.priorities, (entries) => { saved.priorities = entries; },
      (entry) => projects.addPriority(entry.projectId, state.priorityNameDrafts[entry.id] ?? entry.name,
        state.priorityColorDrafts[entry.id] ?? entry.color, entry),
      (entry, sortOrder) => projects.updatePriority(entry, { sortOrder }),
      (entry) => projects.removePriority(entry), true);
    await commit(rows.tags, saved.tags, (entries) => { saved.tags = entries; },
      async (entry) => { await projects.addTag(entry.projectId, state.tagNameDrafts[entry.id] ?? entry.name,
        state.tagColorDrafts[entry.id] ?? entry.color, entry); },
      (entry, sortOrder) => projects.updateTag(entry, { sortOrder }),
      (entry) => projects.removeTag(entry.id));
    await commit(rows.customFields, saved.customFields, (entries) => { saved.customFields = entries; }, null,
      (entry, sortOrder) => projects.updateCustomField(entry, { sortOrder }),
      (entry) => projects.removeCustomField(entry.id));
    for (const field of rows.customFields) {
      const siblings = rows.customFieldOptions.filter((entry) => entry.fieldId === field.id);
      const original = saved.customFieldOptions.filter((entry) => entry.fieldId === field.id);
      await commit(siblings, original, (entries) => {
        saved.customFieldOptions = [...saved.customFieldOptions.filter((entry) => entry.fieldId !== field.id), ...entries];
      }, null, (entry, sortOrder) => projects.updateCustomFieldOption(entry, { sortOrder }),
      (entry) => projects.removeCustomFieldOption(entry.id));
    }
    saved.customFieldOptions = saved.customFieldOptions.filter((option) => rows.customFields.some((field) => field.id === option.fieldId));
  }

  return {
    load, save, addStatus, addPriority, addTag, removeCustomField, moveCustomFieldOption,
    get statuses() { return rows.statuses; },
    get priorities() { return rows.priorities; },
    get tags() { return rows.tags; },
    get customFields() { return rows.customFields; },
    optionsForField: (id: string) => rows.customFieldOptions.filter((entry) => entry.fieldId === id),
    get dirty() {
      return !sameOrder(rows.statuses, saved.statuses) || !sameOrder(rows.priorities, saved.priorities)
        || !sameOrder(rows.tags, saved.tags) || !sameOrder(rows.customFields, saved.customFields)
        || rows.customFields.some((field) => !sameOrder(rows.customFieldOptions.filter((entry) => entry.fieldId === field.id),
          saved.customFieldOptions.filter((entry) => entry.fieldId === field.id)));
    },
    removeStatus: (id: string) => { rows.statuses = rows.statuses.filter((entry) => entry.id !== id); },
    removePriority: (id: string) => { rows.priorities = rows.priorities.filter((entry) => entry.id !== id); },
    removeTag: (id: string) => { rows.tags = rows.tags.filter((entry) => entry.id !== id); },
    removeCustomFieldOption: (id: string) => { rows.customFieldOptions = rows.customFieldOptions.filter((entry) => entry.id !== id); },
    moveStatus: async (entry: ProjectStatus, direction: -1 | 1) => { rows.statuses = moved(rows.statuses, entry, direction); },
    movePriority: async (entry: ProjectPriorityConfig, direction: -1 | 1) => { rows.priorities = moved(rows.priorities, entry, direction); },
    moveTag: async (entry: ProjectTag, direction: -1 | 1) => { rows.tags = moved(rows.tags, entry, direction); },
    moveCustomField: async (entry: ProjectCustomField, direction: -1 | 1) => { rows.customFields = moved(rows.customFields, entry, direction); },
  };
}
