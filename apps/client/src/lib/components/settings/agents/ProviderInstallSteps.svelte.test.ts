// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderFamilyMetadataRead, ProviderInstallGuide } from "$lib/chat/contracts";
import ProviderInstallSteps from "./ProviderInstallSteps.svelte";
import type { ProviderInstallSituation } from "./provider-install-steps";

const opener = vi.hoisted(() => ({ openUrl: vi.fn((): Promise<void> => Promise.resolve()) }));
vi.mock("@tauri-apps/plugin-opener", () => opener);

function installGuide(familyId: string): ProviderInstallGuide {
  return {
    shell: "terminal",
    command: `install ${familyId}`,
    signInCommand: `${familyId} login`,
    signInAfterInstallCommand: `${familyId} login`,
    docsUrl: `https://example.com/${familyId}`,
    requirement: null,
  };
}

function family(familyId: string): ProviderFamilyMetadataRead {
  return {
    familyId,
    displayName: familyId,
    configurationSchemaVersion: 1,
    supportedPlatforms: ["linux"],
    minimumTestedCliVersion: null,
    maximumTestedCliVersion: null,
    updateCommand: null,
    installGuide: installGuide(familyId),
    defaultExecutableCandidates: [familyId],
    implementationStatus: "available",
    maturity: "stable",
    protocolName: familyId,
    potentialCapabilities: [],
    unavailableReason: null,
  };
}

interface Deferred {
  promise: Promise<void>;
  resolve: () => void;
  reject: (reason: unknown) => void;
}

function deferred(): Deferred {
  let resolve = (): void => {};
  let reject = (_reason: unknown): void => {};
  const promise = new Promise<void>((onResolve, onReject) => {
    resolve = () => onResolve();
    reject = onReject;
  });
  return { promise, resolve, reject };
}

async function settle(): Promise<void> {
  for (let index = 0; index < 5; index += 1) {
    await Promise.resolve();
    flushSync();
  }
}

function buttonWithText(target: HTMLElement, text: string): HTMLButtonElement {
  const button = [...target.querySelectorAll<HTMLButtonElement>("button")]
    .find((candidate) => candidate.textContent?.trim().includes(text));
  if (!button) throw new Error(`No button with ${text}`);
  return button;
}

const situation: ProviderInstallSituation = { kind: "not-installed" };
let target: HTMLDivElement;
let component: ReturnType<typeof mount> | undefined;

describe("provider install steps", () => {
  beforeEach(() => {
    target = document.createElement("div");
    document.body.append(target);
    opener.openUrl.mockReset();
  });

  afterEach(() => {
    if (component) void unmount(component);
    component = undefined;
    target.remove();
    vi.restoreAllMocks();
  });

  it("keeps a refresh started for one family from disabling the next family's refresh", async () => {
    const pending = deferred();
    const onRefresh = vi.fn(() => pending.promise);
    const props = $state({ family: family("codex"), guide: installGuide("codex"), situation, onRefresh });
    component = mount(ProviderInstallSteps, { target, props });
    flushSync();

    buttonWithText(target, "Refresh").click();
    flushSync();
    expect(buttonWithText(target, "Refresh").disabled).toBe(true);

    props.family = family("claude");
    props.guide = installGuide("claude");
    flushSync();
    expect(buttonWithText(target, "Refresh").disabled).toBe(false);

    const next = deferred();
    onRefresh.mockImplementation(() => next.promise);
    buttonWithText(target, "Refresh").click();
    flushSync();
    expect(buttonWithText(target, "Refresh").disabled).toBe(true);

    pending.resolve();
    await settle();
    expect(buttonWithText(target, "Refresh").disabled).toBe(true);

    next.resolve();
    await settle();
    expect(buttonWithText(target, "Refresh").disabled).toBe(false);
  });

  it("keeps a guide link failure for one family off the next family", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const failed = "The guide could not be opened.";
    opener.openUrl.mockImplementation(() => Promise.reject(new Error("no opener")));
    const props = $state({ family: family("codex"), guide: installGuide("codex"), situation });
    component = mount(ProviderInstallSteps, { target, props });
    flushSync();

    buttonWithText(target, "example.com").click();
    await settle();
    expect(target.textContent).toContain(failed);

    props.family = family("claude");
    props.guide = installGuide("claude");
    flushSync();
    expect(target.textContent).not.toContain(failed);

    const pending = deferred();
    opener.openUrl.mockImplementation(() => pending.promise);
    buttonWithText(target, "example.com").click();
    props.family = family("gemini");
    props.guide = installGuide("gemini");
    flushSync();
    pending.reject(new Error("no opener"));
    await settle();
    expect(target.textContent).not.toContain(failed);
  });
});
