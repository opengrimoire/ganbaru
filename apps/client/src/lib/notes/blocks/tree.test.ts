import { describe, expect, it } from "vitest";
import { createBlockWrite, createSyncedBlockPayload, createTextPayload } from "./factory";
import {
  buildNotesChildIdsByParent,
  flattenNotesBlockChildren,
  flattenNotesBlockTree,
  notesIndentationContextIds,
  planDeleteBlock,
  planDropBlockWithinSiblings,
  planMergeWithPrevious,
  planMoveBlockWithinSiblings,
  planNestBlock,
  planOutdentBlock,
  planReparentChildrenAfterMerge,
  planReparentChildrenBeforeDelete,
  type NotesTreeState,
} from "./tree";
import { richTextPlainText } from "$lib/notes/rich-text/core";
import { notesBlockOutlineFromBlock } from "./outline";
import type { NotesBlock, NotesBlockType, NotesBlockWrite, NotesParent } from "$lib/notes/types";

const now = "2026-06-30T09:00:00.000Z";

function blockFromWrite(write: NotesBlockWrite, parent: NotesParent): NotesBlock {
  const base = {
    object: "block" as const,
    id: write.id,
    parent,
    created_time: now,
    last_edited_time: now,
    has_children: false,
    in_trash: false,
    archived: false,
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
  };
  switch (write.type) {
    case "paragraph":
      return { ...base, type: write.type, paragraph: write.paragraph };
    case "heading_1":
      return { ...base, type: write.type, heading_1: write.heading_1 };
    case "heading_2":
      return { ...base, type: write.type, heading_2: write.heading_2 };
    case "heading_3":
      return { ...base, type: write.type, heading_3: write.heading_3 };
    case "heading_4":
      return { ...base, type: write.type, heading_4: write.heading_4 };
    case "heading_5":
      return { ...base, type: write.type, heading_5: write.heading_5 };
    case "heading_6":
      return { ...base, type: write.type, heading_6: write.heading_6 };
    case "bulleted_list_item":
      return { ...base, type: write.type, bulleted_list_item: write.bulleted_list_item };
    case "numbered_list_item":
      return { ...base, type: write.type, numbered_list_item: write.numbered_list_item };
    case "to_do":
      return { ...base, type: write.type, to_do: write.to_do };
    case "toggle":
      return { ...base, type: write.type, toggle: write.toggle };
    case "callout":
      return { ...base, type: write.type, callout: write.callout };
    case "quote":
      return { ...base, type: write.type, quote: write.quote };
    case "child_page":
      return { ...base, type: write.type, child_page: write.child_page };
    case "child_database":
      return { ...base, type: write.type, child_database: write.child_database };
    case "breadcrumb":
      return { ...base, type: write.type, breadcrumb: write.breadcrumb };
    case "table_of_contents":
      return { ...base, type: write.type, table_of_contents: write.table_of_contents };
    case "column_list":
      return { ...base, type: write.type, column_list: write.column_list };
    case "column":
      return { ...base, type: write.type, column: write.column };
    case "table":
      return { ...base, type: write.type, table: write.table };
    case "table_row":
      return { ...base, type: write.type, table_row: write.table_row };
    case "tab":
      return { ...base, type: write.type, tab: write.tab };
    case "image":
      return { ...base, type: write.type, image: write.image };
    case "video":
      return { ...base, type: write.type, video: write.video };
    case "audio":
      return { ...base, type: write.type, audio: write.audio };
    case "file":
      return { ...base, type: write.type, file: write.file };
    case "pdf":
      return { ...base, type: write.type, pdf: write.pdf };
    case "bookmark":
      return { ...base, type: write.type, bookmark: write.bookmark };
    case "link_preview":
      return { ...base, type: write.type, link_preview: write.link_preview };
    case "synced_block":
      return { ...base, type: write.type, synced_block: write.synced_block };
    case "template":
      return { ...base, type: write.type, template: write.template };
    case "button":
      return { ...base, type: write.type, button: write.button };
    case "embed":
      return { ...base, type: write.type, embed: write.embed };
    case "equation":
      return { ...base, type: write.type, equation: write.equation };
    case "divider":
      return { ...base, type: write.type, divider: write.divider };
    case "code":
      return { ...base, type: write.type, code: write.code };
    case "unsupported":
      return { ...base, type: write.type, unsupported: write.unsupported };
  }
}

