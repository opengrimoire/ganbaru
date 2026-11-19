/**
 * Reference picker candidates in the Chat composer: their shape, grouping, keyboard navigation over
 * enabled entries, conversion to message references, and the assignment target that a post's
 * execution environment references select.
 */
import type {
  ChatAiAgentRead,
  ChatAssignmentTargetRead,
  ChatChannelRead,
  ChatMessageReference,
  ChatParticipantRead,
  ProjectWorkingFolderPathRead,
} from "$lib/chat/contracts";

/** One entry the reference picker offers for an `@` or `#` trigger. */
export type ReferenceCandidate =
  | {
      key: string;
      kind: "participant";
      group: string;
      label: string;
      description: string;
      participant: ChatParticipantRead;
      agent: ChatAiAgentRead | null;
      needsMembership: boolean;
    }
  | {
      key: string;
      kind: "channel";
      group: string;
      label: string;
      description: string;
      channel: ChatChannelRead;
    }
  | {
      key: string;
      kind: "workingFolder";
      group: string;
      label: string;
      description: string;
      workingFolderId: string;
    }
  | {
      key: string;
      kind: "workspacePath";
      group: string;
      label: string;
      description: string;
      workingFolderId: string;
      path: ProjectWorkingFolderPathRead;
    }
  | {
      key: string;
      kind: "executionEnvironment";
      group: string;
      label: string;
      description: string;
      target: ChatAssignmentTargetRead;
    };

/** A run of consecutive candidates that share a group label, with each candidate's flat index. */
export interface ReferenceCandidateGroup {
  label: string;
  entries: Array<{ candidate: ReferenceCandidate; index: number }>;
}

/** Whether any value contains the lowercase query; an empty query matches everything. */
export function matchesQuery(query: string, ...values: string[]): boolean {
  return !query || values.some((value) => value.toLocaleLowerCase().includes(query));
}

/** Groups consecutive candidates by their group label, keeping the picker order. */
export function groupReferenceCandidates(candidates: readonly ReferenceCandidate[]): ReferenceCandidateGroup[] {
  const groups: ReferenceCandidateGroup[] = [];
  candidates.forEach((candidate, index) => {
    const group = groups.at(-1);
    if (group?.label === candidate.group) group.entries.push({ candidate, index });
    else groups.push({ label: candidate.group, entries: [{ candidate, index }] });
  });
  return groups;
}

/** Whether the picker shows the candidate without letting it be chosen. */
export function candidateIsDisabled(candidate: ReferenceCandidate): boolean {
  return candidate.kind === "executionEnvironment" && !candidate.target.eligible;
}

/** Index of the first enabled candidate, or 0 when none is enabled. */
export function firstPickerIndex(candidates: readonly ReferenceCandidate[]): number {
  const index = candidates.findIndex((candidate) => !candidateIsDisabled(candidate));
  return index < 0 ? 0 : index;
}

/** Index of the last enabled candidate, or 0 when none is enabled. */
export function lastPickerIndex(candidates: readonly ReferenceCandidate[]): number {
  for (let index = candidates.length - 1; index >= 0; index -= 1) {
    if (!candidateIsDisabled(candidates[index]!)) return index;
  }
  return 0;
}

/** Next enabled index in the direction, wrapping around; the current index when no other is enabled. */
export function nextPickerIndex(
  candidates: readonly ReferenceCandidate[],
  current: number,
  direction: 1 | -1,
): number {
  if (candidates.length === 0) return 0;
  let index = current;
  for (let attempts = 0; attempts < candidates.length; attempts += 1) {
    index = (index + direction + candidates.length) % candidates.length;
    if (!candidateIsDisabled(candidates[index]!)) return index;
  }
  return current;
}

/**
 * Builds the message reference a chosen candidate inserts, with a fresh reference ID. Offsets are set
 * when the reference is inserted. Returns null for an execution environment without an execution target.
 */
export function candidateReference(candidate: ReferenceCandidate): ChatMessageReference | null {
  const metadata = {
    referenceId: crypto.randomUUID(),
    labelSnapshot: candidate.label.replace(/^[@#]/u, ""),
    startOffset: 0,
    endOffset: 0,
    plainTextProjection: candidate.kind === "channel"
      ? candidate.label
      : `@${candidate.label.replace(/^@/u, "")}`,
  };
  switch (candidate.kind) {
    case "participant":
      return {
        kind: "participant",
        metadata,
        participantId: candidate.participant.id,
        participantKind: candidate.participant.kind,
      };
    case "channel":
      return { kind: "channel", metadata, channelId: candidate.channel.id };
    case "workingFolder":
      return { kind: "workingFolder", metadata, workingFolderId: candidate.workingFolderId };
    case "workspacePath":
      return {
        kind: "workspacePath",
        metadata,
        workingFolderId: candidate.workingFolderId,
        pathKind: candidate.path.kind === "directory" ? "folder" : "file",
        relativePath: candidate.path.relativePath,
      };
    case "executionEnvironment":
      return candidate.target.executionTarget ? {
        kind: "executionEnvironment",
        metadata,
        executionEnvironmentId: candidate.target.executionTarget.executionEnvironmentId,
      } : null;
  }
}

/**
 * Picks the eligible target for the single execution environment the references name, else the
 * eligible default target, else null.
 */
export function inferAssignmentTarget(
  references: readonly ChatMessageReference[],
  targets: readonly ChatAssignmentTargetRead[],
): ChatAssignmentTargetRead | null {
  const referencedEnvironmentIds = new Set(references.flatMap((reference) => (
    reference.kind === "executionEnvironment" ? [reference.executionEnvironmentId] : []
  )));
  if (referencedEnvironmentIds.size === 1) {
    const environmentId = [...referencedEnvironmentIds][0];
    const target = targets.find((candidate) => (
      candidate.executionTarget?.executionEnvironmentId === environmentId && candidate.eligible
    ));
    if (target) return target;
  }
  return targets.find((target) => target.isDefault && target.eligible) ?? null;
}
