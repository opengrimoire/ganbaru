// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderFamilyMetadataRead } from "$lib/chat/contracts";
import UnconfiguredProviderFamily from "./UnconfiguredProviderFamily.svelte";

const chat = vi.hoisted(() => ({
  settings: null as { configuration: { automaticProviderSetupDisabled: string[] }; providerInstances: { configuration: { familyId: string } }[] } | null,
  refreshAllProviders: vi.fn((): Promise<unknown> => Promise.resolve()),
}));

vi.mock("$lib/stores/chat.svelte", () => ({ getChat: () => chat }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

function family(familyId: string, displayName: string): ProviderFamilyMetadataRead {
  return {
    familyId,
    displayName,
    configurationSchemaVersion: 1,
    supportedPlatforms: ["linux"],
    minimumTestedCliVersion: null,
    maximumTestedCliVersion: null,
    updateCommand: null,
    installGuide: {
      shell: "terminal",
      command: `install ${familyId}`,
      signInCommand: `${familyId} login`,
      signInAfterInstallCommand: `${familyId} login`,
      docsUrl: `https://example.com/${familyId}`,
      requirement: null,
    },
    defaultExecutableCandidates: [familyId],
    implementationStatus: "available",
    maturity: "stable",
    protocolName: familyId,
    potentialCapabilities: [],
    unavailableReason: null,
  };
}

interface Deferred {
  promise: Promise<unknown>;
  resolve: () => void;
}

function deferred(): Deferred {
  let resolve = (): void => {};
  const promise = new Promise<unknown>((onResolve) => {
    resolve = () => onResolve(undefined);
  });
  return { promise, resolve };
}

async function settle(): Promise<void> {
  for (let index = 0; index < 5; index += 1) {
    await Promise.resolve();
    flushSync();
  }
}

function refreshButton(target: HTMLElement): HTMLButtonElement {
  const button = [...target.querySelectorAll<HTMLButtonElement>("button")]
    .find((candidate) => candidate.textContent?.trim() === "Refresh");
  if (!button) throw new Error("No refresh button");
  return button;
}

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | undefined;

describe("Unconfigured provider family", () => {
  beforeEach(() => {
    target = document.createElement("div");
    document.body.append(target);
    chat.settings = { configuration: { automaticProviderSetupDisabled: [] }, providerInstances: [] };
    chat.refreshAllProviders.mockReset();
  });

  afterEach(() => {
    if (component) unmount(component);
    component = undefined;
    target.remove();
  });

  it("keeps a refresh outcome on the family it ran for when another family is selected", async () => {
    const pending = deferred();
    chat.refreshAllProviders.mockReturnValueOnce(pending.promise);
    const props = $state({
      family: family("claude", "Claude"),
      installedExecutables: ["/usr/bin/claude"] as readonly string[] | undefined,
      onSetUp: () => {},
    });
    component = mount(UnconfiguredProviderFamily, { target, props });
    flushSync();

    refreshButton(target).click();
    flushSync();
    props.family = family("codex", "Codex");
    props.installedExecutables = ["/usr/bin/codex"];
    flushSync();
    pending.resolve();
    await settle();

    expect(target.textContent).not.toContain("could not add");
  });

  it("does not carry a settled outcome over to the next family selected", async () => {
    const props = $state({
      family: family("claude", "Claude"),
      installedExecutables: ["/usr/bin/claude"] as readonly string[] | undefined,
      onSetUp: () => {},
    });
    component = mount(UnconfiguredProviderFamily, { target, props });
    flushSync();

    refreshButton(target).click();
    await settle();
    expect(target.textContent).toContain("Ganbaru could not add Claude yet.");

    props.family = family("codex", "Codex");
    props.installedExecutables = ["/usr/bin/codex"];
    flushSync();
    expect(target.textContent).not.toContain("could not add");
  });
});
