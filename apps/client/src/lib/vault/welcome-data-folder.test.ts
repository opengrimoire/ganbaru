import { describe, expect, it } from "vitest";
import { DATA_ENTRIES, entryExtension, vaultPathTrail } from "./welcome-data-folder";

describe("entryExtension", () => {
  it("reads the last extension in capitals", () => {
    expect(entryExtension("ganbaru.sqlite")).toBe("SQLITE");
    expect(entryExtension("archive.tar.gz")).toBe("GZ");
  });

  it("finds no extension in a bare name, a dotfile, or a trailing dot", () => {
    expect(entryExtension("assets")).toBe("");
    expect(entryExtension(".gitignore")).toBe("");
    expect(entryExtension("notes.")).toBe("");
  });
});

describe("welcome data folder entries", () => {
  it("lists folders before files", () => {
    const firstFile = DATA_ENTRIES.findIndex((entry) => entry.kind !== "folder");
    expect(DATA_ENTRIES.slice(firstFile).every((entry) => entry.kind !== "folder")).toBe(true);
  });

  it("names every entry once", () => {
    expect(new Set(DATA_ENTRIES.map((entry) => entry.name)).size).toBe(DATA_ENTRIES.length);
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
