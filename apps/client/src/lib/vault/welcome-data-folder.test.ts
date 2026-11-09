import { describe, expect, it } from "vitest";
import { DATA_ENTRIES, splitVaultPath } from "./welcome-data-folder";

describe("welcome data folder entries", () => {
  it("lists folders before files", () => {
    const firstFile = DATA_ENTRIES.findIndex((entry) => !entry.folder);
    expect(DATA_ENTRIES.slice(firstFile).every((entry) => !entry.folder)).toBe(true);
  });

  it("names every entry once", () => {
    expect(new Set(DATA_ENTRIES.map((entry) => entry.name)).size).toBe(DATA_ENTRIES.length);
  });
});

describe("split vault path", () => {
  it("splits a Unix path into the folder and its parent", () => {
    expect(splitVaultPath("/home/ana/Documents/Ganbaru")).toEqual({ name: "Ganbaru", parent: "/home/ana/Documents" });
  });

  it("splits Windows paths on backslashes", () => {
    expect(splitVaultPath("C:\\Users\\Ana\\Documents\\Ganbaru Dev")).toEqual({
      name: "Ganbaru Dev",
      parent: "C:\\Users\\Ana\\Documents",
    });
  });

  it("ignores trailing and repeated separators", () => {
    expect(splitVaultPath("Documents//Ganbaru/")).toEqual({ name: "Ganbaru", parent: "Documents" });
  });

  it("keeps the separator of a root or drive parent", () => {
    expect(splitVaultPath("/Ganbaru")).toEqual({ name: "Ganbaru", parent: "/" });
    expect(splitVaultPath("D:\\Ganbaru")).toEqual({ name: "Ganbaru", parent: "D:\\" });
  });

  it("has no parent for a bare folder name", () => {
    expect(splitVaultPath("Ganbaru")).toEqual({ name: "Ganbaru", parent: null });
  });

  it("keeps a lone root as the name", () => {
    expect(splitVaultPath("/")).toEqual({ name: "/", parent: null });
  });
});
