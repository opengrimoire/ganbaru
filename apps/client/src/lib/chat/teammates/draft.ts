import type { ModelOptionSelection, ProviderModel, SafetyMode } from "$lib/chat/contracts";
import { isKnownModelOption, modelOptionRole } from "$lib/chat/composer/model-picker-model";

export interface TeammateProfileDraftSnapshotInput {
  displayName: string;
  role: string;
  instructions: string;
  providerId: string;
  safetyMode: SafetyMode;
  modelId: string;
  providerManagedModel: boolean;
  modelOptions: readonly ModelOptionSelection[];
  effort: string | null;
  speed: string | null;
}

export interface TeammateExecutionSummary {
  effort: string | null;
  speed: "standard" | "fast" | null;
}

/**
 * Derives stable teammate policy summaries from provider model options.
 */
export function teammateExecutionSummary(
  options: readonly ModelOptionSelection[],
  model: ProviderModel | null,
): TeammateExecutionSummary {
  const effortDefinition = model?.options.find((option) => (
    isKnownModelOption(option) && modelOptionRole(option) === "effort"
  ));
  const effortSelection = options.find((option) => (
    option.key === effortDefinition?.key || /effort|reasoning/iu.test(option.key)
  ));
  const effort = effortSelection?.value.kind === "choice"
    ? effortSelection.value.value
    : null;

  const speedDefinition = model?.options.find((option) => (
    isKnownModelOption(option) && modelOptionRole(option) === "speed"
  ));
  const speedSelection = options.find((option) => (
    option.key === speedDefinition?.key
      || /speed|fast|service[_ ]?tier/iu.test(option.key)
  ))?.value;
  if (speedSelection?.kind === "boolean") {
    return { effort, speed: speedSelection.value ? "fast" : "standard" };
  }
  if (speedSelection?.kind !== "choice") return { effort, speed: null };
  const choice = speedDefinition?.kind === "choice"
    ? speedDefinition.options.find((entry) => entry.value === speedSelection.value)
    : null;
  const identity = `${choice?.value ?? speedSelection.value} ${choice?.label ?? ""}`.toLowerCase();
  if (identity.includes("fast") || identity.includes("priority")) {
    return { effort, speed: "fast" };
  }
  if (identity.includes("standard") || identity.includes("default") || identity.includes("normal")) {
    return { effort, speed: "standard" };
  }
  return { effort, speed: null };
}

/**
 * Creates a canonical snapshot of persisted teammate profile and execution fields.
 */
export function teammateProfileDraftSnapshot(
  input: TeammateProfileDraftSnapshotInput,
): string {
  const modelOptions = [...input.modelOptions].sort((left, right) => left.key.localeCompare(right.key));
  return JSON.stringify({
    displayName: input.displayName.trim(),
    role: input.role.trim(),
    instructions: input.instructions.trim(),
    providerId: input.providerId,
    safetyMode: input.safetyMode,
    modelId: input.modelId,
    providerManagedModel: input.providerManagedModel,
    modelOptions,
    effort: input.effort,
    speed: input.speed,
  });
}

/**
 * Largest teammate instructions, in UTF-8 bytes, that the backend accepts. Mirrors the Rust
 * bound, which keeps room in the provider developer instruction cap for the context package
 * framing that dispatch adds before the instructions.
 */
export const TEAMMATE_INSTRUCTIONS_MAX_BYTES = 61_440;

const utf8Encoder = new TextEncoder();

/**
 * Whether `instructions`, trimmed as saving trims them, pass the backend byte bound.
 */
export function teammateInstructionsTooLong(instructions: string): boolean {
  return utf8Encoder.encode(instructions.trim()).length > TEAMMATE_INSTRUCTIONS_MAX_BYTES;
}
