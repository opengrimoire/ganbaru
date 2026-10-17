// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import SettingsModal from "./SettingsModal.svelte";

vi.mock("@tauri-apps/api/window", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@tauri-apps/api/window")>();
  return {
    ...actual,
    getCurrentWindow: () => ({ label: "main" }),
  };
});

vi.mock("$lib/stores/theme-editor.svelte", () => ({
  getThemeEditor: () => ({ editingId: null }),
}));

vi.mock("$lib/stores/viewport.svelte", () => ({
  getViewport: () => ({ below: () => false }),
}));

vi.mock("$lib/build-info", () => ({
  BUILD_REF: "0.0.0+test",
  GITHUB_REPOSITORY: "opengrimoire/ganbaru",
}));

vi.mock("$lib/components/settings/detail-registry", async () => {
  const { default: Stub } = await import("./SettingsSectionStub.test.svelte");
  const load = async (kind: string) => ({ kind, component: Stub });
  return {
    loadSettingsDetail: load,
    retrySettingsDetail: load,
  };
});

class ResizeObserverStub {
  observe(): void {}
  disconnect(): void {}
}

describe("SettingsModal remount state", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;
  let originalScrollTo: typeof HTMLElement.prototype.scrollTo | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    if (originalScrollTo) {
      HTMLElement.prototype.scrollTo = originalScrollTo;
    } else {
      Reflect.deleteProperty(HTMLElement.prototype, "scrollTo");
    }
    originalScrollTo = undefined;
    vi.unstubAllGlobals();
  });

  it("does not retain the previous active section across modal instances", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    target = document.createElement("div");
    document.body.append(target);

    component = mount(SettingsModal, {
      target,
      props: { initialSection: "about", onClose: () => undefined },
    });
    await tick();
    expect(target.querySelector("[data-settings-modal-panel]")?.getAttribute("data-settings-section"))
      .toBe("about");
    expect(target.querySelector("[data-loading]")).toBeNull();
    await unmount(component);
    component = mount(SettingsModal, {
      target,
      props: { onClose: () => undefined },
    });
    await tick();
    expect(target.querySelector("[data-settings-modal-panel]")?.getAttribute("data-settings-section"))
      .toBe("appearance");
    expect(target.querySelector("[data-loading]")).toBeNull();
  });

  it("loads Data as one stable section on its first visit", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    vi.stubGlobal("__GANBARU_BUILD_PLATFORM__", "linux");
    target = document.createElement("div");
    document.body.append(target);

    component = mount(SettingsModal, {
      target,
      props: { initialSection: "data", onClose: () => undefined },
    });

    await vi.waitFor(() => {
      expect(target?.textContent).toContain("Linked devices");
      expect(target?.textContent).toContain("Current folder");
      expect(target?.textContent).not.toContain("Could not load Data");
    });
  });

  it("uses categories before details and omits shortcuts in the mobile presentation", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    originalScrollTo = HTMLElement.prototype.scrollTo;
    Object.defineProperty(HTMLElement.prototype, "scrollTo", {
      configurable: true,
      value: vi.fn(),
    });
    target = document.createElement("div");
    document.body.append(target);

    component = mount(SettingsModal, {
      target,
      props: { presentation: "mobile", onClose: () => undefined },
    });
    await tick();

    expect(target.textContent).not.toContain("Shortcuts");
    const aboutButton = [...target.querySelectorAll("button")]
      .find((button) => button.textContent?.trim() === "About");
    expect(aboutButton).toBeDefined();
    aboutButton?.click();
    await tick();

    expect(target.querySelector("[data-settings-modal-panel]")?.getAttribute("data-settings-section"))
      .toBe("about");
    const backButton = target.querySelector<HTMLButtonElement>(
      '[aria-label="Back to settings categories"]',
    );
    expect(backButton).not.toBeNull();
    backButton?.click();
    await tick();
    expect(target.querySelector('[aria-label="Back to settings categories"]')).toBeNull();
  });

  it("renders and closes a nested detail in the mobile presentation", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    vi.stubGlobal("__GANBARU_BUILD_PLATFORM__", "android");
    originalScrollTo = HTMLElement.prototype.scrollTo;
    Object.defineProperty(HTMLElement.prototype, "scrollTo", {
      configurable: true,
      value: vi.fn(),
    });
    target = document.createElement("div");
    document.body.append(target);

    component = mount(SettingsModal, {
      target,
      props: {
        presentation: "mobile",
        initialSection: "distractions",
        initialDistractionsTab: "limits",
        onClose: () => undefined,
      },
    });
    await tick();

    const addLimitButton = [...target.querySelectorAll("button")]
      .find((button) => button.textContent?.trim() === "Add limit");
    expect(addLimitButton).toBeDefined();
    addLimitButton?.click();

    await vi.waitFor(() => {
      expect(target?.querySelector("[data-settings-section-test-stub]")).not.toBeNull();
    });
    const backButton = target.querySelector<HTMLButtonElement>(
      '[aria-label="Back to section"]',
    );
    expect(backButton).not.toBeNull();
    backButton?.click();
    await tick();

    expect(target.querySelector("[data-settings-section-test-stub]")).toBeNull();
    expect([...target.querySelectorAll("button")]
      .some((button) => button.textContent?.trim() === "Add limit")).toBe(true);
  });
});
