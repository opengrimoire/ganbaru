import type { ProviderFamilyId, ProviderFileNoteKind, ProviderFileRead, ProviderPreferenceRead } from "./contracts";

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
  model_reasoning_summary: "codexReasoningSummary",
  model_verbosity: "codexVerbosity",
  web_search: "codexWebSearch",
  "history.persistence": "codexHistory",
  hide_agent_reasoning: "codexHideReasoning",
  show_raw_agent_reasoning: "codexRawReasoning",
  check_for_update_on_startup: "codexUpdateCheck",
  "analytics.enabled": "codexAnalytics",
  "feedback.enabled": "codexFeedback",
  model_context_window: "codexContextWindow",
  model_auto_compact_token_limit: "codexAutoCompactLimit",
  tool_output_token_limit: "codexToolOutputLimit",
  project_doc_max_bytes: "codexProjectDocBytes",
  "session.auto_compact_threshold_percent": "grokAutoCompactPercentage",
  "toolset.bash.timeout_secs": "grokCommandTimeout",
  "toolset.bash.max_timeout_secs": "grokCommandMaxTimeout",
  "toolset.bash.output_byte_limit": "grokCommandOutputLimit",
  "tools.respect_gitignore": "grokRespectGitignore",
  "features.web_fetch": "grokWebFetch",
  "subagents.enabled": "grokSubagents",
  "memory.enabled": "grokMemory",
  "cli.auto_update": "grokAutoUpdate",
  "ui.show_thinking_blocks": "grokThinkingBlocks",
  share: "opencodeShare",
  update: "opencodeUpdate",
  snapshots: "opencodeSnapshots",
  "compaction.auto": "opencodeAutoCompact",
  "compaction.keep.tokens": "opencodeCompactionKeepTokens",
  "tool_output.max_lines": "opencodeToolOutputLines",
  "tool_output.max_bytes": "opencodeToolOutputBytes",
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
export type ProviderFilesNote = "allowlist" | "native" | null;

/**
 * The note Settings shows about how teammates use a provider's files.
 *
 * @param familyId Provider family of the selected instance.
 */
export function providerFilesNote(familyId: ProviderFamilyId): ProviderFilesNote {
  switch (familyId) {
    case "claude":
      return "allowlist";
    case "codex":
    case "cursor":
    case "grok":
    case "opencode":
      return "native";
    default:
      return null;
  }
}

/**
 * Whether teammate runs keep only the catalog preferences of a family's configuration file, so a
 * value that fails its check is skipped rather than passed to the provider.
 *
 * @param familyId Provider family of the selected instance.
 */
export function providerPreferencesAllowlisted(familyId: ProviderFamilyId): boolean {
  return familyId === "claude" || familyId === "cursor";
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

/** Catalog names of the values choice preferences offer; values that mean the same share a name. */
export const PROVIDER_PREFERENCE_OPTION_NAMES = {
  auto: "automatic",
  concise: "concise",
  detailed: "detailed",
  none: "none",
  low: "low",
  medium: "medium",
  high: "high",
  disabled: "off",
  disable: "off",
  cached: "cached",
  indexed: "indexed",
  live: "live",
  "save-all": "saveAll",
  manual: "manual",
  notify: "notify",
} as const;

export type ProviderPreferenceOptionName =
  (typeof PROVIDER_PREFERENCE_OPTION_NAMES)[keyof typeof PROVIDER_PREFERENCE_OPTION_NAMES];

/**
 * The catalog name of a choice value, or null for a value this build has no label for.
 *
 * @param value Option the backend reports for a choice preference.
 */
export function providerPreferenceOptionName(value: string): ProviderPreferenceOptionName | null {
  return isDescribedOption(value) ? PROVIDER_PREFERENCE_OPTION_NAMES[value] : null;
}

function isDescribedOption(value: string): value is keyof typeof PROVIDER_PREFERENCE_OPTION_NAMES {
  return Object.hasOwn(PROVIDER_PREFERENCE_OPTION_NAMES, value);
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

/** Catalog names of the notes about instruction sources the shown files do not cover. */
export const PROVIDER_FILE_NOTE_NAMES = {
  cursor_user_rules: "cursorUserRules",
  grok_private_home: "grokPrivateHome",
  grok_rules: "grokRules",
  grok_other_home_instructions: "grokOtherHomeInstructions",
  codex_override: "codexOverride",
  opencode_claude_fallback: "opencodeClaudeFallback",
  opencode_instructions: "opencodeInstructions",
  claude_rules: "claudeRules",
} as const satisfies Record<ProviderFileNoteKind, string>;

const GROK_RULES_DOCS = "https://docs.x.ai/build/features/project-rules";
const OPENCODE_RULES_DOCS = "https://opencode.ai/docs/rules/";

/**
 * Official documentation of where each noted instruction source lives. Every URL must also be
 * listed in the desktop opener scope.
 */
export const PROVIDER_FILE_NOTE_LINKS: Readonly<Record<ProviderFileNoteKind, string>> = {
  cursor_user_rules: "https://cursor.com/docs/rules",
  grok_private_home: GROK_RULES_DOCS,
  grok_rules: GROK_RULES_DOCS,
  grok_other_home_instructions: GROK_RULES_DOCS,
  codex_override: "https://learn.chatgpt.com/docs/agent-configuration/agents-md",
  opencode_claude_fallback: OPENCODE_RULES_DOCS,
  opencode_instructions: OPENCODE_RULES_DOCS,
  claude_rules: "https://code.claude.com/docs/en/memory",
};

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
