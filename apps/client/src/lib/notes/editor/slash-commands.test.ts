import { describe, expect, it } from "vitest";
import {
  clampNotesSlashActiveIndex,
  filterNotesSlashCommandItems,
  flatNotesSlashCommandSectionItems,
  nextNotesSlashActiveIndex,
  notesSlashInputSessionFromText,
  notesSlashCommandKey,
  notesSlashCommandItems,
  recordNotesSlashCommandKey,
  sectionNotesSlashCommandItems,
} from "./slash-commands";
import {
  notesBlockInsertCommandKey,
  notesBlockInsertCommands,
} from "$lib/notes/blocks/insertion";

describe("notes slash commands", () => {
  it("builds block action and color commands for color-capable blocks", () => {
    const commands = notesSlashCommandItems({ canSetColor: true });

    expect(commands).toContainEqual(
      expect.objectContaining({
        section: "blocks",
        command: { kind: "block", blockType: "paragraph" },
      }),
    );
    expect(commands).toContainEqual(
      expect.objectContaining({
        section: "actions",
        command: { kind: "action", action: "duplicate" },
      }),
    );
    expect(commands).toContainEqual(
      expect.objectContaining({
        section: "colors",
        command: { kind: "color", color: "red_background" },
      }),
    );
  });

  it("omits color commands for blocks that cannot store block color", () => {
    const commands = notesSlashCommandItems({ canSetColor: false });

    expect(commands.some((command) => command.command.kind === "color")).toBe(false);
    expect(commands.some((command) => command.command.kind === "action")).toBe(true);
    expect(commands.some((command) => command.command.kind === "block")).toBe(true);
  });

  it("builds toggle heading commands in the block section", () => {
    const commands = notesSlashCommandItems({ canSetColor: false });

    expect(commands).toContainEqual(
      expect.objectContaining({
        section: "blocks",
        command: { kind: "toggle_heading", headingType: "heading_2" },
      }),
    );
    expect(commands).toContainEqual(
      expect.objectContaining({
        section: "blocks",
        command: { kind: "toggle_heading", headingType: "heading_4" },
      }),
    );
    expect(notesSlashCommandKey({ kind: "toggle_heading", headingType: "heading_2" })).toBe(
      "toggle-heading:heading_2",
    );
  });

  it("uses the shared block insertion catalog for block commands", () => {
    const commands = notesSlashCommandItems({ canSetColor: false });
    const blockKeys = commands
      .filter((command) => command.section === "blocks")
      .map((command) => command.key);

    expect(blockKeys).toEqual(notesBlockInsertCommands().map(notesBlockInsertCommandKey));
  });

  it("filters by command aliases", () => {
    const commands = notesSlashCommandItems({ canSetColor: true });

    expect(commands.every((command) => command.searchText.length > 0)).toBe(true);
    expect(filterNotesSlashCommandItems(commands, "dup")).toContainEqual(
      expect.objectContaining({ command: { kind: "action", action: "duplicate" } }),
    );
    expect(filterNotesSlashCommandItems(commands, "red bg")).toContainEqual(
      expect.objectContaining({ command: { kind: "color", color: "red_background" } }),
    );
    expect(filterNotesSlashCommandItems(commands, "toc")).toContainEqual(
      expect.objectContaining({ command: { kind: "block", blockType: "table_of_contents" } }),
    );
    expect(filterNotesSlashCommandItems(commands, "preview")).toContainEqual(
      expect.objectContaining({ command: { kind: "block", blockType: "link_preview" } }),
    );
    expect(filterNotesSlashCommandItems(commands, "template button")).toContainEqual(
      expect.objectContaining({ command: { kind: "block", blockType: "template" } }),
    );
    expect(filterNotesSlashCommandItems(commands, "insert blocks")).toContainEqual(
      expect.objectContaining({ command: { kind: "block", blockType: "button" } }),
    );
    expect(filterNotesSlashCommandItems(commands, "tabs")).toContainEqual(
      expect.objectContaining({ command: { kind: "block", blockType: "tab" } }),
    );
    expect(filterNotesSlashCommandItems(commands, "toggle h2")).toContainEqual(
      expect.objectContaining({ command: { kind: "toggle_heading", headingType: "heading_2" } }),
    );
    expect(filterNotesSlashCommandItems(commands, "h4")).toContainEqual(
      expect.objectContaining({ command: { kind: "block", blockType: "heading_4" } }),
    );
    expect(filterNotesSlashCommandItems(commands, "toggle h4")).toContainEqual(
      expect.objectContaining({ command: { kind: "toggle_heading", headingType: "heading_4" } }),
    );
  });

  it("keeps recent commands first without duplicating them in their normal section", () => {
    const commands = notesSlashCommandItems({ canSetColor: true });
    const recentKeys = recordNotesSlashCommandKey([], "action:duplicate");
    const sections = sectionNotesSlashCommandItems(commands, "", recentKeys);

    expect(sections.recent).toEqual([
      expect.objectContaining({ command: { kind: "action", action: "duplicate" } }),
    ]);
    expect(sections.actions).not.toContainEqual(
      expect.objectContaining({ command: { kind: "action", action: "duplicate" } }),
    );
  });

  it("does not show recent commands while filtering", () => {
    const commands = notesSlashCommandItems({ canSetColor: true });
    const recentKeys = recordNotesSlashCommandKey([], "action:duplicate");
    const sections = sectionNotesSlashCommandItems(commands, "dup", recentKeys);

    expect(sections.recent).toEqual([]);
    expect(sections.actions).toContainEqual(
      expect.objectContaining({ command: { kind: "action", action: "duplicate" } }),
    );
  });

  it("flattens sections and wraps keyboard active index movement", () => {
    const commands = notesSlashCommandItems({ canSetColor: false });
    const sections = sectionNotesSlashCommandItems(commands, "dup", []);
    const items = flatNotesSlashCommandSectionItems(sections, [
      "recent",
      "blocks",
      "actions",
      "colors",
    ]);

    expect(items).toContainEqual(
      expect.objectContaining({ command: { kind: "action", action: "duplicate" } }),
    );
    expect(clampNotesSlashActiveIndex(10, items.length)).toBe(items.length - 1);
    expect(nextNotesSlashActiveIndex(items.length - 1, items.length, "next")).toBe(0);
    expect(nextNotesSlashActiveIndex(0, items.length, "previous")).toBe(items.length - 1);
  });

  it("matches exact heading markers and accent-insensitive localized labels", () => {
    const items = notesSlashCommandItems({ canSetColor: false });
    expect(filterNotesSlashCommandItems(items, "##").map((item) => item.key)).toEqual(["block:heading_2"]);
    const localized = [{ ...items[0], searchText: "parrafo texto" }];
    expect(filterNotesSlashCommandItems(localized, "párrafo")).toHaveLength(1);
  });

  it("opens new input sessions but retains dismissal and rejects ordinary paths or multiline text", () => {
    expect(notesSlashInputSessionFromText("/", false, "")).toEqual({ open: true, query: "" });
    expect(notesSlashInputSessionFromText("/h2", false, "")).toEqual({ open: true, query: "h2" });
    expect(notesSlashInputSessionFromText("/h2", false, "/h").open).toBe(false);
    expect(notesSlashInputSessionFromText("", true, "/").open).toBe(false);
    expect(notesSlashInputSessionFromText("path/to", false, "path").open).toBe(false);
  });

  it("keeps typed slash sessions open only after an intentional slash trigger", () => {
    expect(notesSlashInputSessionFromText("/hea", true)).toEqual({
      open: true,
      query: "hea",
    });
    expect(notesSlashInputSessionFromText("/hea", false)).toEqual({
      open: false,
      query: "",
    });
    expect(notesSlashInputSessionFromText("/one\ntwo", true)).toEqual({
      open: false,
      query: "",
    });
    expect(notesSlashInputSessionFromText("plain", true)).toEqual({
      open: false,
      query: "",
    });
  });
});
