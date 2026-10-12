import {
  blendHex,
  contrastRatio,
  pickReadableForeground,
  relativeLuminance,
  shiftPerceptualL,
} from "$lib/color/math";
import { resolveAppTokens, resolveCalendarCanvas, type Theme } from "$lib/themes";

const SYNTAX_TEXT_CONTRAST_TARGET = 4.5;

export interface ChatSyntaxPalette {
  background: string;
  foreground: string;
  comment: string;
  keyword: string;
  constant: string;
  string: string;
  parameter: string;
  function: string;
  stringExpression: string;
  punctuation: string;
  link: string;
  invalid: string;
  inserted: string;
  deleted: string;
}

/** Derives readable syntax roles from the active app and calendar surfaces. */
export function deriveChatSyntaxPalette(theme: Theme): ChatSyntaxPalette {
  const tokens = resolveAppTokens(theme);
  const background = resolveCalendarCanvas(theme);
  const foreground = readableAccent(background, tokens["--foreground"], tokens["--foreground"]);
  const accepted = readableAccent(background, tokens["--status-accepted"], foreground);
  const tentative = readableAccent(background, tokens["--status-tentative"], foreground);
  const declined = readableAccent(background, tokens["--status-declined"], foreground);
  const primary = readableAccent(background, tokens["--primary"], foreground);
  const muted = readableAccent(background, tokens["--muted-foreground"], foreground);

  return {
    background,
    foreground,
    comment: muted,
    keyword: tentative,
    constant: declined,
    string: accepted,
    parameter: readableAccent(
      background,
      blendHex(tokens["--primary"], tokens["--status-accepted"], 0.55),
      foreground,
    ),
    function: primary,
    stringExpression: readableAccent(
      background,
      blendHex(tokens["--status-accepted"], tokens["--status-tentative"], 0.68),
      foreground,
    ),
    punctuation: muted,
    link: primary,
    invalid: declined,
    inserted: accepted,
    deleted: declined,
  };
}

/** Serializes the shared palette for CodeMirror and Pierre's Shiki theme. */
export function chatSyntaxStyle(theme: Theme): string {
  const palette = deriveChatSyntaxPalette(theme);
  const variables: Readonly<Record<string, string>> = {
    "--chat-syntax-background": palette.background,
    "--chat-syntax-foreground": palette.foreground,
    "--chat-syntax-comment": palette.comment,
    "--chat-syntax-keyword": palette.keyword,
    "--chat-syntax-constant": palette.constant,
    "--chat-syntax-string": palette.string,
    "--chat-syntax-parameter": palette.parameter,
    "--chat-syntax-function": palette.function,
    "--chat-syntax-string-expression": palette.stringExpression,
    "--chat-syntax-punctuation": palette.punctuation,
    "--chat-syntax-link": palette.link,
    "--chat-syntax-invalid": palette.invalid,
    "--chat-syntax-inserted": palette.inserted,
    "--chat-syntax-deleted": palette.deleted,
    "--diffs-foreground": palette.foreground,
    "--diffs-background": palette.background,
    "--diffs-token-comment": palette.comment,
    "--diffs-token-keyword": palette.keyword,
    "--diffs-token-constant": palette.constant,
    "--diffs-token-string": palette.string,
    "--diffs-token-parameter": palette.parameter,
    "--diffs-token-function": palette.function,
    "--diffs-token-string-expression": palette.stringExpression,
    "--diffs-token-punctuation": palette.punctuation,
    "--diffs-token-link": palette.link,
    "--diffs-token-inserted": palette.inserted,
    "--diffs-token-deleted": palette.deleted,
  };
  return Object.entries(variables).map(([name, value]) => `${name}:${value}`).join(";");
}

function readableAccent(background: string, anchor: string, fallback: string): string {
  if (contrastRatio(background, anchor) >= SYNTAX_TEXT_CONTRAST_TARGET) return anchor;
  const direction = relativeLuminance(background) >= relativeLuminance(anchor) ? -1 : 1;
  for (let offset = 0.04; offset <= 0.8; offset += 0.04) {
    const candidate = shiftPerceptualL(anchor, direction * offset);
    if (contrastRatio(background, candidate) >= SYNTAX_TEXT_CONTRAST_TARGET) return candidate;
  }
  return pickReadableForeground(background, {
    ink: fallback,
    canvas: relativeLuminance(background) < 0.5 ? "#FFFFFF" : "#000000",
    target: SYNTAX_TEXT_CONTRAST_TARGET,
  });
}
