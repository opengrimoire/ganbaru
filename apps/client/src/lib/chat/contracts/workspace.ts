import type { ProjectWorkingFolderId, RepositoryKind, UtcTimestamp } from "./common";

export const WORKING_FOLDER_BINDING_STATUSES = [
  "unbound",
  "available",
  "missing",
  "repository_mismatch",
] as const;
export type WorkingFolderBindingStatus = (typeof WORKING_FOLDER_BINDING_STATUSES)[number];
export const WORKING_FOLDER_KINDS = ["managed", "external"] as const;
export type WorkingFolderKind = (typeof WORKING_FOLDER_KINDS)[number];

export interface ProjectWorkingFolder {
  id: ProjectWorkingFolderId;
  projectId: string;
  displayName: string;
  kind: WorkingFolderKind;
  managedRelativePath: string | null;
  sortOrder: number;
  repositoryKind: RepositoryKind;
  repositoryIdentity: string | null;
  createdAt: UtcTimestamp;
  updatedAt: UtcTimestamp;
  archivedAt: UtcTimestamp | null;
  revision: number;
}

export interface CreateProjectWorkingFolderRequest {
  id: ProjectWorkingFolderId;
  projectId: string;
  displayName: string;
}

/** Device-local folder selection authorized by the native picker and applied only when Settings is saved. */
export interface ProjectWorkingFolderSelectionRead {
  selectionId: string;
  canonicalPath: string;
  displayName: string;
}

export interface ProjectWorkingFolderRead {
  workingFolder: ProjectWorkingFolder;
  bindingStatus: WorkingFolderBindingStatus;
  canonicalPath: string | null;
  lastVerifiedAt: UtcTimestamp | null;
  currentBranch: string | null;
}
