import type { ProviderFamilyId, ProviderFileRead, ProviderPreferenceRead } from "./contracts";

/** Catalog names of the teammate preferences Settings describes, keyed by preference ID. */
export const PROVIDER_PREFERENCE_NAMES = {
  language: "language",
  alwaysThinkingEnabled: "alwaysThinking",
  showThinkingSummaries: "thinkingSummaries",
  "attribution.commit": "commitAttribution",
  "attribution.pr": "pullRequestAttribution",
  CLAUDE_CODE_DISABLE_1M_CONTEXT: "disableLongContext",
  CLAUDE_CODE_DISABLE_THINKING: "disableThinking",
  CLAUDE_CODE_DISABLE_ADAPTIVE_THINKING: "disableAdaptiveThinking",
  CLAUDE_CODE_MAX_OUTPUT_TOKENS: "maxOutputTokens",
  MAX_THINKING_TOKENS: "maxThinkingTokens",
  CLAUDE_AUTOCOMPACT_PCT_OVERRIDE: "autoCompactPercentage",
  API_TIMEOUT_MS: "apiTimeout",
  BASH_DEFAULT_TIMEOUT_MS: "commandTimeout",
  BASH_MAX_TIMEOUT_MS: "commandMaxTimeout",
  BASH_MAX_OUTPUT_LENGTH: "commandOutputLength",
  DISABLE_TELEMETRY: "disableTelemetry",
  DISABLE_ERROR_REPORTING: "disableErrorReporting",
  CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: "disableNonessentialTraffic",
  CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY: "disableFeedbackSurvey",
  maxMode: "maxMode",
  "network.useHttp1ForAgent": "http1",
  "attribution.attributeCommitsToAgent": "cursorCommitAttribution",
  "attribution.attributePRsToAgent": "cursorPullRequestAttribution",
} as const;

export type ProviderPreferenceName = (typeof PROVIDER_PREFERENCE_NAMES)[keyof typeof PROVIDER_PREFERENCE_NAMES];

/** Kinds of provider documentation Settings links to. */
export type ProviderDocKind = "settings" | "variables" | "instructions";

export interface ProviderDocLink {
  kind: ProviderDocKind;
  url: string;
}

/**
 * Official documentation of each provider family's user configuration. Every URL must also be
 * listed in the desktop opener scope.
 */
export const PROVIDER_DOC_LINKS: Readonly<Record<string, readonly ProviderDocLink[]>> = {
  claude: [
    { kind: "settings", url: "https://code.claude.com/docs/en/settings" },
    { kind: "variables", url: "https://code.claude.com/docs/en/env-vars" },
    { kind: "instructions", url: "https://code.claude.com/docs/en/memory" },
  ],
  codex: [{ kind: "settings", url: "https://developers.openai.com/codex/config-reference" }],
  cursor: [{ kind: "settings", url: "https://cursor.com/docs/cli/reference/configuration" }],
  grok: [{ kind: "settings", url: "https://docs.x.ai/build/settings/reference" }],
  opencode: [{ kind: "settings", url: "https://opencode.ai/docs/config" }],
};

/** How teammate runs use a provider family's files, which decides the note Settings shows. */
export type ProviderFilesNote = "allowlist" | "native" | "cursor_rules" | null;

/**
 * The note Settings shows about how teammates use a provider's files.
 *
 * @param familyId Provider family of the selected instance.
 */
export function providerFilesNote(familyId: ProviderFamilyId): ProviderFilesNote {
  switch (familyId) {
    case "claude":
      return "allowlist";
    case "cursor":
      return "cursor_rules";
    case "codex":
    case "grok":
    case "opencode":
      return "native";
    default:
      return null;
  }
}

/**
 * The catalog name of a preference, or null for an ID this build has no description for.
 *
 * @param id Dotted settings path or variable name the backend reports.
 */
export function providerPreferenceName(id: string): ProviderPreferenceName | null {
  return isDescribedPreference(id) ? PROVIDER_PREFERENCE_NAMES[id] : null;
}

function isDescribedPreference(id: string): id is keyof typeof PROVIDER_PREFERENCE_NAMES {
  return Object.hasOwn(PROVIDER_PREFERENCE_NAMES, id);
}

/**
 * The value to save for text a user typed into a count, percentage, or text preference.
 *
 * Blank input clears the preference, except that a text preference whose empty value has a
 * meaning keeps an empty string. Counts and percentages are trimmed and left for the backend to
 * check.
 *
 * @param entry Preference being edited.
 * @param input Text from the field.
 */
export function providerPreferenceInputValue(entry: ProviderPreferenceRead, input: string): string | null {
  if (entry.valueType === "text") {
    if (input.length > 0) return input;
    return entry.emptyAllowed ? "" : null;
  }
  const trimmed = input.trim();
  return trimmed.length > 0 ? trimmed : null;
}

/** Instruction files first, then configuration files, each in backend order. */
export function splitProviderFiles(files: readonly ProviderFileRead[]): {
  instructions: ProviderFileRead[];
  configuration: ProviderFileRead[];
} {
  return {
    instructions: files.filter((file) => file.kind === "instructions"),
    configuration: files.filter((file) => file.kind === "configuration"),
  };
}
