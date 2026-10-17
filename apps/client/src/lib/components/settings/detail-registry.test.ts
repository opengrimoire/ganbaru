import { describe, expect, it } from "vitest";
import {
  loadNotesTransferDialog,
  notesTransferDialogHasLoaded,
} from "$lib/components/settings/sections/notes-transfer-dialog-registry";
import {
  loadSettingsDetail,
  settingsDetailHasLoaded,
} from "$lib/components/settings/detail-registry";
import { NOTES_TRANSFER_OPERATIONS } from "$lib/settings/types";

describe("Settings detail registries", () => {
  it("loads only the selected detail panel and shares its in-flight import", async () => {
    const first = loadSettingsDetail("notes-transfer");
    const second = loadSettingsDetail("notes-transfer");

    expect(second).toBe(first);
    expect(settingsDetailHasLoaded("distractions-limit")).toBe(false);
    await expect(first).resolves.toMatchObject({ kind: "notes-transfer" });
    expect(settingsDetailHasLoaded("notes-transfer")).toBe(true);
  });

  it("loads only the selected Notes transfer dialog before other operations", async () => {
    const first = loadNotesTransferDialog("html-import");
    const second = loadNotesTransferDialog("html-import");

    expect(second).toBe(first);
    for (const operation of NOTES_TRANSFER_OPERATIONS) {
      if (operation !== "html-import") expect(notesTransferDialogHasLoaded(operation)).toBe(false);
    }
    await expect(first).resolves.toMatchObject({ operation: "html-import" });
  });

  it("resolves every Notes transfer operation to its matching constructor", async () => {
    for (const operation of NOTES_TRANSFER_OPERATIONS) {
      const loaded = await loadNotesTransferDialog(operation);
      expect(loaded.operation).toBe(operation);
    }
  });
});
