// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ChatProjectPrimaryWorkingFolderRead,
  ProjectWorkingFolderRead,
  ProviderFamilyMetadataRead,
  ProviderInstanceRead,
} from "$lib/chat/contracts";
import ProjectSettingsWorkingFoldersSection from "$lib/components/projects/settings/ProjectSettingsWorkingFoldersSection.svelte";
import type { ProjectSettingsWorkingFoldersPorts, WorkingFolderSelection } from "$lib/projects/settings/working-folders-draft.svelte";

const chat = vi.hoisted(() => ({
  workingFolders: [] as ProjectWorkingFolderRead[],
  loading: false,
  settings: {
    providerInstances: [] as ProviderInstanceRead[],
    providerFamilies: [] as ProviderFamilyMetadataRead[],
    configuration: { workingFolderProviderPreferences: {} as Record<string, string> },
  },
  selectedChannel: null as { projectId: string } | null,
  primaryWorkingFolder: null as ChatProjectPrimaryWorkingFolderRead | null,
  selectedWorkingFolderId: null as string | null,
  ensureLoaded: vi.fn<() => Promise<void>>(),
  openWorkingFolder: vi.fn<(id: string) => Promise<void>>(),
  setWorkingFolderProviderPreference: vi.fn<(id: string, providerId: string | null) => Promise<void>>(),
  addExternalWorkingFolder: vi.fn<() => Promise<void>>(),
  removeWorkingFolder: vi.fn<(id: string) => Promise<void>>(),
  renameWorkingFolder: vi.fn<() => Promise<void>>(),
  archiveWorkingFolder: vi.fn<() => Promise<void>>(),
  restoreWorkingFolder: vi.fn<() => Promise<void>>(),
  recreateManagedWorkingFolder: vi.fn<(id: string) => Promise<void>>(),
  locateWorkingFolder: vi.fn<(id: string, title: string) => Promise<void>>(),
  rebindWorkingFolder: vi.fn<() => Promise<void>>(),
}));
const api = vi.hoisted(() => ({
  readChatProjectPrimaryWorkingFolder: vi.fn<() => Promise<ChatProjectPrimaryWorkingFolderRead>>(),
  setChatProjectPrimaryWorkingFolder: vi.fn<() => Promise<ChatProjectPrimaryWorkingFolderRead>>(),
}));
vi.mock("$lib/stores/chat.svelte", () => ({ getChat: () => chat }));
vi.mock("$lib/api/chat", () => api);
const folderApi = vi.hoisted(() => ({
  pickProjectWorkingFolder: vi.fn<(projectId: string, title: string, id?: string) => Promise<WorkingFolderSelection | null>>(),
  addSelectedProjectWorkingFolder: vi.fn<ProjectSettingsWorkingFoldersPorts["add"]>(),
  bindSelectedProjectWorkingFolder: vi.fn<ProjectSettingsWorkingFoldersPorts["bind"]>(),
  renameProjectWorkingFolder: vi.fn<ProjectSettingsWorkingFoldersPorts["rename"]>(),
  archiveProjectWorkingFolder: vi.fn<ProjectSettingsWorkingFoldersPorts["archive"]>(),
  restoreProjectWorkingFolder: vi.fn<ProjectSettingsWorkingFoldersPorts["restore"]>(),
  recreateManagedProjectWorkingFolder: vi.fn<ProjectSettingsWorkingFoldersPorts["recreate"]>(),
}));
vi.mock("$lib/api/project-working-folders", () => folderApi);

let component: (ReturnType<typeof mount> & { saveDraft: () => Promise<void>; discardDraft: () => void }) | undefined;
let target: HTMLDivElement;

