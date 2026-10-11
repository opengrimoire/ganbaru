import { Temporal } from "@js-temporal/polyfill";
import { describe, expect, it } from "vitest";
import {
  buildDateMentionTargets,
  type NotesNamedMentionTarget,
} from "$lib/notes/rich-text/core";
import {
  buildNotesCommentMentionGroups,
  flattenNotesCommentMentionGroups,
} from "./comment-mentions";

const labels = {
  today: "Today",
  tomorrow: "Tomorrow",
  yesterday: "Yesterday",
  nextWeek: "Next week",
  date: "Date",
  reminder: "Reminder",
  remindTitle: (date: string) => `Remind ${date}`,
};

function dates(query: string) {
  return buildDateMentionTargets(query, {
    today: Temporal.PlainDate.from("2026-10-11"),
    locale: "en-US",
    labels,
  });
}

const named: NotesNamedMentionTarget[] = [
  { kind: "user", id: "local", title: "Ada", subtitle: "You" },
  { kind: "page", id: "page-roadmap", title: "Roadmap", subtitle: "Workspace" },
  { kind: "page", id: "page-retro", title: "Retro notes", subtitle: "Workspace" },
  { kind: "database", id: "db-tasks", title: "Tasks", subtitle: "Database" },
  { kind: "project", id: "project-a", title: "Road trip", subtitle: "Project" },
];

describe("comment mention groups", () => {
  it("offers a short date list, people, and pages in discussion order before typing", () => {
    const groups = buildNotesCommentMentionGroups({ query: "", dateTargets: dates(""), namedTargets: named });

    expect(groups.map((group) => group.kind)).toEqual(["date", "people", "pages"]);
    expect(groups[0].targets.map((target) => target.title)).toEqual(["Today", "Tomorrow"]);
    expect(groups[1].targets.map((target) => target.id)).toEqual(["local"]);
    expect(groups[2].targets.map((target) => target.id)).toEqual(["db-tasks", "page-retro", "page-roadmap"]);
  });

  it("keeps other local objects out of the comment menu", () => {
    const ids = flattenNotesCommentMentionGroups(
      buildNotesCommentMentionGroups({ query: "road", dateTargets: [], namedTargets: named }),
    ).map((target) => target.id);

    expect(ids).toEqual(["page-roadmap"]);
  });

  it("drops groups the query empties and keeps date matches", () => {
    const groups = buildNotesCommentMentionGroups({
      query: "tomorrow",
      dateTargets: dates("tomorrow"),
      namedTargets: named,
    });

    expect(groups.map((group) => group.kind)).toEqual(["date"]);
    expect(groups[0].targets[0]).toMatchObject({ kind: "date", title: "Tomorrow", reminder: false });
  });

  it("matches people by subtitle and respects group limits", () => {
    const pages: NotesNamedMentionTarget[] = Array.from({ length: 9 }, (_, index) => ({
      kind: "page",
      id: `page-${index}`,
      title: `You page ${index}`,
    }));
    const groups = buildNotesCommentMentionGroups({
      query: "you",
      dateTargets: [],
      namedTargets: [...named, ...pages],
      limits: { idleDates: 1, queryDates: 1, people: 1, pages: 3 },
    });

    expect(groups.map((group) => [group.kind, group.targets.length])).toEqual([["people", 1], ["pages", 3]]);
  });

  it("flattens groups in render order for keyboard selection", () => {
    const groups = buildNotesCommentMentionGroups({ query: "", dateTargets: dates(""), namedTargets: named });

    expect(flattenNotesCommentMentionGroups(groups).map((target) => target.kind)).toEqual([
      "date",
      "date",
      "user",
      "database",
      "page",
      "page",
    ]);
  });
});
