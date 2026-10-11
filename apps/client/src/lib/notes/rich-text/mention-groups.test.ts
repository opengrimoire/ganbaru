import { describe, expect, it } from "vitest";
import type { NotesDateMentionTarget, NotesNamedMentionTarget } from "./core";
import {
  buildNotesMentionGroups,
  flattenNotesMentionGroups,
  NOTES_MENTION_GROUP_LIMITS,
  notesMentionRowKey,
  type NotesMentionGroup,
} from "./mention-groups";

const today: NotesDateMentionTarget = {
  kind: "date",
  id: "date:2026-10-11",
  title: "Today",
  subtitle: "Date",
  date: { start: "2026-10-11", end: null, time_zone: null },
  reminder: false,
};

function pages(count: number): NotesNamedMentionTarget[] {
  return Array.from({ length: count }, (_, index) => ({
    kind: "page",
    id: `page-${index}`,
    title: `Page ${String(index).padStart(2, "0")}`,
  }));
}

const user: NotesNamedMentionTarget = { kind: "user", id: "user-1", title: "Alice", subtitle: "Local user" };
const project: NotesNamedMentionTarget = { kind: "project", id: "project-1", title: "Roadmap", subtitle: "Project" };

function summary(groups: readonly NotesMentionGroup[]): string[][] {
  return groups.map((group) => [group.kind, ...group.rows.map(notesMentionRowKey)]);
}

describe("Notes mention groups", () => {
  it("orders sections as dates, people, pages, then app objects and ends people with the invitation", () => {
    const groups = buildNotesMentionGroups({
      query: "",
      dateTargets: [today],
      namedTargets: [project, ...pages(1), user],
      canInvite: true,
    });

    expect(summary(groups)).toEqual([
      ["date", "date:date:2026-10-11"],
      ["people", "user:user-1", "invite"],
      ["pages", "page:page-0"],
      ["projects", "project:project-1"],
    ]);
  });

  it("truncates a long section behind a more row until that section is expanded", () => {
    const total = NOTES_MENTION_GROUP_LIMITS.pages + 3;
    const collapsed = buildNotesMentionGroups({ query: "", dateTargets: [], namedTargets: pages(total) });

    expect(collapsed[0]?.rows).toHaveLength(NOTES_MENTION_GROUP_LIMITS.pages + 1);
    expect(collapsed[0]?.rows.at(-1)).toEqual({ kind: "more", group: "pages", hiddenCount: 3 });

    const expanded = buildNotesMentionGroups({
      query: "",
      dateTargets: [],
      namedTargets: pages(total),
      expanded: new Set(["pages"]),
    });
    expect(expanded[0]?.rows).toHaveLength(total);
    expect(expanded[0]?.rows.every((row) => row.kind === "target")).toBe(true);
  });

  it("filters named targets by the query and drops empty sections", () => {
    const groups = buildNotesMentionGroups({
      query: "road",
      dateTargets: [],
      namedTargets: [user, project, ...pages(2)],
      canInvite: true,
    });

    expect(summary(groups)).toEqual([["projects", "project:project-1"]]);
  });

  it("keeps the invitation while a person matches the query", () => {
    const groups = buildNotesMentionGroups({ query: "ali", dateTargets: [], namedTargets: [user], canInvite: true });

    expect(summary(groups)).toEqual([["people", "user:user-1", "invite"]]);
  });

  it("offers the invitation alone when no people exist and nothing is typed", () => {
    expect(summary(buildNotesMentionGroups({ query: " ", dateTargets: [], namedTargets: [], canInvite: true })))
      .toEqual([["people", "invite"]]);
    expect(buildNotesMentionGroups({ query: "", dateTargets: [], namedTargets: [] })).toEqual([]);
  });

  it("flattens rows in the order the sections render", () => {
    const groups = buildNotesMentionGroups({
      query: "",
      dateTargets: [today],
      namedTargets: [project, user],
      canInvite: true,
    });

    expect(flattenNotesMentionGroups(groups).map(notesMentionRowKey)).toEqual([
      "date:date:2026-10-11",
      "user:user-1",
      "invite",
      "project:project-1",
    ]);
  });
});
