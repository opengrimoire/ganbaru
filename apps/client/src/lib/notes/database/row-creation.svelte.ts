import {
  applyNotesDataSourceTemplate,
  createNotesDataSourceRowPage,
  createNotesDataSourceSubitem,
  loadNotesPage,
  updateNotesDataSourceRowProperty,
} from "$lib/api/notes";
import { createProvisionalNotesPage } from "$lib/notes/pages/creation";
import {
  notesDatabaseTableCellEditValue,
  notesDatabaseTableEditValuesEqual,
  type NotesDatabaseTableColumn,
  type NotesDatabaseTableEditValue,
} from "./table";
import type { NotesDataSourceRowPageCreateRequest, NotesPage } from "$lib/notes/types";
import type { NotesDatabaseRowHierarchy } from "./row-hierarchy";

interface RowEdit {
  value: NotesDatabaseTableEditValue;
  revision: number;
  submitted: boolean;
}

interface CreatedRow {
  sourceId: string;
  templateId: string;
  parentRowId: string | null;
  request: NotesDataSourceRowPageCreateRequest;
  page: NotesPage;
  persisted: boolean;
  pending: boolean;
  error: string | null;
  edits: Record<string, RowEdit>;
}

/** Show row pages immediately and serialize their edits behind creation. */
export function createNotesDatabaseRowCreation(onEdited: () => Promise<unknown>) {
  let rows = $state<CreatedRow[]>([]);
  let revision = 0;

  function find(pageId: string): CreatedRow | undefined {
    return rows.find((row) => row.page.id === pageId);
  }

  async function persist(row: CreatedRow): Promise<void> {
    if (row.pending) return;
    row.pending = true;
    row.error = null;
    let edited = false;
    try {
      if (!row.persisted) {
        let page: NotesPage;
        try {
          const loaded = row.parentRowId
            ? await createNotesDataSourceSubitem(row.sourceId, row.parentRowId, row.request)
            : row.templateId
            ? await applyNotesDataSourceTemplate(row.sourceId, row.templateId, {
                id: row.request.id, title: row.request.title,
              })
            : await createNotesDataSourceRowPage(row.sourceId, row.request);
          page = loaded.page;
        } catch (creationError) {
          // A failed response can follow a committed create. Recover the same ID.
          try {
            const recovered = await loadNotesPage(row.request.id);
            if (recovered.page.parent.type !== "data_source_id"
              || recovered.page.parent.data_source_id !== row.sourceId
              || recovered.page.in_trash || recovered.page.archived) throw creationError;
            page = recovered.page;
          } catch {
            throw creationError;
          }
        }
        row.page = page;
        row.persisted = true;
      }
      for (;;) {
        const next = Object.entries(row.edits).find(([, edit]) => edit.submitted);
        if (!next) break;
        const [propertyId, edit] = next;
        row.page = await updateNotesDataSourceRowProperty(row.sourceId, row.page.id, {
          property_id: propertyId, value: edit.value,
        });
        if (row.edits[propertyId]?.revision === edit.revision) delete row.edits[propertyId];
        edited = true;
      }
    } catch (caught) {
      row.error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      row.pending = false;
    }
    if (edited && !row.error) {
      try {
        await onEdited();
      } catch (caught) {
        row.error = caught instanceof Error ? caught.message : String(caught);
      }
    }
  }

  function begin(sourceId: string, templateId = "", initialValues: readonly { column: NotesDatabaseTableColumn; value: NotesDatabaseTableEditValue; payload: unknown }[] = [], parentRowId: string | null = null): string {
    const request = { id: crypto.randomUUID(), first_block_id: crypto.randomUUID(), title: "" };
    const provisional = createProvisionalNotesPage({
      ...request, parent: { type: "data_source_id", data_source_id: sourceId }, folder_id: null,
    });
    const edits: Record<string, RowEdit> = {};
    for (const { column, value, payload } of initialValues) {
      provisional.page.properties = { ...provisional.page.properties, [column.name]: { id: column.id, type: column.type, [column.type]: payload } };
      edits[column.id] = { value, revision: ++revision, submitted: true };
    }
    rows.push({ sourceId, templateId, parentRowId, request, page: provisional.page,
      persisted: false, pending: false, error: null, edits });
    const row = find(request.id);
    if (row) void persist(row);
    return request.id;
  }

  /** Keep an active title draft intact when creation or another edit returns. */
  function draft(pageId: string, propertyId: string, value: NotesDatabaseTableEditValue): void {
    const row = find(pageId);
    if (row) row.edits[propertyId] = { value, revision: ++revision, submitted: false };
  }

  function submit(pageId: string, column: NotesDatabaseTableColumn, value: NotesDatabaseTableEditValue): boolean {
    const row = find(pageId);
    if (!row) return false;
    const edit = row.edits[column.id];
    if (edit?.submitted && notesDatabaseTableEditValuesEqual(edit.value, value)) return true;
    if (!edit && notesDatabaseTableEditValuesEqual(notesDatabaseTableCellEditValue(row.page, column), value)) return true;
    row.edits[column.id] = { value, revision: ++revision, submitted: true };
    if (!row.error) void persist(row);
    return true;
  }

  /** Retire clean rows only on a canonical refresh, leaving active and failed drafts visible. */
  function acceptWindow(sourceId: string, focusedId: string | null, settledIds: ReadonlySet<string>): void {
    rows = rows.filter((row) => row.sourceId !== sourceId || !settledIds.has(row.page.id) || !row.persisted || row.pending
      || row.error !== null || Object.keys(row.edits).length > 0 || row.page.id === focusedId);
  }

  return {
    begin, draft, submit, acceptWindow,
    settledIds(sourceId: string): ReadonlySet<string> {
      return new Set(rows.filter((row) => row.sourceId === sourceId && row.persisted
        && !row.pending && !row.error && !Object.keys(row.edits).length).map((row) => row.page.id));
    },
    isSaving(sourceId: string): boolean { return rows.some((row) => row.sourceId === sourceId && row.pending); },
    rowsFor(sourceId: string, canonical: NotesPage[]): NotesPage[] {
      const local = rows.filter((row) => row.sourceId === sourceId);
      const localIds = new Set(local.map((row) => row.page.id));
      return [...canonical.filter((page) => !localIds.has(page.id)), ...local.map((row) => row.page)];
    },
    /** Preserve optimistic sub-item placement while creation or its title draft is pending. */
    hierarchyFor(sourceId: string, canonical: NotesDatabaseRowHierarchy): NotesDatabaseRowHierarchy {
      const result = { ...canonical };
      for (const row of rows.filter((row) => row.sourceId === sourceId && row.parentRowId && !result[row.page.id])) {
        const parentId = row.parentRowId;
        if (!parentId) continue;
        const ancestors = [parentId, ...(result[parentId]?.ancestor_row_page_ids ?? [])];
        result[row.page.id] = { parent_row_page_id: parentId, ancestor_row_page_ids: ancestors, depth: ancestors.length, child_count: 0 };
        const parent = result[parentId];
        if (parent && !row.persisted) result[parentId] = { ...parent, child_count: parent.child_count + 1 };
      }
      return result;
    },
    /** Keep a retained local row's parent current after a canonical move. */
    acceptParent(pageId: string, parentRowId: string | null): void {
      const row = find(pageId);
      if (row) row.parentRowId = parentRowId;
    },
    valueFor(page: NotesPage, column: NotesDatabaseTableColumn): NotesDatabaseTableEditValue {
      const edit = find(page.id)?.edits[column.id];
      return edit ? edit.value : notesDatabaseTableCellEditValue(page, column);
    },
    blocked(pageId: string): boolean {
      const row = find(pageId);
      return Boolean(row && (!row.persisted || row.pending || row.error || Object.keys(row.edits).length));
    },
    errorFor(pageId: string): string | null { return find(pageId)?.error ?? null; },
    retry(pageId: string): void {
      const row = find(pageId);
      if (row?.error) void persist(row);
    },
    /** Submit titles when the view closes, including a create still in flight. */
    flush(): void {
      for (const row of rows) {
        for (const edit of Object.values(row.edits)) edit.submitted = true;
        if (!row.error && Object.keys(row.edits).length) void persist(row);
      }
    },
  };
}
