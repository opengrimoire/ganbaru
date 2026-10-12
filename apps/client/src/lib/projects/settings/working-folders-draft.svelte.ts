import type {
  ChatProjectPrimaryWorkingFolderRead,
  CreateProjectWorkingFolderRequest,
  ProjectWorkingFolderRead,
  ProjectWorkingFolderSelectionRead,
} from "$lib/chat/contracts";

export type WorkingFolderSelection = ProjectWorkingFolderSelectionRead;

export interface ProjectSettingsWorkingFoldersPorts {
  pick: (workingFolderId?: string) => Promise<WorkingFolderSelection | null>;
  add: (request: CreateProjectWorkingFolderRequest, selection: WorkingFolderSelection) => Promise<ProjectWorkingFolderRead>;
  bind: (id: string, selection: WorkingFolderSelection) => Promise<ProjectWorkingFolderRead>;
  rename: (id: string, name: string, revision: number) => Promise<ProjectWorkingFolderRead>;
  archive: (id: string, revision: number) => Promise<ProjectWorkingFolderRead>;
  restore: (id: string, revision: number) => Promise<ProjectWorkingFolderRead>;
  recreate: (id: string) => Promise<ProjectWorkingFolderRead>;
  remove: (id: string) => Promise<void>;
  provider: (id: string, instanceId: string | null) => Promise<void>;
  primary: (projectId: string, id: string, revision: number) => Promise<ChatProjectPrimaryWorkingFolderRead>;
}

/** Clone folder reads so draft edits never mutate the Chat store. */
function copy(folder: ProjectWorkingFolderRead): ProjectWorkingFolderRead {
  return { ...folder, workingFolder: { ...folder.workingFolder } };
}

