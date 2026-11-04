import { loadProjectConflicts, type ProjectConflicts } from "$lib/api/project-conflicts";

/**
 * Sync conflicts of the projects the window shows. Views call {@link ensure} from an effect that also reads
 * {@link revision}, so they read again after {@link invalidate}, which marks every loaded read as stale.
 */
function createProjectConflictsStore() {
  let revision = $state(0);
  let reads = $state<Record<string, ProjectConflicts>>({});
  const loadedRevisions = new Map<string, number>();
  const inFlight = new Map<string, { revision: number; request: Promise<void> }>();

  async function load(projectId: string, requestedRevision: number): Promise<void> {
    try {
      const read = await loadProjectConflicts(projectId);
      if (requestedRevision < (loadedRevisions.get(projectId) ?? 0)) return;
      reads = { ...reads, [projectId]: read };
      loadedRevisions.set(projectId, requestedRevision);
    } catch (error: unknown) {
      console.warn("Projects conflicts could not be loaded", error);
    } finally {
      if (inFlight.get(projectId)?.revision === requestedRevision) inFlight.delete(projectId);
    }
  }

  return {
    /** Bumped by every {@link invalidate}. */
    get revision(): number {
      return revision;
    },
    /** The conflicts last read for a project, or undefined before the first read or when none were read. */
    read(projectId: string | null | undefined): ProjectConflicts | undefined {
      return projectId ? reads[projectId] : undefined;
    },
    /** Reads a project's conflicts unless they are current or being read. */
    ensure(projectId: string): Promise<void> {
      const current = revision;
      if (loadedRevisions.get(projectId) === current) return Promise.resolve();
      const pending = inFlight.get(projectId);
      if (pending?.revision === current) return pending.request;
      const request = load(projectId, current);
      inFlight.set(projectId, { revision: current, request });
      return request;
    },
    /** Marks every read as stale after a resolution or after sync changed rows. */
    invalidate(): void {
      revision += 1;
    },
  };
}

export const projectConflicts = createProjectConflictsStore();
