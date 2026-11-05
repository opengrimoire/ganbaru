import { describe, expect, it } from "vitest";
import licenseSource from "../../../../../LICENSE?raw";
import { type LicenseBlock, parseLicenseText } from "./license-text";

function blockWords(block: LicenseBlock): string[] {
  const text = "lines" in block ? block.lines.join(" ") : block.text;
  return text.split(/\s+/).filter(Boolean);
}

describe("parseLicenseText", () => {
  it("keeps every word of the repository license in order", () => {
    const words = parseLicenseText(licenseSource).flatMap(blockWords);
    expect(words).toEqual(licenseSource.split(/\s+/).filter(Boolean));
  });

  it("reads the centered name and version as the title and later centered lines as headings", () => {
    const blocks = parseLicenseText(licenseSource);
    expect(blocks[0]).toEqual({ kind: "title", lines: ["GNU AFFERO GENERAL PUBLIC LICENSE", "Version 3, 19 November 2007"] });
    expect(blocks).toContainEqual({ kind: "heading", text: "Preamble" });
    expect(blocks).toContainEqual({ kind: "heading", text: "TERMS AND CONDITIONS" });
    expect(blocks).toContainEqual({ kind: "heading", text: "How to Apply These Terms to Your New Programs" });
  });

  it("finds every numbered term as a section", () => {
    const sections = parseLicenseText(licenseSource).filter((block) => block.kind === "section");
    expect(sections).toHaveLength(18);
    expect(sections[0]).toEqual({ kind: "section", text: "0. Definitions." });
    expect(sections[17]).toEqual({ kind: "section", text: "17. Interpretation of Sections 15 and 16." });
  });

  it("joins hard-wrapped lines of a paragraph and a lettered clause", () => {
    const source = ["  First line of a", "paragraph that wraps.", "", "    a) A clause that", "    keeps going."].join("\n");
    expect(parseLicenseText(source)).toEqual([
      { kind: "paragraph", text: "First line of a paragraph that wraps." },
      { kind: "item", text: "a) A clause that keeps going." },
    ]);
  });

  it("keeps the line breaks of an indented sample notice", () => {
    const source = ["  Attach this:", "", "    <program name>", "    Copyright (C) <year>  <author>"].join("\n");
    expect(parseLicenseText(source)[1]).toEqual({ kind: "notice", lines: ["<program name>", "Copyright (C) <year>  <author>"] });
  });

  it("treats Windows line endings and whitespace-only separators as plain blank lines", () => {
    const source = "                Title\r\n   \r\nBody line one\r\nline two\r\n\r\n";
    expect(parseLicenseText(source)).toEqual([
      { kind: "title", lines: ["Title"] },
      { kind: "paragraph", text: "Body line one line two" },
    ]);
  });

  it("returns no blocks for empty text", () => {
    expect(parseLicenseText("\n \n")).toEqual([]);
  });
});
