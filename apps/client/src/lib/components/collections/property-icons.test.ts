import { describe, expect, it } from "vitest";
import { collectionPropertyTypeSections, type CollectionPropertyTypeOption } from "./property-icons";

const OPTIONS: CollectionPropertyTypeOption<string>[] = [
  { value: "text", label: "Text", kind: "text", section: "basic" },
  { value: "relation", label: "Relation", kind: "relation", section: "advanced" },
  { value: "number", label: "Number", kind: "number", section: "basic" },
  { value: "created_time", label: "Created time", kind: "created_time", section: "automatic" },
];

/** Reduce sections to their values so assertions read as the picker's grid. */
function values(sections: CollectionPropertyTypeOption<string>[][]): string[][] {
  return sections.map((section) => section.map((option) => option.value));
}

describe("collection property type sections", () => {
  it("groups types by section in first-seen order while keeping their order inside a section", () => {
    expect(values(collectionPropertyTypeSections(OPTIONS, ""))).toEqual([["text", "number"], ["relation"], ["created_time"]]);
  });

  it("keeps unsectioned types together in one section", () => {
    const options = OPTIONS.map(({ section: _section, ...option }) => option);
    expect(values(collectionPropertyTypeSections(options, ""))).toEqual([["text", "relation", "number", "created_time"]]);
  });

  it("filters by label ignoring case and surrounding space, dropping sections left empty", () => {
    expect(values(collectionPropertyTypeSections(OPTIONS, "  e  "))).toEqual([["text", "number"], ["relation"], ["created_time"]]);
    expect(values(collectionPropertyTypeSections(OPTIONS, "TIME"))).toEqual([["created_time"]]);
    expect(values(collectionPropertyTypeSections(OPTIONS, "um"))).toEqual([["number"]]);
  });

  it("matches with the locale's case rules", () => {
    const options: CollectionPropertyTypeOption<string>[] = [{ value: "date", label: "İleri tarih", kind: "date" }];
    expect(values(collectionPropertyTypeSections(options, "ileri", "tr"))).toEqual([["date"]]);
  });

  it("returns no sections when nothing matches", () => {
    expect(collectionPropertyTypeSections(OPTIONS, "missing")).toEqual([]);
  });
});
