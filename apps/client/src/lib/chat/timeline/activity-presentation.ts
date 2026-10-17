import type { TimelineActivityRow } from "./model";

export interface CommandActivityPresentation {
  command: string | null;
  cwd: string | null;
  output: string | null;
  exitCode: number | null;
  durationMs: number | null;
  running: boolean;
}

export interface FileChangePresentation {
  path: string;
  kind: string;
  diff: string | null;
  additions: number;
  deletions: number;
}

export interface FileReadActivityPresentation {
  path: string | null;
}

export interface FileSearchActivityPresentation {
  query: string | null;
  scope: string | null;
}

export type ActivitySummaryKind =
  | "thinking"
  | "commands"
  | "file_changes"
  | "file_reads"
  | "file_searches"
  | "web_searches"
  | "image_views"
  | "tools"
  | "collaboration"
  | "review"
  | "compaction"
  | "errors";

export interface ActivitySummaryCount {
  kind: ActivitySummaryKind;
  count: number;
}

const MAX_TRANSIENT_SUMMARY_CHARACTERS = 160;

/** Returns whether a timeline activity represents a shell command lifecycle. */
export function isCommandActivity(activity: TimelineActivityRow): boolean {
  return activity.activityKind === "command_execution" || activity.activityKind === "command_output";
}

/** Returns whether a timeline activity represents a provider file edit lifecycle. */
export function isFileChangeActivity(activity: TimelineActivityRow): boolean {
  return activity.activityKind === "file_change" || activity.activityKind === "file_change_output";
}

/** Extracts the best known path from a file-oriented provider activity. */
export function activityFilePath(activity: TimelineActivityRow): string | null {
  const changedPath = fileChangePresentation(activity)[0]?.path;
  if (changedPath) return changedPath;
  const titlePath = activity.title.match(/^(?:edit|editing|edited|write|writing|wrote)\s+(.+)$/iu)?.[1]?.trim();
  return metadataPath(activity) ?? titlePath ?? null;
}

/** Returns whether an activity is a provider-neutral file read action. */
export function isFileReadActivity(activity: TimelineActivityRow): boolean {
  return fileReadActivityPresentation(activity) !== null;
}

/** Returns whether an activity searches local workspace files or file contents. */
export function isFileSearchActivity(activity: TimelineActivityRow): boolean {
  return fileSearchActivityPresentation(activity) !== null;
}

/** Extracts the best provider-neutral file target for a read activity. */
export function fileReadActivityPresentation(
  activity: TimelineActivityRow,
): FileReadActivityPresentation | null {
  const shellAction = semanticShellAction(activity);
  if (shellAction?.kind === "read") return { path: shellAction.path };
  if (isCommandActivity(activity)) return null;
  const tool = activityToolNames(activity).find(isReadToolName);
  const titlePath = activity.title.match(/^(?:read|reading|open|opening|opened)\s+(.+)$/iu)?.[1]?.trim() ?? null;
  if (!tool && !titlePath) return null;
  return { path: metadataPath(activity) ?? titlePath };
}

/** Extracts the query and scope for a local file-search activity. */
export function fileSearchActivityPresentation(
  activity: TimelineActivityRow,
): FileSearchActivityPresentation | null {
  const metadata = record(activity.metadata?.value);
  const protocolSearch = normalizedToolName(stringValue(metadata?.toolKind) ?? "") === "search";
  if (activity.activityKind === "web_search" && !protocolSearch) return null;
  const shellAction = semanticShellAction(activity);
  if (shellAction?.kind === "search") {
    return { query: shellAction.query, scope: shellAction.scope };
  }
  if (isCommandActivity(activity)) return null;
  const tool = activityToolNames(activity).find(isFileSearchToolName);
  const titleMatch = activity.title.match(/^(?:searching|searched)\s+(?:for\s+)?(.+?)(?:\s+in\s+(.+))?$/iu);
  if (!protocolSearch && !tool && !titleMatch) return null;
  const searchMetadata = metadataSearch(activity);
  return {
    query: searchMetadata.query ?? titleMatch?.[1]?.trim() ?? null,
    scope: searchMetadata.scope ?? titleMatch?.[2]?.trim() ?? null,
  };
}

