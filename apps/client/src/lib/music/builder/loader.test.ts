import { describe, expect, it, vi } from "vitest";
import type { MusicBuilderComponent } from "./loader";
import { createLazyMusicBuilderLoader } from "./loader";

describe("lazy music builder loader", () => {
  it("deduplicates concurrent loads and caches the constructor", async () => {
    const component = (() => undefined) as unknown as MusicBuilderComponent;
    const importer = vi.fn(async () => ({ default: component }));
    const loader = createLazyMusicBuilderLoader(importer);
    const [first, second] = await Promise.all([loader.load(), loader.load()]);
    expect(first).toBe(component);
    expect(second).toBe(component);
    expect(await loader.load()).toBe(component);
    expect(importer).toHaveBeenCalledOnce();
  });

  it("allows retry after a failed chunk request", async () => {
    const component = (() => undefined) as unknown as MusicBuilderComponent;
    const importer = vi.fn()
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce({ default: component });
    const loader = createLazyMusicBuilderLoader(importer);
    await expect(loader.load()).rejects.toThrow("offline");
    expect(await loader.load()).toBe(component);
    expect(importer).toHaveBeenCalledTimes(2);
  });
});
