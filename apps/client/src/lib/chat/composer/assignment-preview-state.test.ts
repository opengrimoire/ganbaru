import { describe, expect, it } from "vitest";
import type { ChatAssignmentTargetRead } from "$lib/chat/contracts";
import {
  ASSIGNMENT_PREVIEW_OFF,
  previewTargets,
  transitionAssignmentPreview,
  type AssignmentPreviewState,
} from "./assignment-preview-state";

const target = { displayName: "Folder", eligible: true } as unknown as ChatAssignmentTargetRead;
const loading = (request: number): AssignmentPreviewState => (
  transitionAssignmentPreview(ASSIGNMENT_PREVIEW_OFF, { type: "requested", request })
);

describe("transitionAssignmentPreview", () => {
  it("lands the targets of the read in flight", () => {
    const ready = transitionAssignmentPreview(loading(1), { type: "loaded", request: 1, targets: [target] });
    expect(ready).toEqual({ phase: "ready", targets: [target] });
    expect(previewTargets(ready)).toEqual([target]);
  });

  it("drops the targets of an earlier read once another started", () => {
    const second = transitionAssignmentPreview(loading(1), { type: "requested", request: 2 });
    expect(transitionAssignmentPreview(second, { type: "loaded", request: 1, targets: [target] })).toBe(second);
  });

  it("drops a read that lands after the preview turned off", () => {
    const off = transitionAssignmentPreview(loading(3), { type: "disabled" });
    expect(off).toEqual(ASSIGNMENT_PREVIEW_OFF);
    expect(transitionAssignmentPreview(off, { type: "loaded", request: 3, targets: [target] })).toBe(off);
  });

  it("shows no targets while a read runs, even after earlier ones landed", () => {
    const ready = transitionAssignmentPreview(loading(1), { type: "loaded", request: 1, targets: [target] });
    const again = transitionAssignmentPreview(ready, { type: "requested", request: 2 });
    expect(previewTargets(again)).toEqual([]);
  });

  it("ignores a result once the read settled", () => {
    const ready = transitionAssignmentPreview(loading(1), { type: "loaded", request: 1, targets: [] });
    expect(transitionAssignmentPreview(ready, { type: "loaded", request: 1, targets: [target] })).toBe(ready);
  });

  it("clears landed targets after a delivery and leaves other phases alone", () => {
    const ready = transitionAssignmentPreview(loading(1), { type: "loaded", request: 1, targets: [target] });
    expect(transitionAssignmentPreview(ready, { type: "cleared" })).toEqual({ phase: "ready", targets: [] });
    const pending = loading(2);
    expect(transitionAssignmentPreview(pending, { type: "cleared" })).toBe(pending);
    expect(transitionAssignmentPreview(ASSIGNMENT_PREVIEW_OFF, { type: "cleared" })).toBe(ASSIGNMENT_PREVIEW_OFF);
    expect(transitionAssignmentPreview(ASSIGNMENT_PREVIEW_OFF, { type: "disabled" })).toBe(ASSIGNMENT_PREVIEW_OFF);
  });
});
