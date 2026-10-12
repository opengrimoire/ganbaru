import { describe, expect, it } from "vitest";
import { parseNotesBlock, parseNotesComment, parseNotesMentionNotification, parseNotesPageNotificationSettings, parseNotesPageHistorySettings, parseNotesPageHistorySnapshot, parseNotesPageTemplate } from "./validation";
import { baseBlock, basePage, baseRichText } from "./validation.fixtures";

describe("notes collaboration-history boundary validation", () => {
  it("parses mention notification DTOs", () => {
      expect(parseNotesMentionNotification({
        object: "mention_notification",
        id: "notification-a",
        source_type: "comment",
        source_id: "10101010-1010-4010-8010-101010101010",
        page_id: basePage.id,
        page_title: "Inbox",
        block_id: baseBlock.id,
        comment_id: "10101010-1010-4010-8010-101010101010",
        kind: "user_mention",
        target_type: "user",
        target_id: "12121212-1212-4212-8212-121212121212",
        trigger_at: null,
        remind_minutes_before: 0,
        plain_text: "Alice",
        source_plain_text: "Hi Alice",
        status: "pending",
        delivered_at: null,
        suppressed: false,
        created_time: "2026-07-01T12:00:00.000Z",
        last_edited_time: "2026-07-01T12:00:00.000Z",
      })).toMatchObject({
        source_type: "comment",
        kind: "user_mention",
        target_type: "user",
        status: "pending",
      });
    });

  it("parses database date reminders and rejects unknown sources", () => {
    const reminder = {
      object: "mention_notification",
      id: "notification-b",
      source_type: "page_property",
      source_id: basePage.id,
      page_id: basePage.id,
      page_title: "Meeting",
      block_id: null,
      comment_id: null,
      kind: "reminder",
      target_type: "date",
      target_id: "when",
      trigger_at: "2026-07-02T09:00:00",
      remind_minutes_before: 30,
      plain_text: "When",
      source_plain_text: "Meeting",
      status: "pending",
      delivered_at: null,
      suppressed: false,
      created_time: "2026-07-01T12:00:00.000Z",
      last_edited_time: "2026-07-01T12:00:00.000Z",
    };

    expect(parseNotesMentionNotification(reminder)).toMatchObject({
      source_type: "page_property",
      block_id: null,
      target_id: "when",
      remind_minutes_before: 30,
    });
    expect(() => parseNotesMentionNotification({ ...reminder, source_type: "page" }))
      .toThrow("mention notification.source_type is unsupported");
  });

  const commentDto = (reactions: unknown) => ({
    object: "comment",
    id: "10101010-1010-4010-8010-101010101010",
    parent: { type: "page_id", page_id: basePage.id },
    discussion_id: "13131313-1313-4313-8313-131313131313",
    created_time: "2026-07-01T12:00:00.000Z",
    last_edited_time: "2026-07-01T12:00:00.000Z",
    created_by: { object: "user", id: "12121212-1212-4212-8212-121212121212" },
    rich_text: [baseRichText],
    attachments: [],
    display_name: { type: "user", resolved_name: "Alice" },
    deleted_at: null,
    reactions,
  });

  it("parses comment reactions with their reactors", () => {
    const comment = parseNotesComment(commentDto([
      { emoji: "👍", count: 2, reacted_by_me: true, names: ["Alice", "Bob"] },
    ]));

    expect(comment.reactions).toEqual([
      { emoji: "👍", count: 2, reacted_by_me: true, names: ["Alice", "Bob"] },
    ]);
  });

  it("rejects reactions whose count disagrees with their reactors", () => {
    expect(() => parseNotesComment(commentDto([
      { emoji: "👍", count: 3, reacted_by_me: false, names: ["Alice"] },
    ]))).toThrow("comment.reactions[0].count must match its names");
    expect(() => parseNotesComment(commentDto([
      { emoji: "👍", count: 0, reacted_by_me: false, names: [] },
    ]))).toThrow("comment.reactions[0].count must match its names");
    expect(() => parseNotesComment(commentDto(undefined))).toThrow("comment.reactions must be an array");
  });

  it("parses page notification settings and rejects unknown levels", () => {
    expect(parseNotesPageNotificationSettings({
      object: "page_notification_settings",
      page_id: basePage.id,
      level: "mentions",
    })).toEqual({ object: "page_notification_settings", page_id: basePage.id, level: "mentions" });
    expect(() => parseNotesPageNotificationSettings({
      object: "page_notification_settings",
      page_id: basePage.id,
      level: "loud",
    })).toThrow("page notification settings.level is unsupported");
  });

  it("parses page template DTOs", () => {
      const template = parseNotesPageTemplate({
        object: "page_template",
        id: "99999999-9999-4999-8999-999999999999",
        name: "Weekly review",
        source_page_id: basePage.id,
        properties: {
          title: {
            id: "title",
            type: "title",
            title: [baseRichText],
          },
        },
        icon: { type: "emoji", emoji: "📄" },
        cover: null,
        block_count: 3,
        created_time: "2026-07-01T12:00:00.000Z",
        last_edited_time: "2026-07-01T12:00:00.000Z",
      });

      expect(template.name).toBe("Weekly review");
      expect(template.source_page_id).toBe(basePage.id);
      expect(template.block_count).toBe(3);
    });

  it("rejects negative page template block counts", () => {
      expect(() =>
        parseNotesPageTemplate({
          object: "page_template",
          id: "99999999-9999-4999-8999-999999999999",
          name: "Weekly review",
          source_page_id: null,
          properties: {},
          icon: null,
          cover: null,
          block_count: -1,
          created_time: "2026-07-01T12:00:00.000Z",
          last_edited_time: "2026-07-01T12:00:00.000Z",
        }),
      ).toThrow("page template.block_count must not be negative");
    });

  it("parses page history snapshot DTOs", () => {
      const snapshot = parseNotesPageHistorySnapshot({
        object: "page_history_snapshot",
        id: "88888888-8888-4888-8888-888888888888",
        page_id: basePage.id,
        title: "Draft",
        icon: { type: "emoji", emoji: "🕘" },
        cover: null,
        block_count: 4,
        reason: "update_block",
        created_by: { object: "user", id: "70707070-7070-4070-8070-707070707070" },
        created_time: "2026-07-01T12:00:00.000Z",
        page_last_edited_time: "2026-07-01T11:59:00.000Z",
      });

      expect(snapshot.page_id).toBe(basePage.id);
      expect(snapshot.block_count).toBe(4);
      expect(snapshot.reason).toBe("update_block");
      expect(snapshot.created_by.id).toBe("70707070-7070-4070-8070-707070707070");
    });

  it("rejects invalid page history snapshot block counts", () => {
      expect(() =>
        parseNotesPageHistorySnapshot({
          object: "page_history_snapshot",
          id: "88888888-8888-4888-8888-888888888888",
          page_id: basePage.id,
          title: "Draft",
          icon: null,
          cover: null,
          block_count: -1,
          reason: "update_block",
          created_by: { object: "user", id: "70707070-7070-4070-8070-707070707070" },
          created_time: "2026-07-01T12:00:00.000Z",
          page_last_edited_time: "2026-07-01T11:59:00.000Z",
        }),
      ).toThrow("page history snapshot.block_count must not be negative");
    });

  it("parses page history settings DTOs", () => {
      expect(
        parseNotesPageHistorySettings({
          object: "page_history_settings",
          retention_days: 90,
          updated_at: "2026-07-01T12:00:00.000Z",
        }),
      ).toMatchObject({ retention_days: 90 });

      expect(
        parseNotesPageHistorySettings({
          object: "page_history_settings",
          retention_days: 180,
          updated_at: "2026-07-01T12:00:00.000Z",
        }),
      ).toMatchObject({ retention_days: 180 });
    });

  it("accepts disabled history and rejects invalid retention windows", () => {
      expect(
        parseNotesPageHistorySettings({
          object: "page_history_settings",
          retention_days: 0,
          updated_at: "2026-07-01T12:00:00.000Z",
        }),
      ).toMatchObject({ retention_days: 0 });

      expect(() =>
        parseNotesPageHistorySettings({
          object: "page_history_settings",
          retention_days: 14,
          updated_at: "2026-07-01T12:00:00.000Z",
        }),
      ).toThrow("page history settings.retention_days is unsupported");
    });

  it("parses template block payloads", () => {
      const block = parseNotesBlock({
        ...baseBlock,
        type: "template",
        template: {
          rich_text: [baseRichText],
        },
      });

      expect(block.type).toBe("template");
      if (block.type === "template") {
        expect(block.template.rich_text[0]?.plain_text).toBe("Heading");
      }
    });

  it("rejects invalid template rich text payloads", () => {
      expect(() =>
        parseNotesBlock({
          ...baseBlock,
          type: "template",
          template: {
            rich_text: "Add task",
          },
        }),
      ).toThrow("block.template.rich_text must be an array");

      expect(() =>
        parseNotesBlock({
          ...baseBlock,
          type: "template",
          template: {
            rich_text: [baseRichText],
            color: "default",
          },
        }),
      ).toThrow("block.template.color is not supported");

      expect(() =>
        parseNotesBlock({
          ...baseBlock,
          type: "template",
          template: {
            rich_text: [baseRichText],
            children: [],
          },
        }),
      ).toThrow("block.template.children must be stored as child blocks");
    });
});
