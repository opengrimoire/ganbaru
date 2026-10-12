import { getNotesDatabaseReference } from "$lib/api/notes";
import type { NotesBlock } from "$lib/notes/types";

interface DatabaseDeletionContext {
  readPageId: () => string | null;
  blockById: (id: string) => NotesBlock | undefined;
}

export interface NotesDatabaseDeletionPrompt {
  pageId: string;
  databaseIds: readonly string[];
  dataSourceCount: number;
  loading: boolean;
  error: string | null;
}

/** Confirm database ownership before any selected blocks are changed locally. */
export function createNotesDatabaseDeletionController(context: DatabaseDeletionContext) {
  let prompt = $state<NotesDatabaseDeletionPrompt | null>(null);
  let resolveDecision: ((confirmed: boolean) => void) | null = null;
  let generation = 0;

  function cancel(): void {
    generation += 1;
    resolveDecision?.(false);
    resolveDecision = null;
    prompt = null;
  }

  function confirm(): void {
    if (!prompt || prompt.loading || prompt.error) return;
    const valid = prompt.pageId === context.readPageId()
      && prompt.databaseIds.every((id) => context.blockById(id)?.type === "child_database");
    resolveDecision?.(valid);
    resolveDecision = null;
    prompt = null;
  }

  async function loadOwnership(current: NotesDatabaseDeletionPrompt, request: number): Promise<void> {
    try {
      const references = await Promise.all(current.databaseIds.map(getNotesDatabaseReference));
      if (generation !== request || prompt !== current) return;
      // Linked shells own no source, even when their original database is also selected.
      current.dataSourceCount = references.reduce((count, reference) => count + reference.owned_data_source_count, 0);
      current.error = null;
    } catch (caught: unknown) {
      if (generation !== request || prompt !== current) return;
      current.error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      if (generation === request && prompt === current) current.loading = false;
    }
  }

  async function request(blockIds: readonly string[]): Promise<boolean> {
    const pageId = context.readPageId();
    const databaseIds = [...new Set(blockIds.filter((id) => {
      const block = context.blockById(id);
      return block?.type === "child_database" && Boolean(block.child_database.database_id);
    }))];
    if (!pageId) return false;
    if (!databaseIds.length) return true;
    // A repeated keyboard delete keeps the original confirmation and selection.
    if (prompt) return false;
    prompt = { pageId, databaseIds, dataSourceCount: 0, loading: true, error: null };
    const current = prompt;
    const decision = new Promise<boolean>((resolve) => { resolveDecision = resolve; });
    void loadOwnership(current, ++generation);
    return decision;
  }

  function retry(): void {
    if (!prompt || prompt.loading) return;
    prompt.loading = true;
    prompt.error = null;
    void loadOwnership(prompt, ++generation);
  }

  return { get prompt() { return prompt; }, request, confirm, cancel, retry };
}

export type NotesDatabaseDeletionController = ReturnType<typeof createNotesDatabaseDeletionController>;
