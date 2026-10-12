import { describe, expect, it } from "vitest";
import {
  NOTES_BLOCK_CATALOG_BLOCKED_FUTURE_TYPES,
  completeNotesBlockCatalogGateEvidence,
  isNotesCatalogRegisteredBlockType,
  notesBlockCatalogGateDecision,
  notesBlockCatalogRegisteredTypes,
} from "./catalog";
import {
  isNotesInsertableBlockType,
  notesInsertableBlockTypes,
} from "./insertion";

describe("notes block catalog gate", () => {
  it("keeps known future Notion block types out of the shipped local catalog", () => {
    for (const blockType of NOTES_BLOCK_CATALOG_BLOCKED_FUTURE_TYPES) {
      expect(isNotesCatalogRegisteredBlockType(blockType)).toBe(false);
      expect(isNotesInsertableBlockType(blockType)).toBe(false);
    }
  });

  it("requires every shipping requirement before a future block can be registered", () => {
    const decision = notesBlockCatalogGateDecision({
      blockType: "meeting_notes",
      evidence: {
        ...completeNotesBlockCatalogGateEvidence(),
        usable_editing_ui: false,
      },
    });

    expect(decision.allowed).toBe(false);
    expect(decision.reason).toBe("blocked_future_type");
    expect(decision.missingRequirements).toEqual(["usable_editing_ui"]);
  });

  it("only allows unregistered future blocks after the full gate evidence exists", () => {
    const decision = notesBlockCatalogGateDecision({
      blockType: "meeting_notes",
      evidence: completeNotesBlockCatalogGateEvidence(),
    });

    expect(decision.allowed).toBe(true);
    expect(decision.reason).toBe("ready_for_registration");
    expect(isNotesCatalogRegisteredBlockType("meeting_notes")).toBe(false);
  });

  it("treats current shipped block types as already registered", () => {
    const decision = notesBlockCatalogGateDecision({
      blockType: "paragraph",
      evidence: {
        ...completeNotesBlockCatalogGateEvidence(),
        persistence: false,
      },
    });

    expect(decision.allowed).toBe(true);
    expect(decision.reason).toBe("already_registered");
    expect(decision.missingRequirements).toEqual([]);
  });

  it("keeps insertable blocks inside the registered catalog", () => {
    const registeredTypes = notesBlockCatalogRegisteredTypes();

    for (const blockType of notesInsertableBlockTypes()) {
      expect(registeredTypes).toContain(blockType);
    }
  });
});