/** Returns whether an activity is a provider-neutral image inspection action. */
export function isImageViewActivity(activity: TimelineActivityRow): boolean {
  if (activity.activityKind === "image_view") return true;
  const tool = normalizedToolName(activity.title);
  return ["view_image", "image_view"]
    .some((name) => tool === name || tool.endsWith(`_${name}`));
}

/** Summarizes consecutive actions by semantic kind while preserving first occurrence order. */
export function summarizeActivityKinds(
  activities: readonly TimelineActivityRow[],
): ActivitySummaryCount[] {
  const counts = new Map<ActivitySummaryKind, number>();
  for (const activity of activities) {
    const kind = activitySummaryKind(activity);
    counts.set(kind, (counts.get(kind) ?? 0) + 1);
  }
  return [...counts].map(([kind, count]) => ({ kind, count }));
}

/** Returns a bounded provider-designated summary suitable for a temporary live status row. */
export function transientActivitySummary(activity: TimelineActivityRow): string | null {
  if (activity.activityKind !== "reasoning_summary") return null;
  const lines = activity.detail
    ?.split(/\r?\n/u)
    .map((line) => line.trim().replace(/^(?:#{1,6}|>|\*|-)\s+/u, ""))
    .filter(Boolean);
  const summary = lines?.at(-1)?.replaceAll(/\s+/gu, " ").trim();
  if (!summary) return null;
  if (summary.length <= MAX_TRANSIENT_SUMMARY_CHARACTERS) return summary;
  return `${summary.slice(0, MAX_TRANSIENT_SUMMARY_CHARACTERS - 1).trimEnd()}…`;
}

/** Converts validated timeline data into the command card view model. */
export function commandActivityPresentation(
  activity: TimelineActivityRow,
): CommandActivityPresentation {
  const metadata = record(activity.metadata?.value);
  const rawCommand = activity.title.trim().replace(/^(?:run|running|ran)\s+/i, "");
  const command = rawCommand.length > 0
    && rawCommand !== "command execution"
    && rawCommand !== "command output"
    ? rawCommand
    : null;
  const output = activity.detail?.trim() || null;
  return {
    command,
    cwd: stringValue(metadata?.cwd),
    output,
    exitCode: integerValue(metadata?.exitCode),
    durationMs: nonNegativeNumber(metadata?.durationMs),
    running: activity.status === "pending"
      || activity.status === "active"
      || activity.status === "waiting",
  };
}

/** Converts provider file-change metadata into bounded diff rows. */
export function fileChangePresentation(activity: TimelineActivityRow): FileChangePresentation[] {
  const metadata = record(activity.metadata?.value);
  if (!Array.isArray(metadata?.changes)) return [];
  const changes: FileChangePresentation[] = [];
  for (const candidate of metadata.changes) {
    const change = record(candidate);
    const path = stringValue(change?.path);
    if (!path) continue;
    const diff = stringValue(change?.diff);
    const counts = diffLineCounts(diff);
    changes.push({
      path,
      kind: stringValue(change?.kind) ?? "modified",
      diff,
      additions: counts.additions,
      deletions: counts.deletions,
    });
  }
  return changes;
}

function diffLineCounts(diff: string | null): { additions: number; deletions: number } {
  if (!diff) return { additions: 0, deletions: 0 };
  let additions = 0;
  let deletions = 0;
  for (const line of diff.split(/\r?\n/u)) {
    if (line.startsWith("+") && !line.startsWith("+++")) additions += 1;
    if (line.startsWith("-") && !line.startsWith("---")) deletions += 1;
  }
  return { additions, deletions };
}

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function stringValue(value: unknown): string | null {
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : null;
}

function integerValue(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) ? value : null;
}

function nonNegativeNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : null;
}

function activitySummaryKind(activity: TimelineActivityRow): ActivitySummaryKind {
  if (activity.id.startsWith("turn-pending:")
    || activity.activityKind === "reasoning"
    || activity.activityKind === "reasoning_text"
    || activity.activityKind === "reasoning_summary") return "thinking";
  if (isFileChangeActivity(activity)) return "file_changes";
  if (isImageViewActivity(activity)) return "image_views";
  if (isFileSearchActivity(activity)) return "file_searches";
  if (activity.activityKind === "web_search") return "web_searches";
  if (isFileReadActivity(activity)) return "file_reads";
  if (isCommandActivity(activity)) return "commands";
  if (activity.activityKind === "collaboration_task") return "collaboration";
  if (activity.activityKind === "review_transition") return "review";
  if (activity.activityKind === "context_compaction") return "compaction";
  if (activity.activityKind === "error" || activity.status === "failed") return "errors";
  return "tools";
}

function normalizedToolName(title: string): string {
  return title.trim().toLowerCase().replaceAll(/[^a-z0-9]+/g, "_");
}

type SemanticShellAction =
  | { kind: "read"; path: string | null }
  | { kind: "search"; query: string | null; scope: string | null };

function semanticShellAction(activity: TimelineActivityRow): SemanticShellAction | null {
  if (!isCommandActivity(activity)) return null;
  const command = unwrapShellCommand(commandActivityPresentation(activity).command);
  if (!command) return null;
  const tokens = shellTokens(command);
  const executable = executableName(tokens[0]);
  if (!executable) return null;

  if (["rg", "ripgrep", "grep", "git-grep", "fd", "fdfind"].includes(executable)) {
    const positions = positionalArguments(tokens.slice(1), SEARCH_OPTIONS_WITH_VALUE);
    if (executable === "rg" || executable === "ripgrep") {
      const filesOnly = tokens.slice(1).some((token) => token === "--files");
      return filesOnly
        ? { kind: "search", query: null, scope: positions[0] ?? null }
        : { kind: "search", query: positions[0] ?? null, scope: positions[1] ?? null };
    }
    return { kind: "search", query: positions[0] ?? null, scope: positions[1] ?? null };
  }
  if (executable === "find") {
    const args = tokens.slice(1);
    const nameIndex = args.findIndex((token) => token === "-name" || token === "-iname" || token === "-path" || token === "-ipath");
    return {
      kind: "search",
      query: nameIndex >= 0 ? args[nameIndex + 1] ?? null : null,
      scope: args.find((token) => !token.startsWith("-")) ?? null,
    };
  }
  if (executable === "select-string") {
    const positions = positionalArguments(tokens.slice(1), new Set(["-pattern", "-path"]));
    return { kind: "search", query: positions[0] ?? null, scope: positions[1] ?? null };
  }
  if (["cat", "bat", "batcat", "head", "tail", "sed", "get-content"].includes(executable)) {
    return { kind: "read", path: readCommandPath(executable, tokens.slice(1)) };
  }
  return null;
}

const SEARCH_OPTIONS_WITH_VALUE = new Set([
  "-A", "-B", "-C", "-g", "-m", "-t", "--after-context", "--before-context",
  "--context", "--encoding", "--engine", "--glob", "--max-count", "--max-depth",
  "--type", "--type-add", "--type-not",
]);

function readCommandPath(executable: string, args: readonly string[]): string | null {
  if (executable === "sed") {
    let scriptConsumed = false;
    const files: string[] = [];
    for (let index = 0; index < args.length; index += 1) {
      const argument = args[index];
      if (!argument) continue;
      if (argument === "-e" || argument === "--expression" || argument === "-f" || argument === "--file") {
        index += 1;
        scriptConsumed = true;
        continue;
      }
      if (argument.startsWith("-")) continue;
      if (!scriptConsumed) {
        scriptConsumed = true;
        continue;
      }
      files.push(argument);
    }
    return files.length === 1 ? files[0] ?? null : null;
  }
  const optionsWithValue = executable === "head" || executable === "tail"
    ? new Set(["-n", "--lines", "-c", "--bytes"])
    : new Set<string>();
  const files = positionalArguments(args, optionsWithValue);
  return files.length === 1 ? files[0] ?? null : null;
}

function positionalArguments(args: readonly string[], optionsWithValue: ReadonlySet<string>): string[] {
  const positions: string[] = [];
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (!argument) continue;
    if (argument === "--") {
      positions.push(...args.slice(index + 1));
      break;
    }
    const option = argument.split("=", 1)[0] ?? argument;
    if (argument.startsWith("-")) {
      if (!argument.includes("=") && optionsWithValue.has(option)) index += 1;
      continue;
    }
    positions.push(argument);
  }
  return positions;
}

