import type { ActivityStatus, ChatMessageRead, ChatWorkAssignmentId, PlanStep } from "$lib/chat/contracts";
import { ACTIVITY_STATUSES } from "$lib/chat/contracts";

/** How far a structured plan has come, as the task progress card shows it. */
export interface PlanProgressSummary {
  total: number;
  completed: number;
  /** Steps that failed or were interrupted. */
  failed: number;
  /** The step in progress, else the first step still to do, else `null`. */
  activeStep: PlanStep | null;
  /** One status per step, in plan order, for the segmented bar. */
  segments: ActivityStatus[];
}

const FAILED_STATUSES: ReadonlySet<ActivityStatus> = new Set(["failed", "interrupted"]);

/**
 * Summarizes the steps of a structured plan.
 *
 * @param steps The plan steps in plan order.
 * @returns The counts, the step to show, and one bar segment per step; `null` for a plan without steps.
 */
export function summarizePlanProgress(steps: readonly PlanStep[]): PlanProgressSummary | null {
  if (steps.length === 0) return null;
  return {
    total: steps.length,
    completed: steps.filter((step) => step.status === "completed").length,
    failed: steps.filter((step) => FAILED_STATUSES.has(step.status)).length,
    activeStep: steps.find((step) => step.status === "active")
      ?? steps.find((step) => step.status === "pending")
      ?? null,
    segments: steps.map((step) => step.status),
  };
}

/**
 * Reads the steps of the latest structured plan update an agent posted for one assignment.
 *
 * Plan updates reach a reply thread as replicated agent update replies, so every linked device reads
 * the same plan. Proposed plans carry no steps and are skipped.
 *
 * @param replies The loaded replies of the reply thread, oldest first.
 * @param assignmentId The assignment whose plan to read.
 * @returns The steps of the latest structured plan for that assignment, or `null` when it posted none.
 */
export function latestStructuredPlanSteps(
  replies: readonly ChatMessageRead[],
  assignmentId: ChatWorkAssignmentId,
): PlanStep[] | null {
  for (let index = replies.length - 1; index >= 0; index -= 1) {
    const steps = structuredPlanSteps(replies[index]?.richContent.value, assignmentId);
    if (steps) return steps;
  }
  return null;
}

function structuredPlanSteps(value: unknown, assignmentId: ChatWorkAssignmentId): PlanStep[] | null {
  if (!isRecord(value) || value.type !== "agent_update" || value.updateKind !== "plan") return null;
  if (value.assignmentId !== assignmentId || !isRecord(value.payload)) return null;
  const candidates = value.payload.steps;
  if (!Array.isArray(candidates)) return null;
  const steps = candidates.flatMap((candidate) => {
    const step = parsePlanStep(candidate);
    return step ? [step] : [];
  });
  return steps.length > 0 ? steps : null;
}

function parsePlanStep(value: unknown): PlanStep | null {
  if (!isRecord(value)) return null;
  const { id, text, status } = value;
  if (typeof id !== "string" || !id || typeof text !== "string" || !isActivityStatus(status)) return null;
  return { id, text, status };
}

function isActivityStatus(value: unknown): value is ActivityStatus {
  return typeof value === "string" && (ACTIVITY_STATUSES as readonly string[]).includes(value);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
