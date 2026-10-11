import { describe, expect, it } from "vitest";
import { Temporal } from "@js-temporal/polyfill";
import {
  applyRichTextAnnotations,
  applyRichTextLink,
  buildDateMentionTargets,
  buildIdleDateMentionTargets,
  createEquationRichText,
  createLinkedTextRichText,
  createDateMentionValue,
  createDateMentionRichText,
  createPageMentionRichText,
  createTextRichText,
  detectPageMentionQuery,
  filterNotesMentionTargets,
  notesDateMentionDetail,
  filterPageMentionTargets,
  insertDateMentionRichText,
  insertEquationRichText,
  insertObjectMentionRichText,
  insertPageMentionRichText,
  normalizeRichTextEquationExpression,
  normalizeRichTextLinkUrl,
  notesPastedLinkUrl,
  editRichTextLink,
  planRichTextEquationConversion,
  replacePlainTextPreservingRichText,
  richTextAnnotationTogglePatch,
  richTextAnnotationsForSelection,
  richTextColorPatch,
  richTextHasVisibleFormatting,
  richTextLinkRangeForSelection,
  richTextPlainText,
} from "./core";

const pageId = "11111111-1111-4111-8111-111111111111";
const dateLabels = {
  today: "Today",
  tomorrow: "Tomorrow",
  yesterday: "Yesterday",
  nextWeek: "Next week",
  date: "Date",
  reminder: "Reminder",
  remindTitle: (label: string) => `Remind ${label}`,
  atTime: (label: string, time: string) => `${label}, ${time}`,
};

