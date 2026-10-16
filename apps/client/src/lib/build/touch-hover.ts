/**
 * Build-time CSS transform that limits hand-written `:hover` styles to devices that can hover.
 *
 * A touch screen keeps `:hover` on the last tapped element until the next tap elsewhere, so a
 * trigger tapped to close its panel stays highlighted. Tailwind's `hover:` variant already sits
 * behind `@media (hover: hover)`; this applies the same rule to component and global CSS.
 */

/** Media query that matches a primary input able to hover. */
export const HOVER_MEDIA_PARAMS = "(hover: hover)";

const HOVER_MEDIA_PATTERN = /\(\s*hover\s*:\s*hover\s*\)/;
const LIST_PSEUDO_CLASSES = new Set(["is", "where", "has", "matches", "-webkit-any"]);

/** How a selector depends on `:hover`. */
export interface HoverSelectorSplit {
  /** The selector contains a positive `:hover`. */
  hover: boolean;
  /**
   * The selector with every alternative `:hover` removed, for example `:is(:hover, :focus-visible)`
   * becoming `:is(:focus-visible)`. Null when every match requires hover.
   */
  withoutHover: string | null;
}

interface ScanResult {
  found: boolean;
  required: boolean;
  out: string;
}

/**
 * Split a single complex selector into its hover-dependent and hover-free forms. A `:hover`
 * inside `:not()` is a negation that holds without hover, so it is left alone.
 */
export function splitHoverSelector(selector: string): HoverSelectorSplit {
  const result = scan(selector);
  if (!result.found) return { hover: false, withoutHover: selector };
  return { hover: true, withoutHover: result.required ? null : result.out };
}

function scan(selector: string): ScanResult {
  let found = false;
  let required = false;
  let out = "";
  let index = 0;
  while (index < selector.length) {
    const char = selector[index];
    if (char === "\"" || char === "'") {
      const end = skipString(selector, index);
      out += selector.slice(index, end);
      index = end;
      continue;
    }
    if (char === "[") {
      const end = skipBracket(selector, index);
      out += selector.slice(index, end);
      index = end;
      continue;
    }
    if (char === "\\") {
      out += selector.slice(index, index + 2);
      index += 2;
      continue;
    }
    if (char === ":" && selector[index + 1] !== ":") {
      const name = readIdentifier(selector, index + 1);
      const afterName = index + 1 + name.length;
      if (name.toLowerCase() === "hover" && selector[afterName] !== "(") {
        found = true;
        required = true;
        out += selector.slice(index, afterName);
        index = afterName;
        continue;
      }
      if (name && selector[afterName] === "(") {
        const close = matchingParen(selector, afterName);
        const inner = selector.slice(afterName + 1, close);
        const pseudo = name.toLowerCase();
        if (LIST_PSEUDO_CLASSES.has(pseudo)) {
          const list = scanList(inner);
          found ||= list.found;
          required ||= list.required;
          out += `:${name}(${list.out})`;
        } else {
          out += selector.slice(index, close + 1);
        }
        index = close + 1;
        continue;
      }
    }
    out += char;
    index += 1;
  }
  return { found, required, out };
}

/** A forgiving selector list matches when any item matches, so items that need hover drop out of the hover-free form. */
function scanList(list: string): ScanResult {
  const items = splitTopLevel(list).map((item) => ({ item, result: scan(item) }));
  const found = items.some(({ result }) => result.found);
  if (!found) return { found, required: false, out: list };
  const kept = items.filter(({ result }) => !result.required).map(({ result }) => result.out.trim());
  return { found, required: kept.length === 0, out: kept.length === 0 ? list : kept.join(", ") };
}

function splitTopLevel(list: string): string[] {
  const items: string[] = [];
  let start = 0;
  let index = 0;
  while (index < list.length) {
    const char = list[index];
    if (char === "\"" || char === "'") index = skipString(list, index);
    else if (char === "[") index = skipBracket(list, index);
    else if (char === "(") index = matchingParen(list, index) + 1;
    else if (char === "\\") index += 2;
    else {
      if (char === ",") {
        items.push(list.slice(start, index));
        start = index + 1;
      }
      index += 1;
    }
  }
  items.push(list.slice(start));
  return items;
}

