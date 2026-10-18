import {
  REPOSITORY_KINDS,
  WORKING_FOLDER_KINDS,
  WORKING_FOLDER_BINDING_STATUSES,
  WORKING_FOLDER_TRUST_STATES,
  type ProjectWorkingFolderRead,
  type ProjectWorkingFolder,
} from "../contracts";
import {
  readEnum,
  readIdentifier,
  readNonNegativeSafeInteger,
  readNullable,
  readRecord,
  readString,
  readUtcTimestamp,
} from "./readers";

export function parseProjectWorkingFolder(value: unknown, label = "workspace"): ProjectWorkingFolder {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    projectId: readIdentifier(record.projectId, `${label}.projectId`),
    displayName: readString(record.displayName, `${label}.displayName`),
    kind: readEnum(record.kind, WORKING_FOLDER_KINDS, `${label}.kind`),
    managedRelativePath: readNullable(
      record.managedRelativePath,
      `${label}.managedRelativePath`,
      readString,
    ),
    sortOrder: readNonNegativeSafeInteger(record.sortOrder, `${label}.sortOrder`),
    repositoryKind: readEnum(record.repositoryKind, REPOSITORY_KINDS, `${label}.repositoryKind`),
    repositoryIdentity: readNullable(record.repositoryIdentity, `${label}.repositoryIdentity`, readIdentifier),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
    updatedAt: readUtcTimestamp(record.updatedAt, `${label}.updatedAt`),
    archivedAt: readNullable(record.archivedAt, `${label}.archivedAt`, readUtcTimestamp),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
  };
}

export function parseProjectWorkingFolderRead(value: unknown, label = "workingFolderRead"): ProjectWorkingFolderRead {
  const record = readRecord(value, label);
  return {
    workingFolder: parseProjectWorkingFolder(
      record.workingFolder,
      `${label}.workingFolder`,
    ),
    bindingStatus: readEnum(record.bindingStatus, WORKING_FOLDER_BINDING_STATUSES, `${label}.bindingStatus`),
    canonicalPath: readNullable(record.canonicalPath, `${label}.canonicalPath`, readString),
    lastVerifiedAt: readNullable(record.lastVerifiedAt, `${label}.lastVerifiedAt`, readUtcTimestamp),
    currentBranch: readNullable(record.currentBranch, `${label}.currentBranch`, readString),
    trust: readEnum(record.trust, WORKING_FOLDER_TRUST_STATES, `${label}.trust`),
  };
}

export function parseProjectWorkingFolderReads(value: unknown): ProjectWorkingFolderRead[] {
  if (!Array.isArray(value)) throw new Error("Chat workingFolders must be an array");
  return value.map((workspace, index) => parseProjectWorkingFolderRead(workspace, `workingFolders[${index}]`));
}
