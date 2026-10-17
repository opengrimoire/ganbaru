import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { pickThemeJsonFile, saveThemeJsonFile } from "./json-file";

describe("theme JSON document adapter", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("requests a theme document without exposing its URI to the frontend", async () => {
    invoke.mockResolvedValueOnce('{"id":"midnight"}');

    await expect(pickThemeJsonFile()).resolves.toBe('{"id":"midnight"}');
    expect(invoke).toHaveBeenCalledWith("vault_pick_and_read_theme_json");
  });

  it("passes only the proposed name and validated JSON to the save command", async () => {
    const outcome = {
      saved: true,
      destination: "downloads",
      fileName: "midnight.json",
    } as const;
    invoke.mockResolvedValueOnce(outcome);

    await expect(
      saveThemeJsonFile("midnight.json", '{"id":"midnight"}'),
    ).resolves.toEqual(outcome);
    expect(invoke).toHaveBeenCalledWith("vault_pick_and_write_theme_json", {
      defaultName: "midnight.json",
      contents: '{"id":"midnight"}',
    });
  });
});
