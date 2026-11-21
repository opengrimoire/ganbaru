import { Temporal } from "@js-temporal/polyfill";
import type {
  ChatChangedFileStatus,
  ChatRestorePreviewRead,
  ChatRestoreWarningCode,
} from "$lib/chat/contracts";

/** One changed file the restore rewrites. */
export interface RestoreDialogFileRow {
  relativePath: string;
  previousRelativePath: string | null;
  status: ChatChangedFileStatus;
  additions: number | null;
  deletions: number | null;
  binary: boolean;
}

/** A consequence line of the restore; `warning` marks one the user should weigh before confirming. */
export type RestoreDialogFact =
  | { kind: "staging_restored"; tone: "warning" }
  | { kind: "staging_unchanged"; tone: "info" }
  | { kind: "provider_rollback"; tone: "info" }
  | { kind: "provider_fork"; tone: "warning" };

/** What the checkpoint restore dialog shows for one preview at one moment. */
export interface RestoreDialogModel {
  files: RestoreDialogFileRow[];
  additions: number;
  deletions: number;
  facts: RestoreDialogFact[];
  /** Whole minutes until the preview expires, rounded up; zero once it has expired. */
  remainingMinutes: number;
  /** The backend refuses an expired preview, so the dialog offers a refresh instead of Restore. */
  expired: boolean;
}

/**
 * Builds the restore dialog rows from a preview. A typed warning and the matching preview flag each mark the
 * consequence, so a warning is never lost when the flags disagree.
 *
 * @param preview The restore preview the backend returned.
 * @param nowEpochMilliseconds The current time.
 */
export function buildRestoreDialogModel(
  preview: ChatRestorePreviewRead,
  nowEpochMilliseconds: number,
): RestoreDialogModel {
  const warnings = new Set<ChatRestoreWarningCode>(preview.warnings);
  const stagingRestored = preview.stagedChanges || warnings.has("staging_restored");
  const forkRequired = preview.providerRollback === "unsupported" || warnings.has("provider_fork_required");
  const remainingMilliseconds = Temporal.Instant.from(preview.expiresAt).epochMilliseconds - nowEpochMilliseconds;
  const files = preview.files.map((file) => ({
    relativePath: file.relativePath,
    previousRelativePath: file.previousRelativePath,
    status: file.status,
    additions: file.additions,
    deletions: file.deletions,
    binary: file.binary,
  }));
  return {
    files,
    additions: files.reduce((total, file) => total + (file.additions ?? 0), 0),
    deletions: files.reduce((total, file) => total + (file.deletions ?? 0), 0),
    facts: [
      stagingRestored ? { kind: "staging_restored", tone: "warning" } : { kind: "staging_unchanged", tone: "info" },
      forkRequired ? { kind: "provider_fork", tone: "warning" } : { kind: "provider_rollback", tone: "info" },
    ],
    remainingMinutes: Math.max(0, Math.ceil(remainingMilliseconds / 60_000)),
    expired: remainingMilliseconds <= 0,
  };
}