beforeEach(() => {
  vi.resetAllMocks();
  chat.ensureLoaded.mockResolvedValue(undefined);
  chat.loading = false;
  chat.openWorkingFolder.mockResolvedValue(undefined);
  chat.setWorkingFolderProviderPreference.mockResolvedValue(undefined);
  chat.removeWorkingFolder.mockResolvedValue(undefined);
  chat.locateWorkingFolder.mockResolvedValue(undefined);
  chat.recreateManagedWorkingFolder.mockResolvedValue(undefined);
  folderApi.pickProjectWorkingFolder.mockResolvedValue(null);
  folderApi.recreateManagedProjectWorkingFolder.mockImplementation(async (id) => folder(id, "managed"));
  api.setChatProjectPrimaryWorkingFolder.mockResolvedValue({ projectId: "project", workingFolderId: "external", revision: 4 });
  api.readChatProjectPrimaryWorkingFolder.mockResolvedValue({ projectId: "project", workingFolderId: "managed", revision: 3 });
  chat.workingFolders = [folder("managed", "managed"), folder("external", "external")];
  chat.selectedChannel = null;
  chat.primaryWorkingFolder = null;
  chat.selectedWorkingFolderId = null;
  chat.settings.configuration.workingFolderProviderPreferences = {};
  chat.settings.providerInstances = [provider("local", "codex", "Local provider")];
  chat.settings.providerFamilies = [family("codex", "OpenAI"), family("claude", "Anthropic")];
  target = document.createElement("div");
  target.dataset.floatingRoot = "";
  document.body.append(target);
});

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  target.remove();
  vi.restoreAllMocks();
});

/** Build a managed or external project folder with a healthy device binding. */
function folder(id: string, kind: "managed" | "external"): ProjectWorkingFolderRead {
  return {
    workingFolder: {
      id, projectId: "project", displayName: id, kind,
      managedRelativePath: kind === "managed" ? "projects/project" : null,
      repositoryKind: "none", repositoryIdentity: null, sortOrder: 0,
      createdAt: "2026-10-01T00:00:00Z", updatedAt: "2026-10-01T00:00:00Z",
      archivedAt: null, revision: 1,
    },
    bindingStatus: "available", canonicalPath: `/folders/${id}`,
    lastVerifiedAt: "2026-10-01T00:00:00Z", currentBranch: null,
  };
}

/** Describe a provider family even when no instance has been configured. */
function family(familyId: string, displayName: string): ProviderFamilyMetadataRead {
  return {
    familyId, displayName, configurationSchemaVersion: 1, supportedPlatforms: ["linux"],
    minimumTestedCliVersion: null, defaultExecutableCandidates: [],
    implementationStatus: "available", maturity: "experimental", protocolName: "test",
    potentialCapabilities: [], unavailableReason: null,
  };
}

/** Build a configured instance without involving native provider discovery. */
function provider(instanceId: string, familyId: string, label: string): ProviderInstanceRead {
  return {
    configuration: {
      schemaVersion: 1, instanceId, familyId, label, enabled: true,
      executable: "provider", providerHome: null, launchArguments: [], environment: {},
      credentialReferences: {}, visibleModelIds: [], favoriteModelIds: [],
      providerConfig: { schemaVersion: 1, value: {} },
    },
    lastProbe: null, modelCatalog: null, lastSuccessfulProbeAt: null,
  };
}

/** Mount the folder section and let initial native reads finish. */
async function render(onDirtyChange: (dirty: boolean) => void = () => {}): Promise<void> {
  component = mount(ProjectSettingsWorkingFoldersSection, { target, props: { projectId: "project", onDirtyChange } });
  await tick();
  await tick();
  await vi.waitFor(() => expect(target.querySelector('[aria-label="Options for managed"]')).not.toBeNull());
}

/** Open a folder's secondary controls through its visible options trigger. */
async function openFolderOptions(name: string): Promise<HTMLElement> {
  const trigger = target.querySelector<HTMLButtonElement>(`[aria-label="Options for ${name}"]`)!;
  await vi.waitFor(() => expect(trigger.disabled).toBe(false));
  if (trigger.getAttribute("aria-expanded") !== "true") trigger.click();
  await vi.waitFor(() => expect(target.querySelector('[role="dialog"]')).not.toBeNull());
  const menu = target.querySelector<HTMLElement>('[role="dialog"]')!;
  await vi.waitFor(() => expect(menu.contains(document.activeElement)).toBe(true));
  return menu;
}

