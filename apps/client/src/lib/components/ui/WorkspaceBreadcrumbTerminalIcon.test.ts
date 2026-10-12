// @vitest-environment jsdom

import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import WorkspaceBreadcrumbTerminalIcon from "./WorkspaceBreadcrumbTerminalIcon.svelte";

const mounted: Array<ReturnType<typeof mount>> = [];

afterEach(async () => {
  while (mounted.length > 0) {
    const component = mounted.pop();
    if (component) await unmount(component);
  }
  document.body.replaceChildren();
});

describe("WorkspaceBreadcrumbTerminalIcon", () => {
  it.each(["chevron", "plus"] as const)(
    "renders the %s with the shared priority stroke",
    (kind) => {
      const target = document.createElement("div");
      document.body.append(target);
      mounted.push(mount(WorkspaceBreadcrumbTerminalIcon, { target, props: { kind } }));

      const icon = target.querySelector<SVGElement>(
        `[data-workspace-breadcrumb-terminal-icon="${kind}"]`,
      );
      expect(icon?.getAttribute("width")).toBe("14");
      expect(icon?.getAttribute("height")).toBe("14");
      expect(icon?.getAttribute("stroke-width")).toBe("2");
      expect(icon?.style.getPropertyValue("stroke-width")).toBe("2");
      expect(icon?.style.getPropertyPriority("stroke-width")).toBe("important");
    },
  );
});
