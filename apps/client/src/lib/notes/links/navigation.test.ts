import { afterEach, describe, expect, it, vi } from "vitest";
import { normalizeNotesTextLinkUrl, notesLinkTarget, notesPastedTextLinkUrl, openNotesTextLink } from "./navigation";

const commands = vi.hoisted(() => ({
  openNotesLink: vi.fn(async (_target: { pageId: string; blockId?: string }) => true),
  openUrl: vi.fn(async (_url: string) => undefined),
}));
vi.mock("$lib/stores/notes.svelte", () => ({ getNotes: () => ({ openNotesLink: commands.openNotesLink }) }));
vi.mock("$lib/api/notes/knowledge", () => ({ openNotesExternalUrl: commands.openUrl }));
afterEach(() => { vi.clearAllMocks(); });

describe("Notes link navigation", () => {
  const pageId = "11111111-1111-4111-8111-111111111111";
  const blockId = "22222222-2222-4222-8222-222222222222";
  const hash = `#notes?page=${pageId}&block=${blockId}`;

  it("opens local database references through Notes rather than a web browser", async () => {
    await openNotesTextLink(hash);
    expect(commands.openNotesLink).toHaveBeenCalledExactlyOnceWith({ pageId, blockId });
    expect(commands.openUrl).not.toHaveBeenCalled();
  });

  it("opens web links through the system and rejects executable or filesystem destinations", async () => {
    await openNotesTextLink("https://example.com/tasks");
    expect(commands.openUrl).toHaveBeenCalledExactlyOnceWith("https://example.com/tasks");
    await expect(openNotesTextLink("javascript:alert(1)")).rejects.toThrow("invalid");
    await expect(openNotesTextLink("file:///tmp/private")).rejects.toThrow("invalid");
    expect(commands.openNotesLink).not.toHaveBeenCalled();
  });

  it("reports unavailable local links rather than silently succeeding", async () => {
    commands.openNotesLink.mockResolvedValueOnce(false);
    await expect(openNotesTextLink(hash)).rejects.toThrow("unavailable");
    expect(commands.openUrl).not.toHaveBeenCalled();
  });

  it("recognizes this app's copied absolute links without treating foreign links as local", () => {
    const base = "tauri://localhost/index.html";
    expect(notesLinkTarget(`tauri://localhost/${hash}`, base)).toEqual({ pageId, blockId });
    expect(notesLinkTarget(`tauri://foreign/${hash}`, base)).toBeNull();
    expect(notesLinkTarget(`https://example.com/${hash}`, base)).toBeNull();
    expect(notesLinkTarget("#notes?page=bad", base)).toBeNull();
  });

  it("stores copied app URLs as portable local references while retaining foreign web URLs", () => {
    const base = "https://tauri.localhost/index.html";
    expect(normalizeNotesTextLinkUrl(`https://tauri.localhost/${hash}`, base)).toBe(hash);
    expect(notesPastedTextLinkUrl(` https://tauri.localhost/${hash} `, base)).toBe(hash);
    expect(notesPastedTextLinkUrl(`tauri://localhost/${hash}`, "tauri://localhost/")).toBe(hash);
    expect(normalizeNotesTextLinkUrl(`https://example.com/${hash}`, base)).toBe(`https://example.com/${hash}`);
    expect(notesPastedTextLinkUrl("Task list", base)).toBeNull();
  });
});