function unwrapShellCommand(command: string | null): string | null {
  if (!command) return null;
  const tokens = shellTokens(command);
  const executable = executableName(tokens[0]);
  if (!["bash", "sh", "zsh", "fish", "cmd", "powershell", "pwsh"].includes(executable ?? "")) {
    return command;
  }
  const commandFlag = tokens.findIndex((token) => ["-c", "-lc", "-ic", "/c", "-command"].includes(token.toLowerCase()));
  return commandFlag >= 0 ? tokens[commandFlag + 1] ?? null : command;
}

function shellTokens(command: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let quote: "'" | '"' | null = null;
  let escaped = false;
  for (const character of command.trim()) {
    if (escaped) {
      current += character;
      escaped = false;
      continue;
    }
    if (character === "\\" && quote !== "'") {
      escaped = true;
      continue;
    }
    if (quote) {
      if (character === quote) quote = null;
      else current += character;
      continue;
    }
    if (character === "'" || character === '"') {
      quote = character;
      continue;
    }
    if (/\s/u.test(character)) {
      if (current) tokens.push(current);
      current = "";
      continue;
    }
    current += character;
  }
  if (escaped) current += "\\";
  if (current) tokens.push(current);
  return tokens;
}

function executableName(value: string | undefined): string | null {
  if (!value) return null;
  const normalized = value.replaceAll("\\", "/").split("/").at(-1)?.toLowerCase() ?? "";
  return normalized.endsWith(".exe") ? normalized.slice(0, -4) : normalized;
}

