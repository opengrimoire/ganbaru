// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type {
  ChatWorkspaceChangeBatch,
  ProjectWorkingFolderDirectoryRead,
  ProjectWorkingFolderFileEntry,
} from "$lib/chat/contracts";
import ChatFilesPanel from "./ChatFilesPanel.svelte";

const api = vi.hoisted(() => ({
  listProjectWorkingFolderDirectory: vi.fn<(
    workingFolderId: string,
    relativePath: string,
    includeIgnored: boolean,
    executionEnvironmentId: string | null,
  ) => Promise<ProjectWorkingFolderDirectoryRead>>(),
}));
vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  listProjectWorkingFolderDirectory: api.listProjectWorkingFolderDirectory,
}));
const observer = vi.hoisted(() => ({
  changeListeners: new Set<(batch: ChatWorkspaceChangeBatch) => void>(),
}));
vi.mock("$lib/chat/workspace/observer-client", () => ({
  subscribeChatWorkspaceChanges: (listener: (batch: ChatWorkspaceChangeBatch) => void) => {
    observer.changeListeners.add(listener);
    return () => observer.changeListeners.delete(listener);
  },
  subscribeChatWorkspaceObserverStatus: () => () => {},
}));

function entry(relativePath: string, kind: ProjectWorkingFolderFileEntry["kind"]): ProjectWorkingFolderFileEntry {
  return { relativePath, displayName: relativePath.slice(relativePath.lastIndexOf("/") + 1), kind, ignored: false, byteSize: null };
}

function listing(relativePath: string, entries: ProjectWorkingFolderFileEntry[]): ProjectWorkingFolderDirectoryRead {
  return { relativePath, entries, truncated: false };
}

async function settle(): Promise<void> {
  for (let round = 0; round < 6; round += 1) {
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  flushSync();
}

describe("ChatFilesPanel", () => {
  const mounted: { target: HTMLDivElement; component: ReturnType<typeof mount> }[] = [];

  beforeAll(() => {
    globalThis.ResizeObserver ??= class {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    };
  });

  afterEach(async () => {
    while (mounted.length > 0) {
      const entry = mounted.pop();
      if (!entry) continue;
      await unmount(entry.component);
      entry.target.remove();
    }
    observer.changeListeners.clear();
    api.listProjectWorkingFolderDirectory.mockReset();
  });

  it("shows an expanding folder's files when a workspace change refreshes the tree during its read", async () => {
    let finishSource = null as ((read: ProjectWorkingFolderDirectoryRead) => void) | null;
    api.listProjectWorkingFolderDirectory.mockImplementation(async (_folder, relativePath) => {
      if (relativePath === "src") return new Promise((resolve) => { finishSource = resolve; });
      return listing("", [entry("src", "directory"), entry("README.md", "file")]);
    });
    const target = document.createElement("div");
    document.body.append(target);
    const component = mount(ChatFilesPanel, {
      target,
      props: {
        folderId: "folder-a",
        folderEnvironmentId: null,
        directoryPath: "",
        selectedPath: null,
        treeVisible: true,
        treeWidthPx: 220,
        onStateChange: () => {},
      },
    });
    mounted.push({ target, component });
    await settle();

    target.querySelector<HTMLButtonElement>("button[aria-label='Expand src']")?.click();
    await settle();
    expect(finishSource).not.toBeNull();
    for (const listener of observer.changeListeners) {
      listener({
        workingFolderId: "folder-a",
        executionEnvironmentId: null,
        generation: 1,
        relativePaths: ["README.md"],
        affectedParentDirectories: [""],
        renames: [],
        gitMetadataChanged: false,
        overflowed: false,
        degradedReason: null,
      });
    }
    await settle();
    finishSource?.(listing("src", [entry("src/main.ts", "file")]));
    await settle();

    const labels = [...target.querySelectorAll("[role='treeitem']")].map((row) => row.textContent?.trim());
    expect(labels).toContain("main.ts");
  });
});