/** Find an action by its visible text in the current folder menu. */
function action(menu: HTMLElement, label: string): HTMLButtonElement {
  const button = [...menu.querySelectorAll<HTMLButtonElement>("button")]
    .find((button) => button.textContent?.trim() === label);
  if (!button) throw new Error(`Missing folder action: ${label}`);
  return button;
}

describe("Project working folder settings", () => {
  it("reports pending folder changes to the parent and restores them through Discard", async () => {
    const dirty = vi.fn();
    await render(dirty);
    const trigger = target.querySelector<HTMLButtonElement>('[aria-label="AI provider for managed"]')!;
    trigger.click();
    await vi.waitFor(() => expect(target.querySelector('[role="listbox"]')).not.toBeNull());
    action(target.querySelector<HTMLElement>('[role="listbox"]')!, "Local provider").click();
    await tick();
    expect(dirty).toHaveBeenLastCalledWith(true);
    component!.discardDraft();
    await tick();
    expect(trigger.textContent).toContain("Automatic");
    expect(dirty).toHaveBeenLastCalledWith(false);
    expect(chat.setWorkingFolderProviderPreference).not.toHaveBeenCalled();
  });

  it("previews a folder provider from its row and persists it only on Save", async () => {
    await render();
    const providerTrigger = target.querySelector<HTMLButtonElement>('[aria-label="AI provider for managed"]')!;
    providerTrigger.click();
    await vi.waitFor(() => expect(target.querySelector('[role="listbox"]')).not.toBeNull());
    const menu = target.querySelector<HTMLElement>('[role="listbox"]')!;
    expect(menu.querySelector("svg")).not.toBeNull();
    action(menu, "Local provider").click();
    await tick();
    expect(chat.setWorkingFolderProviderPreference).not.toHaveBeenCalled();
    expect(providerTrigger.textContent).toContain("Local provider");
    expect(menu.isConnected).toBe(false);
    const folderMenu = await openFolderOptions("managed");
    expect(folderMenu.querySelector('[role="listbox"]')).toBeNull();
    expect(action(folderMenu, "Remove from project").disabled).toBe(true);
    await component!.saveDraft();
    expect(chat.setWorkingFolderProviderPreference).toHaveBeenCalledExactlyOnceWith("managed", "local");
  });

  it("shows the full managed path and keeps protected actions visible without allowing changes", async () => {
    const prompt = vi.spyOn(window, "prompt");
    const confirm = vi.spyOn(window, "confirm");
    await render();
    const menu = await openFolderOptions("managed");
    expect(menu.textContent).toContain("/folders/managed");
    expect(menu.textContent).not.toContain("projects/project");
    expect([...menu.querySelectorAll("button")].map((button) => button.textContent?.trim())).toEqual([
      "Open", "Primary", "Rebind", "Rename", "Archive", "Remove from project",
    ]);
    expect(action(menu, "Open").disabled).toBe(false);
    for (const label of ["Primary", "Rebind", "Rename", "Archive", "Remove from project"]) {
      const button = action(menu, label);
      expect(button.disabled).toBe(true);
      button.click();
    }
    expect(prompt).not.toHaveBeenCalled();
    expect(confirm).not.toHaveBeenCalled();
    expect(api.setChatProjectPrimaryWorkingFolder).not.toHaveBeenCalled();
    expect(chat.rebindWorkingFolder).not.toHaveBeenCalled();
    expect(chat.renameWorkingFolder).not.toHaveBeenCalled();
    expect(chat.archiveWorkingFolder).not.toHaveBeenCalled();
    expect(chat.removeWorkingFolder).not.toHaveBeenCalled();
  });

  it.each(["missing", "unbound", "repository_mismatch"] as const)("offers managed-folder recovery for a %s binding", async (bindingStatus) => {
    chat.workingFolders[0].bindingStatus = bindingStatus;
    if (bindingStatus === "unbound") chat.workingFolders[0].canonicalPath = null;
    await render();
    const menu = await openFolderOptions("managed");
    expect(action(menu, "Open").disabled).toBe(true);
    if (bindingStatus === "unbound") {
      expect(menu.textContent).toContain("No local path");
      expect(menu.textContent).not.toContain("projects/project");
    }
    action(menu, "Recreate").click();
    await tick();
    expect(folderApi.recreateManagedProjectWorkingFolder).not.toHaveBeenCalled();
    await component!.saveDraft();
    expect(folderApi.recreateManagedProjectWorkingFolder).toHaveBeenCalledExactlyOnceWith("managed");
  });

  it("shows every provider family and each configured instance while preventing unconfigured preferences", async () => {
    chat.settings.providerInstances.push(provider("second", "codex", "Second account"));
    chat.settings.providerFamilies.push(family("cursor", "Cursor"), family("grok", "Grok"), family("opencode", "OpenCode"));
    await render();
    target.querySelector<HTMLButtonElement>('[aria-label="AI provider for external"]')!.click();
    await vi.waitFor(() => expect(target.querySelector('[role="listbox"]')).not.toBeNull());
    const menu = target.querySelector<HTMLElement>('[role="listbox"]')!;
    const options = [...menu.querySelectorAll<HTMLButtonElement>('[role="option"]')];
    expect(options).toHaveLength(7);
    expect(options.map((option) => option.textContent?.trim())).toEqual([
      "Automatic", "Local provider", "Second account", "Anthropic Not configured",
      "Cursor Not configured", "Grok Not configured", "OpenCode Not configured",
    ]);
    for (const option of options.slice(3)) {
      expect(option.disabled).toBe(true);
      expect(option.querySelector("svg")).not.toBeNull();
      option.click();
    }
    expect(chat.setWorkingFolderProviderPreference).not.toHaveBeenCalled();
    action(menu, "Second account").click();
    await tick();
    expect(chat.setWorkingFolderProviderPreference).not.toHaveBeenCalled();
    await component!.saveDraft();
    expect(chat.setWorkingFolderProviderPreference).toHaveBeenCalledExactlyOnceWith("external", "second");
  });

  it("keeps a removed provider visible and lets Automatic clear the stale preference", async () => {
    chat.settings.configuration.workingFolderProviderPreferences.managed = "removed-provider";
    await render();
    const trigger = target.querySelector<HTMLButtonElement>('[aria-label="AI provider for managed"]')!;
    expect(trigger.textContent).toContain("Unavailable provider");
    trigger.click();
    await vi.waitFor(() => expect(target.querySelector('[role="listbox"]')).not.toBeNull());
    const menu = target.querySelector<HTMLElement>('[role="listbox"]')!;
    expect(action(menu, "Unavailable provider").disabled).toBe(true);
    action(menu, "Automatic").click();
    await tick();
    expect(chat.setWorkingFolderProviderPreference).not.toHaveBeenCalled();
    await component!.saveDraft();
    expect(chat.setWorkingFolderProviderPreference).toHaveBeenCalledExactlyOnceWith("managed", null);
  });

  it("keeps missing-folder recovery accessible and confirms removing only the external association", async () => {
    chat.workingFolders[1].bindingStatus = "missing";
    await render();
    expect(target.textContent).toContain("Folder is missing");
    expect(target.querySelector('[aria-label="Open external"]')).toBeNull();
    let menu = await openFolderOptions("external");
    expect(action(menu, "Open").disabled).toBe(true);
    action(menu, "Locate").click();
    await vi.waitFor(() => expect(folderApi.pickProjectWorkingFolder).toHaveBeenCalledExactlyOnceWith("project", "Choose a project working folder", "external"));
    expect(chat.locateWorkingFolder).not.toHaveBeenCalled();
    await tick();
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    menu = await openFolderOptions("external");
    action(menu, "Remove from project").click();
    await tick();
    expect(confirm).toHaveBeenCalledWith("Remove external from this project? Files on disk will not be deleted.");
    expect(chat.removeWorkingFolder).not.toHaveBeenCalled();
    confirm.mockReturnValue(true);
    menu = await openFolderOptions("external");
    action(menu, "Remove from project").click();
    await tick();
    expect(chat.removeWorkingFolder).not.toHaveBeenCalled();
    await component!.saveDraft();
    expect(chat.removeWorkingFolder).toHaveBeenCalledExactlyOnceWith("external");
  });

  it("keeps primary selection in the open menu and applies it to Chat only on Save", async () => {
    chat.selectedChannel = { projectId: "project" };
    const primary = { projectId: "project", workingFolderId: "external", revision: 4 };
    let resolvePrimary: (primary: ChatProjectPrimaryWorkingFolderRead) => void = () => { throw new Error("Primary request was not started"); };
    api.setChatProjectPrimaryWorkingFolder.mockImplementation(() => new Promise<ChatProjectPrimaryWorkingFolderRead>((resolve) => { resolvePrimary = resolve; }));
    await render();
    const menu = await openFolderOptions("external");
    const primaryButton = action(menu, "Make primary");
    const actionClass = primaryButton.className;
    primaryButton.focus();
    primaryButton.click();
    await tick();
    expect(api.setChatProjectPrimaryWorkingFolder).not.toHaveBeenCalled();
    expect(chat.primaryWorkingFolder).toBeNull();
    expect(chat.selectedWorkingFolderId).toBeNull();
    expect(menu.isConnected).toBe(true);
    expect(action(menu, "Primary")).toBe(primaryButton);
    const save = component!.saveDraft();
    await vi.waitFor(() => expect(api.setChatProjectPrimaryWorkingFolder).toHaveBeenCalledExactlyOnceWith("project", "external", 3));
    resolvePrimary(primary);
    await save;
    await vi.waitFor(() => expect(chat.primaryWorkingFolder).toEqual(primary));
    expect(chat.selectedWorkingFolderId).toBe("external");
    expect(menu.isConnected).toBe(true);
    expect(action(menu, "Primary")).toBe(primaryButton);
    expect(primaryButton.className).toBe(actionClass);
    expect(primaryButton.disabled).toBe(true);
    expect(primaryButton.getAttribute("aria-pressed")).toBe("true");
    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await tick();
    expect(menu.isConnected).toBe(false);
    expect(document.activeElement).toBe(target.querySelector('[aria-label="Options for external"]'));
  });

  it("locks other folder operations during a native request and surfaces the failure", async () => {
    let rejectOpen: (error: Error) => void = () => { throw new Error("Open request was not started"); };
    chat.openWorkingFolder.mockImplementation(() => new Promise<void>((_resolve, reject) => { rejectOpen = reject; }));
    await render();
    const menu = await openFolderOptions("managed");
    menu.querySelector<HTMLButtonElement>('[aria-label="Open managed"]')!.click();
    await tick();
    expect(target.querySelector<HTMLButtonElement>('[aria-label="AI provider for external"]')?.disabled).toBe(true);
    expect(target.querySelector<HTMLButtonElement>('[aria-label="Options for external"]')?.disabled).toBe(true);
    rejectOpen(new Error("Folder could not be opened"));
    await vi.waitFor(() => expect(target.querySelector('[role="alert"]')?.textContent?.trim()).toBe("Folder could not be opened"));
    expect(target.querySelector<HTMLButtonElement>('[aria-label="AI provider for external"]')?.disabled).toBe(false);
  });
});
