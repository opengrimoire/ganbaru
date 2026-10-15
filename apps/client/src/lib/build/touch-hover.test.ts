import { describe, expect, it } from "vitest";
import { splitHoverSelector, touchHoverPostcss, type CssAtRuleNode, type CssParentNode, type CssRuleNode } from "./touch-hover";

/** Minimal stand-in for a PostCSS container, enough to observe where rules end up. */
class FakeContainer implements CssParentNode {
  nodes: Array<FakeRule | FakeAtRule> = [];
  parent?: FakeContainer;
  constructor(readonly type: string) {}
}

class FakeAtRule extends FakeContainer implements CssAtRuleNode {
  readonly name: string;
  readonly params: string;
  constructor(defaults: { name: string; params: string }) {
    super("atrule");
    this.name = defaults.name;
    this.params = defaults.params;
  }

  append(...nodes: FakeRule[]): void {
    for (const node of nodes) {
      node.parent?.nodes.splice(node.parent.nodes.indexOf(node), 1);
      node.parent = this;
      this.nodes.push(node);
    }
  }
}

class FakeRule implements CssRuleNode {
  readonly type = "rule";
  parent?: FakeContainer;
  constructor(public selectors: string[]) {}

  clone(overrides?: { selectors?: string[] }): FakeRule {
    return new FakeRule(overrides?.selectors ?? [...this.selectors]);
  }

  before(node: FakeAtRule): void {
    this.insert(node, 0);
  }

  after(node: FakeAtRule): void {
    this.insert(node, 1);
  }

  private insert(node: FakeAtRule, offset: number): void {
    if (!this.parent) return;
    node.parent = this.parent;
    this.parent.nodes.splice(this.parent.nodes.indexOf(this) + offset, 0, node);
  }
}

function serialize(container: FakeContainer): string {
  return container.nodes.map((node) => node instanceof FakeRule
    ? `${node.selectors.join(", ")} {}`
    : `@${node.name} ${node.params} { ${serialize(node)} }`).join(" ");
}

/** Run the plugin over top-level rules, revisiting changed rules the way PostCSS does. */
function transform(...selectorLists: string[][]): string {
  const root = new FakeContainer("root");
  for (const selectors of selectorLists) {
    const rule = new FakeRule(selectors);
    rule.parent = root;
    root.nodes.push(rule);
  }
  const plugin = touchHoverPostcss();
  const visit = (container: FakeContainer): void => {
    for (const node of [...container.nodes]) {
      if (node instanceof FakeRule) plugin.Rule(node, { AtRule: FakeAtRule });
      else visit(node);
    }
  };
  visit(root);
  visit(root);
  return serialize(root);
}

describe("splitHoverSelector", () => {
  it("leaves selectors without hover unchanged", () => {
    expect(splitHoverSelector(".button.active")).toEqual({ hover: false, withoutHover: ".button.active" });
  });

  it("requires hover for a plain hover selector", () => {
    expect(splitHoverSelector(".button.svelte-1x:hover")).toEqual({ hover: true, withoutHover: null });
    expect(splitHoverSelector("*::-webkit-scrollbar-thumb:hover")).toEqual({ hover: true, withoutHover: null });
  });

  it("drops hover alternatives but keeps open and focus states", () => {
    expect(splitHoverSelector('&:is(:hover, :focus-visible, [aria-expanded="true"]):not(:disabled)')).toEqual({
      hover: true,
      withoutHover: '&:is(:focus-visible, [aria-expanded="true"]):not(:disabled)',
    });
    expect(splitHoverSelector(".comment:is(:hover, :focus-within) .remove")).toEqual({ hover: true, withoutHover: ".comment:is(:focus-within) .remove" });
  });

  it("requires hover when every alternative needs it or another hover is required", () => {
    expect(splitHoverSelector(":is(.row, .heading):hover .action")).toEqual({ hover: true, withoutHover: null });
    expect(splitHoverSelector(".a:is(:hover, .b) .c:hover")).toEqual({ hover: true, withoutHover: null });
    expect(splitHoverSelector(".row:has(.grip:hover)")).toEqual({ hover: true, withoutHover: null });
  });

  it("ignores hover inside a negation and in attribute values", () => {
    expect(splitHoverSelector(".cell:not(:has(.resize:hover))")).toEqual({ hover: false, withoutHover: ".cell:not(:has(.resize:hover))" });
    expect(splitHoverSelector('[data-state=":hover"]')).toEqual({ hover: false, withoutHover: '[data-state=":hover"]' });
    expect(splitHoverSelector(".hovercard:focus")).toEqual({ hover: false, withoutHover: ".hovercard:focus" });
  });
});


describe("touchHoverPostcss", () => {
  it("moves a hover rule behind the hover media query", () => {
    expect(transform([".tool:hover"])).toBe("@media (hover: hover) { .tool:hover {} }");
  });

  it("keeps the non-hover selectors of a shared rule unconditional", () => {
    expect(transform([".tool:hover", ".tool.active"], [".other"])).toBe(".tool.active {} @media (hover: hover) { .tool:hover {} } .other {}");
  });

  it("keeps the open state of a mixed state list on touch", () => {
    expect(transform(['.item:is(:hover, [aria-expanded="true"])'])).toBe(
      '.item:is([aria-expanded="true"]) {} @media (hover: hover) { .item:is(:hover, [aria-expanded="true"]) {} }',
    );
  });

  it("leaves rules already behind a hover media query alone", () => {
    const root = new FakeContainer("root");
    const media = new FakeAtRule({ name: "media", params: "(hover:hover)" });
    media.parent = root;
    root.nodes.push(media);
    const rule = new FakeRule([".tool:hover"]);
    media.append(rule);

    touchHoverPostcss().Rule(rule, { AtRule: FakeAtRule });

    expect(serialize(root)).toBe("@media (hover:hover) { .tool:hover {} }");
  });
});
