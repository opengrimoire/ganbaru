export const basePage = {
  object: "page",
  id: "11111111-1111-4111-8111-111111111111",
  created_time: "2026-06-30T12:00:00.000Z",
  last_edited_time: "2026-06-30T12:00:00.000Z",
  parent: { type: "workspace", workspace: true },
  folder_id: null,
  in_trash: false,
  archived: false,
  icon: null,
  cover: null,
  properties: {},
  url: null,
  public_url: null,
  source_provider: null,
  source_object_id: null,
  source_workspace_id: null,
  source_last_edited_time: null,
};

export const baseBlock = {
  object: "block",
  id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  parent: { type: "page_id", page_id: basePage.id },
  created_time: "2026-06-30T12:00:00.000Z",
  last_edited_time: "2026-06-30T12:00:00.000Z",
  has_children: false,
  in_trash: false,
  archived: false,
  source_provider: null,
  source_object_id: null,
  source_last_edited_time: null,
};

export const baseRichText = {
  type: "text",
  text: {
    content: "Heading",
    link: null,
  },
  annotations: {
    bold: false,
    italic: false,
    strikethrough: false,
    underline: false,
    code: false,
    color: "default",
  },
  plain_text: "Heading",
  href: null,
};
