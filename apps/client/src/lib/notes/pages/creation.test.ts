import { describe, expect, it } from "vitest";
import { createProvisionalNotesPage } from "./creation";

describe("provisional Notes page", () => {
  it("builds an editable canonical page and first paragraph", () => {
    const loaded = createProvisionalNotesPage({
      id: "11111111-1111-4111-8111-111111111111",
      title: "Draft",
      parent: { type: "workspace", workspace: true },
      folder_id: null,
      first_block_id: "22222222-2222-4222-8222-222222222222",
      properties: { __ganbaru_project_id: "project-routine-learning" },
    }, "2026-07-12T00:00:00.000Z");

    expect(loaded.page.id).toBe("11111111-1111-4111-8111-111111111111");
    expect(loaded.page.properties.__ganbaru_project_id).toBe("project-routine-learning");
    expect(loaded.page.properties.title).toMatchObject({ type: "title" });
    expect(loaded.blocks.results).toHaveLength(1);
    expect(loaded.blocks.results[0]).toMatchObject({
      id: "22222222-2222-4222-8222-222222222222",
      type: "paragraph",
      parent: { type: "page_id", page_id: "11111111-1111-4111-8111-111111111111" },
    });
  });
});