function readIdentifier(text: string, start: number): string {
  const match = /^-?[A-Za-z_][\w-]*/.exec(text.slice(start));
  return match ? match[0] : "";
}

function skipString(text: string, start: number): number {
  const quote = text[start];
  let index = start + 1;
  while (index < text.length && text[index] !== quote) index += text[index] === "\\" ? 2 : 1;
  return Math.min(index + 1, text.length);
}

function skipBracket(text: string, start: number): number {
  let index = start + 1;
  while (index < text.length && text[index] !== "]") {
    if (text[index] === "\"" || text[index] === "'") index = skipString(text, index);
    else index += text[index] === "\\" ? 2 : 1;
  }
  return Math.min(index + 1, text.length);
}

function matchingParen(text: string, open: number): number {
  let depth = 0;
  let index = open;
  while (index < text.length) {
    const char = text[index];
    if (char === "\"" || char === "'") {
      index = skipString(text, index);
      continue;
    }
    if (char === "[") {
      index = skipBracket(text, index);
      continue;
    }
    if (char === "\\") {
      index += 2;
      continue;
    }
    if (char === "(") depth += 1;
    if (char === ")") {
      depth -= 1;
      if (depth === 0) return index;
    }
    index += 1;
  }
  return text.length - 1;
}

/** The parts of a PostCSS node this transform reads. */
export interface CssParentNode {
  type: string;
  parent?: CssParentNode | undefined;
}

/**
 * The parts of a PostCSS rule this transform changes. Insertion parameters are `unknown` so
 * PostCSS's own node types satisfy this shape without importing them into the app build.
 */
export interface CssRuleNode extends CssParentNode {
  selectors: string[];
  clone(overrides?: { selectors?: string[] }): CssRuleNode;
  before(node: unknown): unknown;
  after(node: unknown): unknown;
}

/** The parts of a PostCSS at-rule this transform creates. */
export interface CssAtRuleNode {
  append(...nodes: unknown[]): unknown;
}

/** The PostCSS helpers this transform uses. */
export interface CssHelpers {
  AtRule: new (defaults: { name: string; params: string }) => CssAtRuleNode;
}

function insideHoverMedia(rule: CssRuleNode): boolean {
  for (let node = rule.parent; node; node = node.parent) {
    if (node.type !== "atrule" || !("params" in node) || !("name" in node)) continue;
    if (node.name === "media" && typeof node.params === "string" && HOVER_MEDIA_PATTERN.test(node.params)) return true;
  }
  return false;
}

function gateRule(rule: CssRuleNode, helpers: CssHelpers): void {
  const always: string[] = [];
  const gated: string[] = [];
  for (const selector of rule.selectors) {
    const split = splitHoverSelector(selector);
    if (split.hover) gated.push(selector);
    if (split.withoutHover !== null) always.push(split.withoutHover);
  }
  if (gated.length === 0) return;
  const media = new helpers.AtRule({ name: "media", params: HOVER_MEDIA_PARAMS });
  if (always.length === 0) {
    rule.before(media);
    media.append(rule);
    return;
  }
  media.append(rule.clone({ selectors: gated }));
  rule.selectors = always;
  rule.after(media);
}

/** PostCSS plugin that moves hover styles behind `@media (hover: hover)`. */
export function touchHoverPostcss() {
  return {
    postcssPlugin: "ganbaru-touch-hover",
    /** Rules this creates are revisited, but they either lack `:hover` or sit inside the hover query. */
    Rule(rule: CssRuleNode, helpers: CssHelpers) {
      if (rule.selectors.some((selector) => selector.includes(":hover")) && !insideHoverMedia(rule)) gateRule(rule, helpers);
    },
  };
}