describe("notes rich text helpers", () => {
  it("inserts page mentions into a plain text range", () => {
    const richText = insertPageMentionRichText(
      [createTextRichText("See @tar today")],
      4,
      8,
      pageId,
      "Target page",
      "http://localhost:1420/?view=notes#notes?page=11111111-1111-4111-8111-111111111111",
    );

    expect(richTextPlainText(richText)).toBe("See Target page today");
    expect(richText[1]).toMatchObject({
      type: "mention",
      mention: { type: "page", page: { id: pageId } },
      plain_text: "Target page",
    });
  });

  it("inserts date mentions into a plain text range", () => {
    const richText = insertDateMentionRichText(
      [createTextRichText("Due @today")],
      4,
      10,
      createDateMentionValue("2026-06-30"),
      "Today",
    );

    expect(richTextPlainText(richText)).toBe("Due Today");
    expect(richText[1]).toMatchObject({
      type: "mention",
      mention: {
        type: "date",
        date: {
          start: "2026-06-30",
          end: null,
          time_zone: null,
        },
      },
      plain_text: "Today",
    });
  });

  it("inserts local object mentions into a plain text range", () => {
    const richText = insertObjectMentionRichText(
      [createTextRichText("Work on @task now")],
      8,
      13,
      {
        kind: "project_task",
        id: "22222222-2222-4222-8222-222222222222",
        title: "Ship editor",
        subtitle: "Project task",
      },
    );

    expect(richTextPlainText(richText)).toBe("Work on Ship editor now");
    expect(richText[1]).toMatchObject({
      type: "mention",
      mention: {
        type: "ganbaru_object",
        ganbaru_object: {
          type: "project_task",
          id: "22222222-2222-4222-8222-222222222222",
        },
      },
      plain_text: "Ship editor",
    });
  });

  it("applies and removes links over text ranges", () => {
    const linked = applyRichTextLink(
      [createTextRichText("Read the docs")],
      5,
      13,
      "example.com/docs",
    );

    expect(richTextPlainText(linked)).toBe("Read the docs");
    expect(linked[1]).toMatchObject({
      type: "text",
      text: { content: "the docs", link: { url: "https://example.com/docs" } },
      href: "https://example.com/docs",
    });

    const unlinked = applyRichTextLink(linked, 5, 13, null);
    expect(unlinked).toEqual([createTextRichText("Read the docs")]);
  });

  it("edits existing links without changing selected text", () => {
    const linked = applyRichTextLink(
      [createTextRichText("Read the docs")],
      5,
      13,
      "https://example.com/docs",
    );
    const edited = applyRichTextLink(linked, 5, 13, "https://docs.example.org");

    expect(richTextPlainText(edited)).toBe("Read the docs");
    expect(edited[1]).toMatchObject({
      type: "text",
      text: { content: "the docs", link: { url: "https://docs.example.org/" } },
      href: "https://docs.example.org/",
    });
  });

  it("rejects unsafe links before changing rich text", () => {
    const source = [createTextRichText("Read the docs")];

    expect(applyRichTextLink(source, 5, 13, "javascript:alert(1)")).toEqual(source);
  });

  it("applies annotations over text ranges and merges compatible spans", () => {
    const richText = applyRichTextAnnotations(
      [createTextRichText("Make this bold")],
      5,
      14,
      { bold: true, color: "blue" },
    );

    expect(richTextPlainText(richText)).toBe("Make this bold");
    expect(richText[1]).toMatchObject({
      type: "text",
      text: { content: "this bold" },
      annotations: { bold: true, color: "blue" },
    });

    const unbold = applyRichTextAnnotations(richText, 5, 14, { bold: false, color: "default" });
    expect(unbold).toEqual([createTextRichText("Make this bold")]);
  });

  it("inserts inline equations into text ranges", () => {
    const richText = insertEquationRichText(
      [createTextRichText("Use e=mc^2 here")],
      4,
      10,
      " e=mc^2 ",
    );

    expect(richTextPlainText(richText)).toBe("Use e=mc^2 here");
    expect(richText[1]).toEqual(createEquationRichText("e=mc^2"));
  });

  it("plans inline equation conversion with normalized expression and cursor", () => {
    expect(planRichTextEquationConversion("Use  e=mc^2  here", 4, 12)).toEqual({
      type: "convert",
      start: 4,
      end: 12,
      expression: "e=mc^2",
      cursor: 10,
    });
  });

  it("plans inline equation feedback for empty or invalid selections", () => {
    expect(planRichTextEquationConversion("Use e=mc^2 here", 4, 4)).toEqual({
      type: "error",
      reason: "selection_required",
      start: 4,
      end: 4,
    });
    expect(planRichTextEquationConversion("Use bad\u0008 here", 4, 8)).toEqual({
      type: "error",
      reason: "invalid_expression",
      start: 4,
      end: 8,
    });
  });

  it("normalizes inline equation expressions", () => {
    expect(normalizeRichTextEquationExpression("  \\frac{a}{b}  ")).toBe("\\frac{a}{b}");
    expect(normalizeRichTextEquationExpression("")).toBeNull();
    expect(normalizeRichTextEquationExpression("bad\u0008")).toBeNull();
  });

  it("keeps mentions intact when applying annotations around them", () => {
    const mention = createPageMentionRichText(pageId, "Target page", null);
    const richText = applyRichTextAnnotations(
      [createTextRichText("See "), mention, createTextRichText(" soon")],
      0,
      20,
      { italic: true },
    );

    expect(richTextPlainText(richText)).toBe("See Target page soon");
    expect(richText[1]).toEqual(mention);
    expect(richText[0]).toMatchObject({ annotations: { italic: true } });
    expect(richText[2]).toMatchObject({ annotations: { italic: true } });
  });

  it("reads annotation state from selections and collapsed cursors", () => {
    const linked = applyRichTextAnnotations(
      [createTextRichText("Read docs today")],
      5,
      9,
      { code: true, color: "red_background" },
    );

    expect(richTextAnnotationsForSelection(linked, 6, 6).annotations).toMatchObject({
      code: true,
      color: "red_background",
    });
    expect(richTextAnnotationsForSelection(linked, 5, 9).annotations).toMatchObject({
      code: true,
      color: "red_background",
    });
    expect(richTextAnnotationsForSelection(linked, 0, 9).annotations).toMatchObject({
      code: false,
      color: "default",
    });
  });

  it("builds annotation patches for toolbar actions", () => {
    const annotations = richTextAnnotationsForSelection(
      applyRichTextAnnotations([createTextRichText("Bold")], 0, 4, { bold: true }),
      0,
      4,
    ).annotations;

    expect(richTextAnnotationTogglePatch(annotations, "bold")).toEqual({ bold: false });
    expect(richTextColorPatch("green")).toEqual({ color: "green" });
  });

  it("detects visible inline formatting", () => {
    expect(richTextHasVisibleFormatting([createTextRichText("Plain")])).toBe(false);
    expect(
      richTextHasVisibleFormatting(
        applyRichTextAnnotations([createTextRichText("Styled")], 0, 6, { underline: true }),
      ),
    ).toBe(true);
    expect(richTextHasVisibleFormatting([createPageMentionRichText(pageId, "Page", null)])).toBe(true);
    expect(richTextHasVisibleFormatting([createEquationRichText("e=mc^2")])).toBe(true);
  });

  it("detects every rich text shape rendered by the focused editor", () => {
    expect(
      richTextHasVisibleFormatting(
        applyRichTextAnnotations([createTextRichText("Bold")], 0, 4, { bold: true }),
      ),
    ).toBe(true);
    expect(
      richTextHasVisibleFormatting(
        applyRichTextAnnotations([createTextRichText("Italic")], 0, 6, { italic: true }),
      ),
    ).toBe(true);
    expect(
      richTextHasVisibleFormatting(
        applyRichTextAnnotations([createTextRichText("Strike")], 0, 6, {
          strikethrough: true,
        }),
      ),
    ).toBe(true);
    expect(
      richTextHasVisibleFormatting(
        applyRichTextAnnotations([createTextRichText("Code")], 0, 4, { code: true }),
      ),
    ).toBe(true);
    expect(
      richTextHasVisibleFormatting(
        applyRichTextAnnotations([createTextRichText("Blue")], 0, 4, { color: "blue" }),
      ),
    ).toBe(true);
    expect(
      richTextHasVisibleFormatting(
        applyRichTextAnnotations([createTextRichText("Highlight")], 0, 9, {
          color: "yellow_background",
        }),
      ),
    ).toBe(true);
    expect(richTextHasVisibleFormatting([createLinkedTextRichText("Link", "https://example.com")]))
      .toBe(true);
    expect(richTextHasVisibleFormatting([createPageMentionRichText(pageId, "Page", null)]))
      .toBe(true);
    expect(
      richTextHasVisibleFormatting([
        createDateMentionRichText(createDateMentionValue("2026-06-30"), "Today"),
      ]),
    ).toBe(true);
    expect(richTextHasVisibleFormatting([createEquationRichText("e=mc^2")])).toBe(true);
  });

  it("finds an existing link range from a collapsed cursor", () => {
    const linked = [
      createTextRichText("Read "),
      createLinkedTextRichText("the docs", "https://example.com/docs"),
      createTextRichText(" today"),
    ];

    expect(richTextLinkRangeForSelection(linked, 8, 8)).toEqual({
      start: 5,
      end: 13,
      url: "https://example.com/docs",
    });
  });

  it("finds an existing link range from a selected link", () => {
    const linked = [
      createTextRichText("Read "),
      createLinkedTextRichText("the docs", "https://example.com/docs"),
      createTextRichText(" today"),
    ];

    expect(richTextLinkRangeForSelection(linked, 5, 13)).toEqual({
      start: 5,
      end: 13,
      url: "https://example.com/docs",
    });
  });

  it("normalizes supported links and rejects unsafe schemes", () => {
    expect(normalizeRichTextLinkUrl("http://example.com")).toBe("http://example.com/");
    expect(normalizeRichTextLinkUrl("example.com")).toBe("https://example.com/");
    expect(normalizeRichTextLinkUrl("team@example.com")).toBe("mailto:team@example.com");
    expect(normalizeRichTextLinkUrl("mailto:team@example.com")).toBe("mailto:team@example.com");
    expect(normalizeRichTextLinkUrl("team@example")).toBeNull();
    expect(normalizeRichTextLinkUrl("mailto:team%40example.com")).toBeNull();
    expect(normalizeRichTextLinkUrl("javascript:alert(1)")).toBeNull();
  });

  it("preserves valid local block links while rejecting invalid fragments", () => {
    const url = `#notes?page=${pageId}&block=${pageId}`;
    expect(normalizeRichTextLinkUrl(url)).toBe(url);
    expect(normalizeRichTextLinkUrl("#notes?page=invalid")).toBeNull();
    expect(normalizeRichTextLinkUrl("#unrelated")).toBeNull();
    expect(applyRichTextLink([createTextRichText("Tasks")], 0, 5, url)[0].href).toBe(url);
  });

  it.each([
    ["https://example.com/tasks", "https://example.com/tasks"],
    ["example.com", "https://example.com/"],
    ["team@example.com", "mailto:team@example.com"],
    [`#notes?page=${pageId}`, `#notes?page=${pageId}`],
    ["ordinary words", null], ["Text", null], ["javascript:alert(1)", null],
    ["file:///tmp/file", null], ["https://example.com\nMore text", null], ["#notes?page=bad", null],
  ])("recognizes a copied destination %j as %j", (input, expected) => {
    expect(notesPastedLinkUrl(input)).toBe(expected);
  });

  it("edits a linked label without losing its bold formatting or surrounding Unicode text", () => {
    const text = createLinkedTextRichText("Old title", "https://example.com/old");
    text.annotations.bold = true;
    const richText = [createTextRichText("Before "), text, createTextRichText(" after")];
    const result = editRichTextLink(richText, 7, 16, "New 𝄞 title", "https://example.com/new");
    expect(richTextPlainText(result)).toBe("Before New 𝄞 title after");
    expect(result.filter((item) => item.href).every((item) => item.annotations.bold && item.href === "https://example.com/new")).toBe(true);
    expect(result[0].href).toBeNull();
    expect(result.at(-1)?.href).toBeNull();
  });

  it("builds reminder mention targets from reminder queries", () => {
    const targets = buildDateMentionTargets("remind tomorrow", {
      today: Temporal.PlainDate.from("2026-06-30"),
      locale: "en",
      labels: {
        today: "Today",
        tomorrow: "Tomorrow",
        yesterday: "Yesterday",
        nextWeek: "Next week",
        date: "Date",
        reminder: "Reminder",
        remindTitle: (label) => `Remind ${label}`,
        atTime: (label, time) => `${label}, ${time}`,
      },
    });

    expect(targets[0]).toMatchObject({
      kind: "date",
      title: "Remind Tomorrow",
      subtitle: "Reminder",
      date: {
        start: "2026-07-01",
        ganbaru_reminder: { enabled: true },
      },
      reminder: true,
    });
  });

  it("leads with a typed calendar date and stores a typed time as a floating wall time", () => {
    const today = Temporal.PlainDate.from("2026-10-11");
    const [typed] = buildDateMentionTargets("oct 30 12:1", { today, locale: "en-US", labels: dateLabels, timeFormat: "12h" });
    expect(typed).toMatchObject({
      id: "date:2026-10-30T12:01",
      title: "Oct 30, 2026, 12:01 PM",
      date: { start: "2026-10-30T12:01", end: null, time_zone: null },
    });
    if (!typed) throw new Error("expected a typed date suggestion");
    expect(notesDateMentionDetail(typed, "en-US")).toBeNull();

    const [past] = buildDateMentionTargets("2025-12-2", { today, locale: "en-US", labels: dateLabels });
    expect(past).toMatchObject({ title: "Dec 2, 2025", date: { start: "2025-12-02" } });
  });

  it("applies a time typed alone to every suggested day and keeps a reminder word", () => {
    const today = Temporal.PlainDate.from("2026-10-11");
    const timed = buildDateMentionTargets("3pm", { today, locale: "en-US", labels: dateLabels, timeFormat: "24h" });
    expect(timed.map((target) => [target.title, target.date.start])).toEqual([
      ["Today, 15:00", "2026-10-11T15:00"],
      ["Tomorrow, 15:00", "2026-10-12T15:00"],
      ["Yesterday, 15:00", "2026-10-10T15:00"],
      ["Next week, 15:00", "2026-10-18T15:00"],
    ]);

    const [reminder] = buildDateMentionTargets("remind me tomorrow 9am", { today, locale: "en-US", labels: dateLabels, timeFormat: "12h" });
    expect(reminder).toMatchObject({
      title: "Remind Tomorrow, 9:00 AM",
      reminder: true,
      date: { start: "2026-10-12T09:00", ganbaru_reminder: { enabled: true } },
    });
  });

  it("preserves page mentions when surrounding text changes", () => {
    const mention = createPageMentionRichText(pageId, "Target page", null);
    const richText = replacePlainTextPreservingRichText(
      [createTextRichText("See "), mention],
      "Please see Target page soon",
    );

    expect(richTextPlainText(richText)).toBe("Please see Target page soon");
    expect(richText.some((item) => item.type === "mention")).toBe(true);
  });

  it("preserves links and formatted spans when surrounding text changes", () => {
    const [boldText] = applyRichTextAnnotations(
      [createTextRichText("important")],
      0,
      "important".length,
      { bold: true, color: "yellow_background" },
    );
    if (!boldText) throw new Error("expected annotated rich text");
    const richText = replacePlainTextPreservingRichText(
      [
        createTextRichText("Read "),
        createLinkedTextRichText("docs", "https://example.com/docs"),
        createTextRichText(" before "),
        boldText,
      ],
      "Please read docs before important today",
    );

    expect(richTextPlainText(richText)).toBe("Please read docs before important today");
    expect(
      richText.some((item) =>
        item.type === "text"
        && item.plain_text === "docs"
        && item.text.link?.url === "https://example.com/docs"
        && item.href === "https://example.com/docs"
      ),
    ).toBe(true);
    expect(
      richText.some((item) =>
        item.type === "text"
        && item.plain_text === "important"
        && item.annotations.bold
        && item.annotations.color === "yellow_background"
      ),
    ).toBe(true);
  });

  it("preserves page mentions, date mentions, and equations around typed text", () => {
    const dateMention = createDateMentionRichText(
      createDateMentionValue("2026-06-30", true),
      "Today",
    );
    const equation = createEquationRichText("x=1");
    const richText = replacePlainTextPreservingRichText(
      [
        createTextRichText("Plan "),
        createPageMentionRichText(pageId, "Project", null),
        createTextRichText(" for "),
        dateMention,
        createTextRichText(" with "),
        equation,
      ],
      "Plan Project for Today with x=1 soon",
    );

    expect(richTextPlainText(richText)).toBe("Plan Project for Today with x=1 soon");
    expect(richText.some((item) => item.type === "mention" && item.mention.type === "page"))
      .toBe(true);
    expect(richText.some((item) => item.type === "mention" && item.mention.type === "date"))
      .toBe(true);
    expect(richText.some((item) => item.type === "equation" && item.equation.expression === "x=1"))
      .toBe(true);
  });

  it("degrades only the edited rich text object to plain text", () => {
    const richText = replacePlainTextPreservingRichText(
      [
        createTextRichText("See "),
        createPageMentionRichText(pageId, "Project", null),
        createTextRichText(" in "),
        createLinkedTextRichText("docs", "https://example.com/docs"),
        createTextRichText(" with "),
        createEquationRichText("x=1"),
      ],
      "See Project in documentation with x=1",
    );

    expect(richTextPlainText(richText)).toBe("See Project in documentation with x=1");
    expect(richText.some((item) => item.type === "mention" && item.mention.type === "page"))
      .toBe(true);
    expect(richText.some((item) => item.type === "equation" && item.equation.expression === "x=1"))
      .toBe(true);
    expect(
      richText.some((item) =>
        item.type === "text"
        && item.text.link?.url === "https://example.com/docs"
      ),
    ).toBe(false);
  });

  it("collapses mentions back to text when the visible label is removed", () => {
    const richText = replacePlainTextPreservingRichText(
      [createTextRichText("See "), createPageMentionRichText(pageId, "Target page", null)],
      "See target",
    );

    expect(richText).toHaveLength(1);
    expect(richText[0]?.type).toBe("text");
  });

  it("degrades formatted spans when the visible label is partially deleted", () => {
    const [boldText] = applyRichTextAnnotations(
      [createTextRichText("important")],
      0,
      "important".length,
      { bold: true },
    );
    if (!boldText) throw new Error("expected annotated rich text");
    const richText = replacePlainTextPreservingRichText(
      [
        createTextRichText("Keep "),
        boldText,
        createTextRichText(" note"),
      ],
      "Keep import note",
    );

    expect(richTextPlainText(richText)).toBe("Keep import note");
    expect(richText.some((item) => item.type === "text" && item.annotations.bold)).toBe(false);
  });

  it("preserves later objects when an earlier object label is removed", () => {
    const richText = replacePlainTextPreservingRichText(
      [
        createTextRichText("See "),
        createPageMentionRichText(pageId, "Project", null),
        createTextRichText(" and "),
        createEquationRichText("x=1"),
      ],
      "See and x=1",
    );

    expect(richTextPlainText(richText)).toBe("See and x=1");
    expect(richText.some((item) => item.type === "mention")).toBe(false);
    expect(richText.some((item) => item.type === "equation" && item.equation.expression === "x=1"))
      .toBe(true);
  });

  it("detects page mention queries at the cursor", () => {
    expect(detectPageMentionQuery("See @tar", 8, 8)).toEqual({
      start: 4,
      end: 8,
      query: "tar",
    });
    expect(detectPageMentionQuery("email@example.com", 17, 17)).toBeNull();
  });

  it("filters mention targets with prefix matches first", () => {
    expect(
      filterPageMentionTargets(
        [
          { id: "b", kind: "page", title: "Roadmap" },
          { id: "a", kind: "page", title: "Daily roadmap" },
          { id: "c", kind: "page", title: "Archive" },
        ],
        "road",
      ).map((target) => target.id),
    ).toEqual(["b", "a"]);
  });

  it("filters expanded mention targets by title and subtitle", () => {
    expect(
      filterNotesMentionTargets(
        [
          { id: "user", kind: "user", title: "Alice", subtitle: "Local user" },
          { id: "project", kind: "project", title: "Roadmap", subtitle: "Project" },
          { id: "task", kind: "project_task", title: "Bug", subtitle: "Roadmap" },
        ],
        "road",
      ).map((target) => target.id),
    ).toEqual(["project", "task"]);
  });

  it("matches pages and databases by the titles on their route", () => {
    expect(
      filterNotesMentionTargets(
        [
          { id: "nested", kind: "page", title: "Sleep", location: ["Personal", "Health"] },
          { id: "tasks", kind: "database", title: "Tasks", location: ["Work"] },
          { id: "top", kind: "page", title: "Inbox" },
        ],
        "health",
      ).map((target) => target.id),
    ).toEqual(["nested"]);
  });

  it("suggests today and a reminder for tomorrow before anything is typed", () => {
    const targets = buildIdleDateMentionTargets({ today: Temporal.PlainDate.from("2026-12-31"), labels: dateLabels });

    expect(targets.map(({ title, reminder, date }) => ({ title, reminder, start: date.start }))).toEqual([
      { title: "Today", reminder: false, start: "2026-12-31" },
      { title: "Remind Tomorrow", reminder: true, start: "2027-01-01" },
    ]);
    expect(targets[1]?.date.ganbaru_reminder).toMatchObject({ enabled: true });
  });

  it("spells out named dates beside their suggestion and skips titles that already read as the date", () => {
    const [todayTarget] = buildIdleDateMentionTargets({ today: Temporal.PlainDate.from("2026-10-11"), labels: dateLabels });
    if (!todayTarget) throw new Error("expected a today suggestion");

    expect(notesDateMentionDetail(todayTarget, "en")).toBe("Oct 11, 2026");
    expect(notesDateMentionDetail({ ...todayTarget, title: "Oct 11, 2026" }, "en")).toBeNull();
    expect(notesDateMentionDetail({ ...todayTarget, date: { start: "not a date" } }, "en")).toBeNull();
  });
});