/** Stage folder configuration and native selections until the panel explicitly saves. */
export function createProjectSettingsWorkingFoldersDraft(ports: ProjectSettingsWorkingFoldersPorts) {
  let projectId = "";
  let generation = 0;
  let rows = $state<ProjectWorkingFolderRead[]>([]);
  let saved = $state<ProjectWorkingFolderRead[]>([]);
  let primary = $state<ChatProjectPrimaryWorkingFolderRead | null>(null);
  let primaryId = $state<string | null>(null);
  let preferences = $state<Record<string, string>>({});
  let savedPreferences = $state<Record<string, string>>({});
  let paths = $state<Record<string, WorkingFolderSelection>>({});
  let recreations = $state<string[]>([]);

  /** Remove the previous project's draft before reading a different project's configuration. */
  function clear(id: string): void {
    generation += 1;
    projectId = id;
    rows = []; saved = []; primary = null; primaryId = null;
    preferences = {}; savedPreferences = {}; paths = {}; recreations = [];
  }

  /** Load canonical state when the panel opens or switches projects. */
  function load(id: string, folders: readonly ProjectWorkingFolderRead[], selectedPrimary: ChatProjectPrimaryWorkingFolderRead,
    providerPreferences: Readonly<Record<string, string>>): void {
    generation += 1;
    projectId = id;
    saved = folders.filter((folder) => folder.workingFolder.projectId === id).map(copy);
    primary = { ...selectedPrimary };
    savedPreferences = Object.fromEntries(saved.map((folder) => [folder.workingFolder.id, providerPreferences[folder.workingFolder.id] ?? ""]));
    discard();
  }

  /** Cancel pending selections and restore the last successfully persisted state. */
  function discard(): void {
    generation += 1;
    rows = saved.map(copy);
    primaryId = primary?.workingFolderId ?? null;
    preferences = { ...savedPreferences };
    paths = {};
    recreations = [];
  }

  /** Ignore native dialog results after discard, project changes, or unmount. */
  function invalidate(): void { generation += 1; }

  /** Preview a native folder selection without creating its association or binding. */
  async function choose(folder?: ProjectWorkingFolderRead): Promise<void> {
    if (folder && (folder.workingFolder.kind !== "external" || folder.workingFolder.archivedAt)) return;
    const currentGeneration = generation;
    const persisted = folder && saved.some((entry) => entry.workingFolder.id === folder.workingFolder.id);
    const selection = await ports.pick(persisted ? folder.workingFolder.id : undefined);
    if (!selection || currentGeneration !== generation) return;
    if (rows.some((entry) => entry.workingFolder.id !== folder?.workingFolder.id && entry.canonicalPath === selection.canonicalPath)) {
      throw new Error("This folder is already assigned to the selected project");
    }
    if (folder) {
      const id = folder.workingFolder.id;
      const original = saved.find((entry) => entry.workingFolder.id === id);
      rows = rows.map((entry) => entry.workingFolder.id === id ? {
        ...entry, canonicalPath: selection.canonicalPath, bindingStatus: "available", currentBranch: null,
      } : entry);
      if (original?.canonicalPath === selection.canonicalPath && original.bindingStatus === "available") {
        const nextPaths = { ...paths }; delete nextPaths[id]; paths = nextPaths;
        rows = rows.map((entry) => entry.workingFolder.id === id ? { ...copy(original), workingFolder: entry.workingFolder } : entry);
      } else paths = { ...paths, [id]: selection };
      return;
    }
    const now = new Date().toISOString();
    const id = `working-folder:${crypto.randomUUID()}`;
    rows = [...rows, {
      workingFolder: {
        id, projectId, displayName: selection.displayName, kind: "external", managedRelativePath: null,
        repositoryKind: "none", repositoryIdentity: null, archivedAt: null, revision: 0,
        sortOrder: Math.max(0, ...rows.map((entry) => entry.workingFolder.sortOrder)) + 1,
        createdAt: now, updatedAt: now,
      },
      canonicalPath: selection.canonicalPath, bindingStatus: "available", lastVerifiedAt: null, currentBranch: null,
    }];
    paths = { ...paths, [id]: selection };
    preferences = { ...preferences, [id]: "" };
  }

  /** Keep the primary folder active when its previous selection is archived or removed. */
  function ensurePrimary(): void {
    if (rows.some((entry) => entry.workingFolder.id === primaryId && !entry.workingFolder.archivedAt)) return;
    primaryId = rows.find((entry) => entry.workingFolder.kind === "managed")?.workingFolder.id
      ?? rows.find((entry) => !entry.workingFolder.archivedAt)?.workingFolder.id ?? null;
  }

  /** Stage an external folder rename. Managed identities remain protected. */
  function rename(folder: ProjectWorkingFolderRead, name: string): void {
    if (folder.workingFolder.kind !== "external" || !name.trim()) return;
    rows = rows.map((entry) => entry.workingFolder.id === folder.workingFolder.id
      ? { ...entry, workingFolder: { ...entry.workingFolder, displayName: name.trim() } } : entry);
  }

  /** Stage archive or restore without affecting active Chat execution. */
  function setArchived(folder: ProjectWorkingFolderRead, archived: boolean): void {
    if (folder.workingFolder.kind !== "external") return;
    rows = rows.map((entry) => entry.workingFolder.id === folder.workingFolder.id
      ? { ...entry, workingFolder: { ...entry.workingFolder, archivedAt: archived ? new Date().toISOString() : null } } : entry);
    ensurePrimary();
  }

  /** Stage removal of an external association while preserving its files. */
  function remove(folder: ProjectWorkingFolderRead): void {
    if (folder.workingFolder.kind !== "external") return;
    rows = rows.filter((entry) => entry.workingFolder.id !== folder.workingFolder.id);
    const nextPaths = { ...paths }; delete nextPaths[folder.workingFolder.id]; paths = nextPaths;
    ensurePrimary();
  }

  /** Acknowledge each successful command so a failed Save can resume without repeating creation. */
  function acknowledge(result: ProjectWorkingFolderRead): void {
    const id = result.workingFolder.id;
    saved = [...saved.filter((entry) => entry.workingFolder.id !== id), copy(result)];
    const pendingSelection = paths[id];
    rows = rows.map((entry) => entry.workingFolder.id === id ? {
      ...copy(result),
      ...(pendingSelection ? { canonicalPath: pendingSelection.canonicalPath, bindingStatus: "available" as const, currentBranch: null } : {}),
      workingFolder: {
        ...result.workingFolder, displayName: entry.workingFolder.displayName, archivedAt: entry.workingFolder.archivedAt,
      },
    } : entry);
  }

  /** Commit folder configuration only after the settings session has validated its draft. */
  async function save(): Promise<void> {
    const removed = saved.filter((folder) => !rows.some((entry) => entry.workingFolder.id === folder.workingFolder.id));
    if (primary && removed.some((folder) => folder.workingFolder.id === primary?.workingFolderId)) {
      const replacement = saved.find((folder) => folder.workingFolder.id === primaryId && !folder.workingFolder.archivedAt)
        ?? saved.find((folder) => folder.workingFolder.kind === "managed");
      if (!replacement) throw new Error("The project has no saved replacement for its primary working folder");
      primary = await ports.primary(projectId, replacement.workingFolder.id, primary.revision);
    }
    for (const folder of removed) {
      await ports.remove(folder.workingFolder.id);
      saved = saved.filter((entry) => entry.workingFolder.id !== folder.workingFolder.id);
    }
    for (const folder of rows) {
      const id = folder.workingFolder.id;
      let original = saved.find((entry) => entry.workingFolder.id === id);
      if (!original) {
        const path = paths[id];
        if (!path) throw new Error("A new working folder has no selected path");
        original = await ports.add({ id, projectId, displayName: folder.workingFolder.displayName }, path);
        const nextPaths = { ...paths }; delete nextPaths[id]; paths = nextPaths;
        acknowledge(original);
      }
      if (original.workingFolder.archivedAt && !folder.workingFolder.archivedAt) {
        original = await ports.restore(id, original.workingFolder.revision); acknowledge(original);
      }
      if (folder.workingFolder.displayName !== original.workingFolder.displayName) {
        original = await ports.rename(id, folder.workingFolder.displayName, original.workingFolder.revision); acknowledge(original);
      }
      if (paths[id]) {
        original = await ports.bind(id, paths[id]);
        const nextPaths = { ...paths }; delete nextPaths[id]; paths = nextPaths;
        acknowledge(original);
      }
      if (recreations.includes(id)) {
        acknowledge(await ports.recreate(id));
        recreations = recreations.filter((entry) => entry !== id);
      }
    }
    if (primaryId && primaryId !== primary?.workingFolderId) {
      if (!primary) throw new Error("The primary working folder revision is unavailable");
      primary = await ports.primary(projectId, primaryId, primary.revision);
    }
    for (const folder of rows) {
      const id = folder.workingFolder.id;
      const original = saved.find((entry) => entry.workingFolder.id === id);
      if (!original) throw new Error("A working folder was not saved");
      if (Boolean(folder.workingFolder.archivedAt) !== Boolean(original.workingFolder.archivedAt)) {
        acknowledge(await (folder.workingFolder.archivedAt ? ports.archive : ports.restore)(id, original.workingFolder.revision));
      }
      const preference = preferences[id] ?? "";
      if (preference !== (savedPreferences[id] ?? "")) {
        await ports.provider(id, preference || null);
        savedPreferences = { ...savedPreferences, [id]: preference };
      }
    }
    discard();
  }

  return {
    load, clear, discard, invalidate, choose, rename, setArchived, remove, save,
    get folders() { return rows; },
    get primaryId() { return primaryId; },
    get primaryReady() { return primary !== null; },
    preference: (id: string) => preferences[id] ?? "",
    setPreference: (id: string, value: string) => { preferences = { ...preferences, [id]: value }; },
    makePrimary: (folder: ProjectWorkingFolderRead) => { if (!folder.workingFolder.archivedAt) primaryId = folder.workingFolder.id; },
    recreate: (folder: ProjectWorkingFolderRead) => {
      if (folder.workingFolder.kind === "managed" && folder.bindingStatus !== "available" && !recreations.includes(folder.workingFolder.id)) {
        recreations = [...recreations, folder.workingFolder.id];
      }
    },
    canOpen: (folder: ProjectWorkingFolderRead) => saved.some((entry) => entry.workingFolder.id === folder.workingFolder.id)
      && !paths[folder.workingFolder.id] && folder.bindingStatus === "available",
    get dirty() {
      return rows.length !== saved.length || primaryId !== (primary?.workingFolderId ?? null) || Object.keys(paths).length > 0 || recreations.length > 0
        || rows.some((folder) => {
          const original = saved.find((entry) => entry.workingFolder.id === folder.workingFolder.id);
          return !original || folder.workingFolder.displayName !== original.workingFolder.displayName
            || Boolean(folder.workingFolder.archivedAt) !== Boolean(original.workingFolder.archivedAt)
            || (preferences[folder.workingFolder.id] ?? "") !== (savedPreferences[folder.workingFolder.id] ?? "");
        });
    },
  };
}
