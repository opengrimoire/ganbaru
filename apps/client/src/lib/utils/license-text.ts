/**
 * One readable block of a plain-text license laid out in the GNU style.
 *
 * `title` holds the leading centered lines (name and version), `heading` a later centered line, `section` a numbered term title, `item` a lettered clause, `notice` an indented sample whose line breaks carry meaning, and `paragraph` running text.
 */
export type LicenseBlock =
  | { kind: "title"; lines: string[] }
  | { kind: "heading"; text: string }
  | { kind: "section"; text: string }
  | { kind: "item"; text: string }
  | { kind: "notice"; lines: string[] }
  | { kind: "paragraph"; text: string };

/** Leading spaces at which a line counts as centered rather than indented. */
const CENTERED_INDENT = 8;
/** Leading spaces at which a block counts as an indented clause or sample. */
const NESTED_INDENT = 4;
const SECTION_TITLE = /^\d+\.\s+\S/;
const LETTERED_ITEM = /^[a-z]\)\s+\S/;

function leadingSpaces(line: string): number {
  return line.length - line.trimStart().length;
}

function reflow(lines: readonly string[]): string {
  return lines.map((line) => line.trim()).join(" ");
}

function classifyBlock(lines: readonly string[], isFirst: boolean): LicenseBlock {
  const trimmed = lines.map((line) => line.trim());
  const minimumIndent = Math.min(...lines.map(leadingSpaces));
  if (minimumIndent >= CENTERED_INDENT) {
    return isFirst ? { kind: "title", lines: trimmed } : { kind: "heading", text: reflow(lines) };
  }
  if (lines.length === 1 && SECTION_TITLE.test(trimmed[0])) return { kind: "section", text: trimmed[0] };
  if (minimumIndent >= NESTED_INDENT) {
    return LETTERED_ITEM.test(trimmed[0]) ? { kind: "item", text: reflow(lines) } : { kind: "notice", lines: trimmed };
  }
  return { kind: "paragraph", text: reflow(lines) };
}

/**
 * Splits a hard-wrapped plain-text license into blocks that wrap at any width.
 *
 * Only whitespace changes: every word keeps its order, so the license stays verbatim.
 *
 * @param source - License text with blank lines between blocks and hard line breaks inside them.
 * @returns Blocks in document order.
 */
export function parseLicenseText(source: string): LicenseBlock[] {
  return source
    .replace(/\r\n?/g, "\n")
    .split(/\n[ \t]*\n/)
    .map((block) => block.split("\n").filter((line) => line.trim() !== ""))
    .filter((lines) => lines.length > 0)
    .map((lines, index) => classifyBlock(lines, index === 0));
}
