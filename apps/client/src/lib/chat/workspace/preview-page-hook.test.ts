// @vitest-environment jsdom
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import pageHookSource from "../../../../src-tauri/app/src/chat/preview/page_hook.js?raw";

interface PageHookConfig {
  name: string;
  resultBudgetBytes: number;
  fieldChars: number;
  urlChars: number;
  titleChars: number;
  fitRounds: number;
}

interface PageHook {
  snapshot(): string | null;
  matches(selector: string): "matched" | "absent" | "invalid";
}

interface Snapshot {
  url: string;
  title: string;
  truncated: boolean;
  text: string;
  html: string;
}

const BASE_CONFIG: Omit<PageHookConfig, "name"> = {
  resultBudgetBytes: 64 * 1024,
  fieldChars: 1_000_000,
  urlChars: 8_192,
  titleChars: 512,
  fitRounds: 48,
};

let hookCount = 0;
const restorers: Array<() => void> = [];

/** Evaluates the hook source the way the initialization script does and returns the installed hook. */
function installHook(overrides: Partial<PageHookConfig> = {}): { name: string; hook: PageHook } {
  hookCount += 1;
  const name = `ganbaruPreviewTest${hookCount}`;
  const install = new Function(`return (${pageHookSource.trim()});`)() as (config: PageHookConfig) => void;
  install({ ...BASE_CONFIG, ...overrides, name });
  return { name, hook: Reflect.get(window, name) as PageHook };
}

/** Replaces a property for the current test and restores it afterwards. */
function patch(target: object, key: PropertyKey, descriptor: PropertyDescriptor): void {
  const original = Object.getOwnPropertyDescriptor(target, key);
  Object.defineProperty(target, key, { ...descriptor, configurable: true });
  restorers.push(() => {
    if (original) Object.defineProperty(target, key, original);
    else Reflect.deleteProperty(target, key);
  });
}

function restorePatches(): void {
  while (restorers.length > 0) restorers.pop()?.();
}

function utf8Bytes(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function parseSnapshot(result: string | null): Snapshot {
  expect(result).not.toBeNull();
  return JSON.parse(result ?? "") as Snapshot;
}

beforeAll(() => {
  if (!Object.getOwnPropertyDescriptor(HTMLElement.prototype, "innerText")) {
    Object.defineProperty(HTMLElement.prototype, "innerText", {
      configurable: true,
      get(this: HTMLElement) {
        return this.textContent ?? "";
      },
    });
  }
});

afterEach(() => {
  restorePatches();
  document.title = "";
  document.body.replaceChildren();
});

describe("preview page hook", () => {
  it("snapshots the page through references captured before page scripts run", () => {
    document.title = "Preview";
    document.body.textContent = "Visible text";
    const { hook } = installHook();

    const hugeText = "x".repeat(4 * 1024 * 1024);
    const stringify = JSON.stringify;
    let result: string | null = null;
    patch(JSON, "stringify", { value: () => hugeText, writable: true });
    patch(String.prototype, "slice", { value: () => hugeText, writable: true });
    patch(Document.prototype, "title", { get: () => hugeText });
    patch(HTMLElement.prototype, "innerText", { get: () => hugeText });
    patch(Element.prototype, "outerHTML", { get: () => hugeText });
    try {
      result = hook.snapshot();
    } finally {
      restorePatches();
    }

    expect(stringify).toBe(JSON.stringify);
    const snapshot = parseSnapshot(result);
    expect(snapshot.url).toBe(location.href);
    expect(snapshot.title).toBe("Preview");
    expect(snapshot.text).toBe("Visible text");
    expect(snapshot.html).toContain("<body>Visible text</body>");
    expect(snapshot.truncated).toBe(false);
  });

  it("fits large pages under the result budget, shrinking HTML before text", () => {
    const budget = 16 * 1024;
    document.body.textContent = "é".repeat(6 * 1024);
    document.body.append(document.createElement("div"));
    const { hook } = installHook({ resultBudgetBytes: budget });

    const result = hook.snapshot();

    expect(utf8Bytes(JSON.stringify(result))).toBeLessThanOrEqual(budget);
    const snapshot = parseSnapshot(result);
    expect(snapshot.truncated).toBe(true);
    expect(snapshot.text).toBe("é".repeat(6 * 1024));
    expect(snapshot.html.length).toBeLessThan(document.documentElement.outerHTML.length);
  });

  it("drops page content after the fit rounds and fails when metadata alone exceeds the budget", () => {
    document.title = "t".repeat(400);
    document.body.textContent = "y".repeat(32 * 1024);
    const fitted = installHook({ resultBudgetBytes: 2 * 1024, fitRounds: 0 }).hook.snapshot();
    const snapshot = parseSnapshot(fitted);
    expect(snapshot).toMatchObject({ truncated: true, text: "", html: "" });

    const overBudget = installHook({ resultBudgetBytes: 64 }).hook.snapshot();
    expect(overBudget).toBeNull();
  });

  it("reports selector matches without trusting the page's querySelector", () => {
    document.body.innerHTML = '<main id="app"><button class="ready">Go</button></main>';
    const { hook } = installHook();
    let results: string[] = [];
    patch(Document.prototype, "querySelector", { value: () => null, writable: true });
    try {
      results = [hook.matches("#app .ready"), hook.matches(".missing"), hook.matches("##")];
    } finally {
      restorePatches();
    }

    expect(results).toEqual(["matched", "absent", "invalid"]);
  });

  it("keeps the hook from being replaced, extended, or deleted by the page", () => {
    const { name, hook } = installHook();

    expect(Object.isFrozen(hook)).toBe(true);
    expect(Object.getPrototypeOf(hook)).toBeNull();
    expect(Reflect.set(window, name, {})).toBe(false);
    expect(Reflect.deleteProperty(window, name)).toBe(false);
    expect(Reflect.set(hook, "snapshot", () => "{}")).toBe(false);
    expect(Reflect.get(window, name)).toBe(hook);
  });
});
