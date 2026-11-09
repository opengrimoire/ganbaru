import { describe, expect, it } from "vitest";
import {
  DATA_BEATS,
  DATA_ENTRIES,
  dataEntryLit,
  dataEntryReveal,
  dataEntryStart,
  dataFolderReveal,
  vaultPathTrail,
} from "./welcome-data-scene";

/** Every step of the scene, for properties that must hold throughout. */
function moments(step = 10): number[] {
  return Array.from({ length: Math.floor(DATA_BEATS.end / step) + 1 }, (_, index) => index * step);
}

describe("welcome data scene", () => {
  it("starts with an empty folder", () => {
    expect(dataFolderReveal(0)).toBe(0);
    for (const [index] of DATA_ENTRIES.entries()) {
      expect(dataEntryReveal(0, index)).toBe(0);
      expect(dataEntryLit(0, index)).toBe(0);
    }
  });

  it("shows the folder before any of its entries", () => {
    expect(DATA_BEATS.folderIn).toBeLessThanOrEqual(DATA_BEATS.firstEntry);
    expect(dataFolderReveal(DATA_BEATS.firstEntry)).toBe(1);
  });

  it("brings the entries in one after another, in list order", () => {
    for (let index = 1; index < DATA_ENTRIES.length; index += 1) {
      expect(dataEntryStart(index)).toBeGreaterThan(dataEntryStart(index - 1));
      for (const elapsed of moments()) {
        expect(dataEntryReveal(elapsed, index)).toBeLessThanOrEqual(dataEntryReveal(elapsed, index - 1));
      }
    }
  });

  it("never hides an entry once it has appeared", () => {
    for (const [index] of DATA_ENTRIES.entries()) {
      let previous = 0;
      for (const elapsed of moments()) {
        const reveal = dataEntryReveal(elapsed, index);
        expect(reveal).toBeGreaterThanOrEqual(previous);
        previous = reveal;
      }
    }
  });

  it("lights each entry as it arrives and ends with every entry shown and unlit", () => {
    for (const [index] of DATA_ENTRIES.entries()) {
      const peak = Math.max(...moments(5).map((elapsed) => dataEntryLit(elapsed, index)));
      expect(peak).toBeCloseTo(1);
      expect(dataEntryReveal(DATA_BEATS.end, index)).toBe(1);
      expect(dataEntryLit(DATA_BEATS.end, index)).toBe(0);
    }
  });

  it("lists folders before files", () => {
    const firstFile = DATA_ENTRIES.findIndex((entry) => !entry.folder);
    expect(DATA_ENTRIES.slice(firstFile).every((entry) => !entry.folder)).toBe(true);
  });
});

describe("vault path trail", () => {
  it("keeps the last folders of a Unix path", () => {
    expect(vaultPathTrail("/home/ana/Documents/Ganbaru")).toEqual(["Documents", "Ganbaru"]);
  });

  it("splits Windows paths on backslashes", () => {
    expect(vaultPathTrail("C:\\Users\\Ana\\Documents\\Ganbaru Dev")).toEqual(["Documents", "Ganbaru Dev"]);
  });

  it("ignores trailing and repeated separators", () => {
    expect(vaultPathTrail("Documents//Ganbaru/")).toEqual(["Documents", "Ganbaru"]);
  });

  it("returns what there is when the path is shorter than the depth", () => {
    expect(vaultPathTrail("Ganbaru", 3)).toEqual(["Ganbaru"]);
    expect(vaultPathTrail("")).toEqual([]);
  });

  it("keeps at least one folder", () => {
    expect(vaultPathTrail("/a/b/c", 0)).toEqual(["c"]);
  });
});
