import type { NotesDateMentionReminder, NotesDateMentionValue, NotesRichText, NotesRichTextAnnotations } from "../contracts/core";
import { normalizeRichTextEquationExpression, normalizeRichTextLinkUrl } from "$lib/notes/rich-text/core";
import { readBoolean, readDateMentionBoundary, readNullableDateMentionFormat, readLocalObjectMentionType, readMentionObjectId, readNotesColor, readNullableDateMentionBoundary, readNullableDisplayString, readNullableString, readRecord, readString, readUuidString } from "./readers";

export function parseNotesRichTextArray(value: unknown, label: string): NotesRichText[] {
  if (!Array.isArray(value)) throw new Error(`${label} must be an array`);
  return value.map((item, index) => parseNotesRichText(item, `${label}[${index}]`));
}

function parseNotesRichText(value: unknown, label: string): NotesRichText {
  const record = readRecord(value, label);
  const type = readString(record.type, `${label}.type`);
  const annotations = parseAnnotations(record.annotations, `${label}.annotations`);
  const plainText = readString(record.plain_text, `${label}.plain_text`);
  const href = readNullableString(record.href, `${label}.href`);
  if (type === "text") {
    const text = readRecord(record.text, `${label}.text`);
    return {
      type: "text",
      text: {
        content: readString(text.content, `${label}.text.content`),
        link: parseRichTextLink(text.link, `${label}.text.link`),
      },
      annotations,
      plain_text: plainText,
      href,
    };
  }
  if (type === "mention") {
    const mention = readRecord(record.mention, `${label}.mention`);
    const mentionType = readString(mention.type, `${label}.mention.type`);
    if (mentionType === "page") {
      const page = readRecord(mention.page, `${label}.mention.page`);
      return {
        type: "mention",
        mention: {
          type: "page",
          page: {
            id: readString(page.id, `${label}.mention.page.id`),
          },
        },
        annotations,
        plain_text: plainText,
        href,
      };
    }
    if (mentionType === "user") {
      const user = readRecord(mention.user, `${label}.mention.user`);
      return {
        type: "mention",
        mention: {
          type: "user",
          user: {
            object: "user",
            id: readUuidString(user.id, `${label}.mention.user.id`),
          },
        },
        annotations,
        plain_text: plainText,
        href,
      };
    }
    if (mentionType === "database") {
      const database = readRecord(mention.database, `${label}.mention.database`);
      return {
        type: "mention",
        mention: {
          type: "database",
          database: {
            id: readUuidString(database.id, `${label}.mention.database.id`),
          },
        },
        annotations,
        plain_text: plainText,
        href,
      };
    }
    if (mentionType === "ganbaru_object") {
      const object = readRecord(mention.ganbaru_object, `${label}.mention.ganbaru_object`);
      const objectType = readLocalObjectMentionType(
        object.type,
        `${label}.mention.ganbaru_object.type`,
      );
      const objectId = readMentionObjectId(object.id, `${label}.mention.ganbaru_object.id`);
      return {
        type: "mention",
        mention: {
          type: "ganbaru_object",
          ganbaru_object: {
            type: objectType,
            id: objectId,
          },
        },
        annotations,
        plain_text: plainText,
        href,
      };
    }
    if (mentionType === "date") {
      return {
        type: "mention",
        mention: {
          type: "date",
          date: parseDateMentionValue(mention.date, `${label}.mention.date`),
        },
        annotations,
        plain_text: plainText,
        href,
      };
    }
    throw new Error(`${label}.mention.type is unsupported`);
  }
  if (type === "equation") {
    const equation = readRecord(record.equation, `${label}.equation`);
    const expression = readString(equation.expression, `${label}.equation.expression`);
    if (!normalizeRichTextEquationExpression(expression)) {
      throw new Error(
        `${label}.equation.expression must not be empty, too long, or contain control characters`,
      );
    }
    return {
      type: "equation",
      equation: { expression },
      annotations,
      plain_text: plainText,
      href,
    };
  }
  throw new Error(`${label}.type must be text, mention, or equation`);
}

function parseDateMentionValue(value: unknown, label: string): NotesDateMentionValue {
  const record = readRecord(value, label);
  return {
    start: readDateMentionBoundary(record.start, `${label}.start`),
    ...(record.end === undefined
      ? {}
      : { end: readNullableDateMentionBoundary(record.end, `${label}.end`) }),
    ...(record.time_zone === undefined
      ? {}
      : { time_zone: readNullableDisplayString(record.time_zone, `${label}.time_zone`) }),
    ...(record.ganbaru_reminder === undefined
      ? {}
      : {
          ganbaru_reminder: parseNullableDateMentionReminder(
            record.ganbaru_reminder,
            `${label}.ganbaru_reminder`,
          ),
        }),
    ...(record.ganbaru_format === undefined
      ? {}
      : { ganbaru_format: readNullableDateMentionFormat(record.ganbaru_format, `${label}.ganbaru_format`) }),
  };
}

function parseNullableDateMentionReminder(
  value: unknown,
  label: string,
): NotesDateMentionReminder | null {
  if (value === null) return null;
  const record = readRecord(value, label);
  return {
    enabled: readBoolean(record.enabled, `${label}.enabled`),
  };
}

export function dateMentionBoundaryLooksIso(value: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}/u.test(value)) return false;
  const month = Number.parseInt(value.slice(5, 7), 10);
  const day = Number.parseInt(value.slice(8, 10), 10);
  if (month < 1 || month > 12 || day < 1 || day > 31) return false;
  if (value.length === 10) return true;
  return value.at(10) === "T" && value.length <= 80;
}

function parseRichTextLink(value: unknown, label: string): { url: string } | null {
  if (value === null) return null;
  const record = readRecord(value, label);
  const url = readString(record.url, `${label}.url`);
  if (!normalizeRichTextLinkUrl(url)) {
    throw new Error(`${label}.url must be a valid HTTP, HTTPS, or email URL`);
  }
  return {
    url,
  };
}

function parseAnnotations(value: unknown, label: string): NotesRichTextAnnotations {
  const record = readRecord(value, label);
  return {
    bold: readBoolean(record.bold, `${label}.bold`),
    italic: readBoolean(record.italic, `${label}.italic`),
    strikethrough: readBoolean(record.strikethrough, `${label}.strikethrough`),
    underline: readBoolean(record.underline, `${label}.underline`),
    code: readBoolean(record.code, `${label}.code`),
    color: readNotesColor(record.color, `${label}.color`),
  };
}