function activityToolNames(activity: TimelineActivityRow): string[] {
  const metadata = record(activity.metadata?.value);
  const candidates = [
    activity.title,
    stringValue(metadata?.tool),
    stringValue(metadata?.toolName),
    stringValue(metadata?.name),
    stringValue(metadata?.toolKind),
  ];
  return candidates.filter((candidate): candidate is string => candidate !== null)
    .map(normalizedToolName);
}

function isReadToolName(tool: string): boolean {
  return ["read", "read_file", "read_files", "open_file", "read_mcp_resource", "get_content"]
    .some((name) => tool === name || tool.endsWith(`_${name}`));
}

function isFileSearchToolName(tool: string): boolean {
  return ["grep", "glob", "rg", "ripgrep", "find", "find_files", "search_files", "file_search"]
    .some((name) => tool === name || tool.endsWith(`_${name}`));
}

function metadataPath(activity: TimelineActivityRow): string | null {
  const metadata = record(activity.metadata?.value);
  if (!metadata) return null;
  const locations = Array.isArray(metadata.locations) ? metadata.locations : [];
  for (const location of locations) {
    const path = stringValue(record(location)?.relativePath) ?? stringValue(record(location)?.path);
    if (path) return path;
  }
  for (const candidate of metadataInputRecords(metadata)) {
    const path = ["file_path", "filePath", "path", "filename", "relativePath"]
      .map((key) => stringValue(candidate[key]))
      .find((value) => value !== null);
    if (path) return path;
  }
  return null;
}

function metadataSearch(activity: TimelineActivityRow): { query: string | null; scope: string | null } {
  const metadata = record(activity.metadata?.value);
  if (!metadata) return { query: null, scope: null };
  for (const candidate of metadataInputRecords(metadata)) {
    const query = ["pattern", "query", "regex", "glob"]
      .map((key) => stringValue(candidate[key]))
      .find((value) => value !== null) ?? null;
    const scope = ["path", "file_path", "filePath", "directory", "cwd"]
      .map((key) => stringValue(candidate[key]))
      .find((value) => value !== null) ?? null;
    if (query || scope) return { query, scope };
  }
  return { query: null, scope: metadataPath(activity) };
}

function metadataInputRecords(metadata: Record<string, unknown>): Record<string, unknown>[] {
  const candidates = [
    metadata,
    record(metadata.toolInput),
    record(metadata.input),
    record(metadata.rawInput),
    record(record(metadata.state)?.input),
  ];
  return candidates.filter((candidate): candidate is Record<string, unknown> => candidate !== null);
}
