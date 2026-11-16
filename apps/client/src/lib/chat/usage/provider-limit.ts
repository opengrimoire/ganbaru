import type { ChatMessageRead, ChatWorkAssignmentId, UtcTimestamp } from "$lib/chat/contracts";

/** Assignment state reason the executing device writes when a provider usage limit stopped the work. */
export const PROVIDER_USAGE_LIMIT_REASON = "provider_usage_limit";

/** A provider usage limit that stopped an assignment, with when it resets if the provider said. */
export interface ProviderLimitStop {
  resetsAt: UtcTimestamp | null;
}

/** The parts of an assignment that decide whether its reply thread shows a provider limit notice. */
export interface LimitNoticeAssignment {
  id: ChatWorkAssignmentId;
  state: string;
  stateReason: string | null;
}

/** The parts of a live provider rate limit report the notice reads. */
export interface LiveRateLimit {
  limited: boolean;
  resetsAt: UtcTimestamp | null;
}

/**
 * Reads the provider usage limit that stopped `assignmentId` from the newest failure update among
 * `replies`. A newer failure for another reason hides an older limit stop, and rich content another
 * device wrote that does not match the expected shape returns `null`.
 */
export function latestProviderLimitStop(
  replies: readonly ChatMessageRead[],
  assignmentId: ChatWorkAssignmentId,
): ProviderLimitStop | null {
  for (let index = replies.length - 1; index >= 0; index -= 1) {
    const value = record(replies[index]?.richContent.value);
    if (value?.type !== "agent_update" || value.updateKind !== "failure") continue;
    if (value.assignmentId !== assignmentId) continue;
    return providerLimitStop(record(value.payload));
  }
  return null;
}

/**
 * Returns the provider limit notice of a reply thread: the failure every device reads once a provider
 * limit stopped the work, else the live report of the device that runs it. `resetsAt` is `null`
 * when the provider gave no reset time.
 */
export function providerLimitNotice(
  assignment: LimitNoticeAssignment | null,
  replies: readonly ChatMessageRead[],
  live: LiveRateLimit | null,
): ProviderLimitStop | null {
  if (assignment?.state === "failed" && assignment.stateReason === PROVIDER_USAGE_LIMIT_REASON) {
    return { resetsAt: latestProviderLimitStop(replies, assignment.id)?.resetsAt ?? null };
  }
  if (live?.limited) {
    const resetsAt = live.resetsAt !== null && !Number.isNaN(Date.parse(live.resetsAt)) ? live.resetsAt : null;
    return { resetsAt };
  }
  return null;
}

function providerLimitStop(payload: Record<string, unknown> | null): ProviderLimitStop | null {
  if (payload?.code !== PROVIDER_USAGE_LIMIT_REASON) return null;
  const resetsAt = typeof payload.resetsAt === "string" && !Number.isNaN(Date.parse(payload.resetsAt))
    ? payload.resetsAt as UtcTimestamp
    : null;
  return { resetsAt };
}

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}