function block(
  id: string,
  parent: NotesParent,
  text: string,
  type: NotesBlockType = "paragraph",
): NotesBlock {
  return blockFromWrite(createBlockWrite(id, type, text), parent);
}

function state(blocks: NotesBlock[]): NotesTreeState {
  return {
    blocksById: Object.fromEntries(blocks.map((item) => [item.id, item])),
    childIdsByParentId: buildNotesChildIdsByParent(blocks),
  };
}

describe("notes block tree", () => {
  it("loads indentation neighbours in outline order without loading their full subtrees", () => {
    const nodes = [
      block("parent", { type: "page_id", page_id: "page" }, "Parent"),
      block("before", { type: "block_id", block_id: "parent" }, "Before"),
      block("before-child", { type: "block_id", block_id: "before" }, "Existing child"),
      block("current", { type: "block_id", block_id: "parent" }, "Current"),
      block("child", { type: "block_id", block_id: "current" }, "Child"),
      block("after", { type: "block_id", block_id: "parent" }, "After"),
      block("deeper", { type: "block_id", block_id: "after" }, "Deeper"),
      block("other", { type: "page_id", page_id: "page" }, "Other"),
    ];
    const outlines = nodes.map((node, index) => notesBlockOutlineFromBlock(node, "page", index)).reverse();
    expect(notesIndentationContextIds(outlines, ["current"], "nest")).toEqual(["current", "parent", "child", "before", "before-child"]);
    expect(notesIndentationContextIds(outlines, ["current"], "outdent")).toEqual(["current", "parent", "child", "after"]);
  });

  it("flattens top-level and nested blocks in render order", () => {
    const tree = state([
      block("a", { type: "page_id", page_id: "page" }, "A"),
      block("b", { type: "page_id", page_id: "page" }, "B"),
      block("c", { type: "block_id", block_id: "b" }, "C"),
    ]);

    expect(flattenNotesBlockTree(tree, "page").map((item) => [item.block.id, item.depth])).toEqual([
      ["a", 0],
      ["b", 0],
      ["c", 1],
    ]);
  });

  it("hides descendants of closed toggles without removing them from state", () => {
    const tree = state([
      blockFromWrite(
        createBlockWrite("a", "toggle", "Details"),
        { type: "page_id", page_id: "page" },
      ),
      block("b", { type: "block_id", block_id: "a" }, "Hidden"),
    ]);
    const toggle = tree.blocksById.a;
    if (toggle?.type !== "toggle") throw new Error("fixture must create a toggle block");
    toggle.toggle.ganbaru_open = false;

    expect(flattenNotesBlockTree(tree, "page").map((item) => item.block.id)).toEqual(["a"]);
    expect(tree.childIdsByParentId.a).toEqual(["b"]);
  });

  it("shows original synced block children and hides duplicate reference children", () => {
    const tree = state([
      blockFromWrite(
        {
          id: "a",
          type: "synced_block",
          synced_block: createSyncedBlockPayload(),
        },
        { type: "page_id", page_id: "page" },
      ),
      block("b", { type: "block_id", block_id: "a" }, "Visible"),
      blockFromWrite(
        {
          id: "c",
          type: "synced_block",
          synced_block: createSyncedBlockPayload("a"),
        },
        { type: "page_id", page_id: "page" },
      ),
      block("d", { type: "block_id", block_id: "c" }, "Hidden"),
    ]);

    expect(flattenNotesBlockTree(tree, "page").map((item) => item.block.id)).toEqual([
      "a",
      "b",
      "c",
    ]);
    expect(tree.childIdsByParentId.c).toEqual(["d"]);
  });

  it("hides descendants of closed toggle headings without removing them from state", () => {
    const tree = state([
      blockFromWrite(
        {
          id: "a",
          type: "heading_2",
          heading_2: createTextPayload("Details", {
            isToggleable: true,
            open: false,
          }),
        },
        { type: "page_id", page_id: "page" },
      ),
      block("b", { type: "block_id", block_id: "a" }, "Hidden"),
    ]);

    expect(flattenNotesBlockTree(tree, "page").map((item) => item.block.id)).toEqual(["a"]);
    expect(tree.childIdsByParentId.a).toEqual(["b"]);
  });

  it("does not inline child page body blocks in the parent page tree", () => {
    const tree = state([
      blockFromWrite(
        createBlockWrite("child-page", "child_page", "Nested"),
        { type: "page_id", page_id: "page" },
      ),
      block("b", { type: "block_id", block_id: "child-page" }, "Hidden"),
    ]);
    const childPage = tree.blocksById["child-page"];
    if (childPage?.type !== "child_page") throw new Error("fixture must create a child page block");
    childPage.has_children = true;

    expect(flattenNotesBlockTree(tree, "page").map((item) => item.block.id)).toEqual([
      "child-page",
    ]);
  });

  it("keeps child database descendants visible in the parent page tree", () => {
    const tree = state([
      blockFromWrite(
        createBlockWrite("child-database", "child_database", "Tasks"),
        { type: "page_id", page_id: "page" },
      ),
      block("b", { type: "block_id", block_id: "child-database" }, "Visible"),
    ]);
    const childDatabase = tree.blocksById["child-database"];
    if (childDatabase?.type !== "child_database") {
      throw new Error("fixture must create a child database block");
    }
    childDatabase.has_children = true;

    expect(flattenNotesBlockTree(tree, "page").map((item) => item.block.id)).toEqual([
      "child-database",
      "b",
    ]);
  });

  it("keeps template children visible in the parent page tree", () => {
    const tree = state([
      blockFromWrite(
        createBlockWrite("template", "template", "Daily plan"),
        { type: "page_id", page_id: "page" },
      ),
      block("b", { type: "block_id", block_id: "template" }, "Visible"),
    ]);
    const template = tree.blocksById.template;
    if (template?.type !== "template") throw new Error("fixture must create a template block");
    template.has_children = true;

    expect(flattenNotesBlockTree(tree, "page").map((item) => item.block.id)).toEqual([
      "template",
      "b",
    ]);
  });

  it("keeps button action children visible in the parent page tree", () => {
    const tree = state([
      blockFromWrite(
        createBlockWrite("button", "button", "Add agenda"),
        { type: "page_id", page_id: "page" },
      ),
      block("b", { type: "block_id", block_id: "button" }, "Visible"),
    ]);
    const button = tree.blocksById.button;
    if (button?.type !== "button") throw new Error("fixture must create a button block");
    button.has_children = true;

    expect(flattenNotesBlockTree(tree, "page").map((item) => item.block.id)).toEqual([
      "button",
      "b",
    ]);
  });

  it("renders table blocks as one surface without exposing table rows", () => {
    const tree = state([
      blockFromWrite(
        createBlockWrite("table", "table"),
        { type: "page_id", page_id: "page" },
      ),
      blockFromWrite(
        createBlockWrite("row-a", "table_row"),
        { type: "block_id", block_id: "table" },
      ),
      blockFromWrite(
        createBlockWrite("row-b", "table_row"),
        { type: "block_id", block_id: "table" },
      ),
    ]);
    const table = tree.blocksById.table;
    if (table?.type !== "table") throw new Error("fixture must create a table block");
    table.has_children = true;

    expect(flattenNotesBlockTree(tree, "page").map((item) => item.block.id)).toEqual(["table"]);
    expect(tree.childIdsByParentId.table).toEqual(["row-a", "row-b"]);
  });

  it("renders column lists as one surface while each column flattens its own contents", () => {
    const tree = state([
      blockFromWrite(
        createBlockWrite("columns", "column_list"),
        { type: "page_id", page_id: "page" },
      ),
      blockFromWrite(
        createBlockWrite("left-column", "column"),
        { type: "block_id", block_id: "columns" },
      ),
      blockFromWrite(
        createBlockWrite("right-column", "column"),
        { type: "block_id", block_id: "columns" },
      ),
      block("left-a", { type: "block_id", block_id: "left-column" }, "Left"),
      block("right-a", { type: "block_id", block_id: "right-column" }, "Right"),
    ]);
    const columns = tree.blocksById.columns;
    const leftColumn = tree.blocksById["left-column"];
    const rightColumn = tree.blocksById["right-column"];
    if (columns?.type !== "column_list") throw new Error("fixture must create columns");
    if (leftColumn?.type !== "column") throw new Error("fixture must create a left column");
    if (rightColumn?.type !== "column") throw new Error("fixture must create a right column");
    columns.has_children = true;
    leftColumn.has_children = true;
    rightColumn.has_children = true;

    expect(flattenNotesBlockTree(tree, "page").map((item) => item.block.id)).toEqual(["columns"]);
    expect(flattenNotesBlockChildren(tree, "left-column").map((item) => item.block.id)).toEqual([
      "left-a",
    ]);
    expect(flattenNotesBlockChildren(tree, "right-column").map((item) => item.block.id)).toEqual([
      "right-a",
    ]);
  });

  it("renders tab blocks as one surface while each label flattens its panel contents", () => {
    const tree = state([
      blockFromWrite(
        createBlockWrite("tabs", "tab"),
        { type: "page_id", page_id: "page" },
      ),
      block("overview-label", { type: "block_id", block_id: "tabs" }, "Overview"),
      block("details-label", { type: "block_id", block_id: "tabs" }, "Details"),
      block("overview-content", { type: "block_id", block_id: "overview-label" }, "Summary"),
      block("details-content", { type: "block_id", block_id: "details-label" }, "Notes"),
    ]);
    const tabs = tree.blocksById.tabs;
    const overviewLabel = tree.blocksById["overview-label"];
    const detailsLabel = tree.blocksById["details-label"];
    if (tabs?.type !== "tab") throw new Error("fixture must create tabs");
    if (overviewLabel?.type !== "paragraph") {
      throw new Error("fixture must create the overview label");
    }
    if (detailsLabel?.type !== "paragraph") {
      throw new Error("fixture must create the details label");
    }
    tabs.has_children = true;
    overviewLabel.has_children = true;
    detailsLabel.has_children = true;

    expect(flattenNotesBlockTree(tree, "page").map((item) => item.block.id)).toEqual(["tabs"]);
    expect(tree.childIdsByParentId.tabs).toEqual(["overview-label", "details-label"]);
    expect(flattenNotesBlockChildren(tree, "overview-label").map((item) => item.block.id)).toEqual([
      "overview-content",
    ]);
    expect(flattenNotesBlockChildren(tree, "details-label").map((item) => item.block.id)).toEqual([
      "details-content",
    ]);
  });

  it("plans nesting under the previous sibling when that sibling accepts children", () => {
    const tree = state([
      block("a", { type: "page_id", page_id: "page" }, "A"),
      block("b", { type: "page_id", page_id: "page" }, "B"),
    ]);

    expect(planNestBlock(tree, "b")).toEqual({
      blockId: "b",
      parentId: "a",
      after: null,
    });
  });

  it("plans nesting under callouts because they accept children", () => {
    const tree = state([
      blockFromWrite(
        createBlockWrite("a", "callout", "Remember"),
        { type: "page_id", page_id: "page" },
      ),
      block("b", { type: "page_id", page_id: "page" }, "B"),
    ]);

    expect(planNestBlock(tree, "b")).toEqual({
      blockId: "b",
      parentId: "a",
      after: null,
    });
  });

  it("plans nesting under template blocks because they accept children", () => {
    const tree = state([
      blockFromWrite(
        createBlockWrite("a", "template", "Daily plan"),
        { type: "page_id", page_id: "page" },
      ),
      block("b", { type: "page_id", page_id: "page" }, "B"),
    ]);

    expect(planNestBlock(tree, "b")).toEqual({
      blockId: "b",
      parentId: "a",
      after: null,
    });
  });

  it("plans nesting under button blocks because they accept action children", () => {
    const tree = state([
      blockFromWrite(
        createBlockWrite("a", "button", "Add agenda"),
        { type: "page_id", page_id: "page" },
      ),
      block("b", { type: "page_id", page_id: "page" }, "B"),
    ]);

    expect(planNestBlock(tree, "b")).toEqual({
      blockId: "b",
      parentId: "a",
      after: null,
    });
  });

  it("plans nesting under toggleable headings only", () => {
    const toggleHeading = blockFromWrite(
      {
        id: "a",
        type: "heading_1",
        heading_1: createTextPayload("Heading", { isToggleable: true }),
      },
      { type: "page_id", page_id: "page" },
    );
    const normalHeading = blockFromWrite(
      createBlockWrite("c", "heading_1", "Normal heading"),
      { type: "page_id", page_id: "page" },
    );
    const tree = state([
      toggleHeading,
      block("b", { type: "page_id", page_id: "page" }, "B"),
      normalHeading,
      block("d", { type: "page_id", page_id: "page" }, "D"),
    ]);

    expect(planNestBlock(tree, "b")).toEqual({
      blockId: "b",
      parentId: "a",
      after: null,
    });
    expect(planNestBlock(tree, "d")).toBeNull();
  });

  it("does not plan normal nesting under a table block", () => {
    const tree = state([
      blockFromWrite(
        createBlockWrite("table", "table"),
        { type: "page_id", page_id: "page" },
      ),
      block("b", { type: "page_id", page_id: "page" }, "B"),
    ]);

    expect(planNestBlock(tree, "b")).toBeNull();
  });

  it("does not plan normal nesting under a column list block", () => {
    const tree = state([
      blockFromWrite(
        createBlockWrite("columns", "column_list"),
        { type: "page_id", page_id: "page" },
      ),
      block("b", { type: "page_id", page_id: "page" }, "B"),
    ]);

    expect(planNestBlock(tree, "b")).toBeNull();
  });

  it("does not plan normal nesting under a tab block", () => {
    const tree = state([
      blockFromWrite(
        createBlockWrite("tabs", "tab"),
        { type: "page_id", page_id: "page" },
      ),
      block("b", { type: "page_id", page_id: "page" }, "B"),
    ]);

    expect(planNestBlock(tree, "b")).toBeNull();
  });

  it("does not plan nesting internal structural blocks under normal blocks", () => {
    const tree = state([
      block("a", { type: "block_id", block_id: "columns" }, "A"),
      blockFromWrite(
        createBlockWrite("column", "column"),
        { type: "block_id", block_id: "columns" },
      ),
    ]);

    expect(planNestBlock(tree, "column")).toBeNull();
  });

  it("plans outdent after the parent block", () => {
    const tree = state([
      block("a", { type: "page_id", page_id: "page" }, "A"),
      block("b", { type: "block_id", block_id: "a" }, "B"),
    ]);

    expect(planOutdentBlock(tree, "b")).toEqual({
      blockId: "b",
      parentId: "page",
      after: "a",
    });
  });

  it("does not outdent tab panel content directly into the tab label layer", () => {
    const tree = state([
      blockFromWrite(
        createBlockWrite("tabs", "tab"),
        { type: "page_id", page_id: "page" },
      ),
      block("label", { type: "block_id", block_id: "tabs" }, "Overview"),
      block("content", { type: "block_id", block_id: "label" }, "Summary"),
    ]);

    expect(planOutdentBlock(tree, "content")).toBeNull();
  });

  it("does not outdent column content directly into the column list layer", () => {
    const tree = state([
      blockFromWrite(
        createBlockWrite("columns", "column_list"),
        { type: "page_id", page_id: "page" },
      ),
      blockFromWrite(
        createBlockWrite("column", "column"),
        { type: "block_id", block_id: "columns" },
      ),
      block("content", { type: "block_id", block_id: "column" }, "Column content"),
    ]);

    expect(planOutdentBlock(tree, "content")).toBeNull();
  });

  it("outdents nested column content to its owning column when valid", () => {
    const tree = state([
      blockFromWrite(
        createBlockWrite("columns", "column_list"),
        { type: "page_id", page_id: "page" },
      ),
      blockFromWrite(
        createBlockWrite("column", "column"),
        { type: "block_id", block_id: "columns" },
      ),
      block("parent", { type: "block_id", block_id: "column" }, "Parent"),
      block("child", { type: "block_id", block_id: "parent" }, "Child"),
    ]);

    expect(planOutdentBlock(tree, "child")).toEqual({
      blockId: "child",
      parentId: "column",
      after: "parent",
    });
  });

  it("plans moving a block up within the current sibling group", () => {
    const tree = state([
      block("a", { type: "page_id", page_id: "page" }, "A"),
      block("b", { type: "page_id", page_id: "page" }, "B"),
      block("c", { type: "page_id", page_id: "page" }, "C"),
    ]);

    expect(planMoveBlockWithinSiblings(tree, "b", "up")).toEqual({
      blockId: "b",
      parentId: "page",
      after: null,
      before: "a",
    });
    expect(planMoveBlockWithinSiblings(tree, "c", "up")).toEqual({
      blockId: "c",
      parentId: "page",
      after: null,
      before: "b",
    });
  });

  it("plans moving a block down within the current sibling group", () => {
    const tree = state([
      block("a", { type: "page_id", page_id: "page" }, "A"),
      block("b", { type: "page_id", page_id: "page" }, "B"),
      block("c", { type: "page_id", page_id: "page" }, "C"),
    ]);

    expect(planMoveBlockWithinSiblings(tree, "a", "down")).toEqual({
      blockId: "a",
      parentId: "page",
      after: "b",
      before: null,
    });
    expect(planMoveBlockWithinSiblings(tree, "b", "down")).toEqual({
      blockId: "b",
      parentId: "page",
      after: "c",
      before: null,
    });
  });

  it("does not plan sibling movement past current boundaries", () => {
    const tree = state([
      block("a", { type: "page_id", page_id: "page" }, "A"),
      block("b", { type: "page_id", page_id: "page" }, "B"),
    ]);

    expect(planMoveBlockWithinSiblings(tree, "a", "up")).toBeNull();
    expect(planMoveBlockWithinSiblings(tree, "b", "down")).toBeNull();
  });

  it("does not expose internal table rows or columns as movable document blocks", () => {
    const tree = state([
      blockFromWrite(
        createBlockWrite("table", "table"),
        { type: "page_id", page_id: "page" },
      ),
      blockFromWrite(
        createBlockWrite("row", "table_row"),
        { type: "block_id", block_id: "table" },
      ),
      blockFromWrite(
        createBlockWrite("columns", "column_list"),
        { type: "page_id", page_id: "page" },
      ),
      blockFromWrite(
        createBlockWrite("column", "column"),
        { type: "block_id", block_id: "columns" },
      ),
    ]);

    expect(planMoveBlockWithinSiblings(tree, "row", "up")).toBeNull();
    expect(planMoveBlockWithinSiblings(tree, "column", "down")).toBeNull();
  });

  it("plans dropping a block before a sibling", () => {
    const tree = state([
      block("a", { type: "page_id", page_id: "page" }, "A"),
      block("b", { type: "page_id", page_id: "page" }, "B"),
      block("c", { type: "page_id", page_id: "page" }, "C"),
    ]);

    expect(planDropBlockWithinSiblings(tree, "c", "a", "before")).toEqual({
      blockId: "c",
      parentId: "page",
      after: null,
      before: "a",
    });
  });

  it("plans dropping a block after a sibling", () => {
    const tree = state([
      block("a", { type: "page_id", page_id: "page" }, "A"),
      block("b", { type: "page_id", page_id: "page" }, "B"),
      block("c", { type: "page_id", page_id: "page" }, "C"),
    ]);

    expect(planDropBlockWithinSiblings(tree, "a", "c", "after")).toEqual({
      blockId: "a",
      parentId: "page",
      after: "c",
      before: null,
    });
  });

  it("does not plan sibling drops that would keep adjacent order unchanged", () => {
    const tree = state([
      block("a", { type: "page_id", page_id: "page" }, "A"),
      block("b", { type: "page_id", page_id: "page" }, "B"),
    ]);

    expect(planDropBlockWithinSiblings(tree, "a", "b", "before")).toBeNull();
    expect(planDropBlockWithinSiblings(tree, "b", "a", "after")).toBeNull();
  });

  it("does not plan sibling drops across different parents", () => {
    const tree = state([
      block("a", { type: "page_id", page_id: "page" }, "A"),
      block("b", { type: "block_id", block_id: "a" }, "B"),
    ]);

    expect(planDropBlockWithinSiblings(tree, "b", "a", "before")).toBeNull();
  });

  it("keeps the only block as an empty paragraph on delete", () => {
    const tree = state([block("a", { type: "page_id", page_id: "page" }, "")]);
    const flat = flattenNotesBlockTree(tree, "page");

    expect(planDeleteBlock(flat, "a")).toEqual({
      deleteBlockId: null,
      focusBlockId: "a",
      keepOnlyBlockAsParagraph: true,
    });
  });

  it("plans deletion focus and rich text merge", () => {
    const tree = state([
      block("a", { type: "page_id", page_id: "page" }, "A"),
      block("b", { type: "page_id", page_id: "page" }, "B"),
    ]);
    const flat = flattenNotesBlockTree(tree, "page");
    const merge = planMergeWithPrevious(flat, "b");

    expect(planDeleteBlock(flat, "b")?.focusBlockId).toBe("a");
    expect(planDeleteBlock(flat, "a")?.focusBlockId).toBe("b");
    expect(merge).toMatchObject({
      sourceBlockId: "b",
      targetBlockId: "a",
      targetCursorOffset: 1,
    });
    expect(merge ? richTextPlainText(merge.mergedRichText) : "").toBe("AB");
  });

  it("plans child reparenting before deleting a block", () => {
    const tree = state([
      block("a", { type: "page_id", page_id: "page" }, ""),
      block("child-a", { type: "block_id", block_id: "a" }, "A"),
      block("child-b", { type: "block_id", block_id: "a" }, "B"),
    ]);

    expect(planReparentChildrenBeforeDelete(tree, "a")).toEqual({
      childIds: ["child-a", "child-b"],
      parentId: "page",
      after: "a",
    });
  });

  it("moves merged block children under a child-capable target", () => {
    const tree = state([
      block("target", { type: "page_id", page_id: "page" }, "A"),
      block("target-child", { type: "block_id", block_id: "target" }, "Target child"),
      block("source", { type: "page_id", page_id: "page" }, "B"),
      block("source-child", { type: "block_id", block_id: "source" }, "Source child"),
    ]);

    expect(planReparentChildrenAfterMerge(tree, "source", "target")).toEqual({
      childIds: ["source-child"],
      parentId: "target",
      after: "target-child",
    });
  });

  it("keeps merged block children near the source when the target cannot own children", () => {
    const tree = state([
      block("target", { type: "page_id", page_id: "page" }, "Heading", "heading_1"),
      block("source", { type: "page_id", page_id: "page" }, "B"),
      block("source-child", { type: "block_id", block_id: "source" }, "Source child"),
    ]);

    expect(planReparentChildrenAfterMerge(tree, "source", "target")).toEqual({
      childIds: ["source-child"],
      parentId: "page",
      after: "source",
    });
  });
});
