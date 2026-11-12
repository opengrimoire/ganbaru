import type { ChatCommitDraftContextRead } from "$lib/chat/contracts";

/** Localized text a draft request prompt is assembled from. */
export interface DraftPromptText {
  /** What to write and the requested reply shape. */
  instruction: string;
  changedFiles: string;
  /** Note for paths left out of the list. */
  omittedFiles: (count: number) => string;
  subjects: string;
  patch: string;
  /** Note for a patch cut at its size limit. */
  patchTruncated: string;
  /** Note for a patch too large to include at all. */
  patchOmitted: string;
}

/**
 * Builds the turn prompt that asks a teammate to draft text from bounded Git context. Every cut
 * the context reports is stated, and the patch sits in a fence longer than any backtick run in it.
 *
 * @param context Bounded Git context.
 * @param text Localized prompt text.
 * @returns The prompt to send as a turn.
 */
export function buildDraftPrompt(context: ChatCommitDraftContextRead, text: DraftPromptText): string {
  const sections = [text.instruction];
  const files = context.stagedPaths.map((path) => `- ${path}`);
  if (context.omittedFiles > 0) files.push(text.omittedFiles(context.omittedFiles));
  sections.push(`${text.changedFiles}\n${files.join("\n")}`);
  if (context.recentSubjects.length > 0) {
    sections.push(`${text.subjects}\n${context.recentSubjects.map((subject) => `- ${subject}`).join("\n")}`);
  }
  if (context.patch.length > 0) {
    const fence = "`".repeat(Math.max(3, longestBacktickRun(context.patch) + 1));
    const patch = `${text.patch}\n${fence}diff\n${context.patch.replace(/\n$/u, "")}\n${fence}`;
    sections.push(context.patchTruncated ? `${patch}\n${text.patchTruncated}` : patch);
  } else if (context.patchTruncated) {
    sections.push(text.patchOmitted);
  }
  return sections.join("\n\n");
}

/** The length of the longest run of backticks in `text`. */
function longestBacktickRun(text: string): number {
  let longest = 0;
  for (const run of text.match(/`+/gu) ?? []) longest = Math.max(longest, run.length);
  return longest;
}
