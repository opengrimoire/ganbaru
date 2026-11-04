import type { TranslationKey } from "$lib/i18n/translator.svelte";
import type { DistractionsRuleRefusal, DistractionsRulesEditOutcome } from "./rules";

/** Why the latest rule edit left the stored rules unchanged. */
export type DistractionsEditProblem =
  | { kind: "refused"; reason: DistractionsRuleRefusal }
  | { kind: "failed" };

type DistractionsEditStatusKey = Extract<TranslationKey, `settings.distractions.editStatus.${string}`>;

const REFUSAL_MESSAGE_KEYS = {
  missing: "settings.distractions.editStatus.missing",
  "invalid-target": "settings.distractions.editStatus.invalid",
  "invalid-category": "settings.distractions.editStatus.invalid",
  "invalid-name": "settings.distractions.editStatus.invalid",
  "invalid-hosts": "settings.distractions.editStatus.invalid",
  "duplicate-name": "settings.distractions.editStatus.duplicate",
  "invalid-app": "settings.distractions.editStatus.invalid",
  "protected-source": "settings.distractions.editStatus.protectedSource",
  "invalid-minutes": "settings.distractions.editStatus.invalid",
  "invalid-budget": "settings.distractions.editStatus.invalid",
  "invalid-sources": "settings.distractions.editStatus.invalid",
  "duplicate-source": "settings.distractions.editStatus.duplicate",
  "too-large": "settings.distractions.editStatus.tooLarge",
} as const satisfies Record<DistractionsRuleRefusal, DistractionsEditStatusKey>;

/**
 * The problem a rule edit outcome reports, or `null` when the edit was saved.
 *
 * @param outcome - The native edit outcome.
 * @returns The refusal to show, or `null` for a saved edit.
 */
export function distractionsEditProblem(outcome: DistractionsRulesEditOutcome): DistractionsEditProblem | null {
  return outcome.status === "saved" ? null : { kind: "refused", reason: outcome.reason };
}

/**
 * The localized message key that explains why a rule edit was not saved.
 *
 * @param problem - The refusal or failure of the latest edit.
 * @returns A catalog key under `settings.distractions.editStatus`.
 */
export function distractionsEditProblemMessageKey(problem: DistractionsEditProblem): DistractionsEditStatusKey {
  return problem.kind === "failed" ? "settings.distractions.editStatus.failed" : REFUSAL_MESSAGE_KEYS[problem.reason];
}
