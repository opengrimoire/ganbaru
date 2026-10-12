import { describe, expect, it, vi } from "vitest";
import {
  classifyLoadFailure,
  recoverLoadFailure,
} from "./module-load-recovery";

describe("classifyLoadFailure", () => {
  it.each([
    "Failed to fetch dynamically imported module: http://127.0.0.1:1420/src/view.svelte",
    "Error loading dynamically imported module: http://localhost/view.js",
    "Importing a module script failed.",
    "Failed to load module script: Expected a JavaScript-or-Wasm module script",
  ])("requires a document reload for browser module failure: %s", (message) => {
    expect(classifyLoadFailure(new TypeError(message))).toEqual({
      message,
      requiresDocumentReload: true,
    });
  });

  it("keeps ordinary application failures retryable in place", () => {
    expect(classifyLoadFailure(new Error("database is temporarily busy"))).toEqual({
      message: "database is temporarily busy",
      requiresDocumentReload: false,
    });
  });

  it("preserves non-Error rejection details", () => {
    expect(classifyLoadFailure("network unavailable")).toEqual({
      message: "network unavailable",
      requiresDocumentReload: false,
    });
  });

  it("reads the message of a native command error object", () => {
    expect(classifyLoadFailure({ code: "read_only", message: "This vault is read-only on this device" })).toEqual({
      message: "This vault is read-only on this device",
      requiresDocumentReload: false,
    });
  });
});

describe("recoverLoadFailure", () => {
  it("reloads instead of repeating a browser-cached module import", () => {
    const retry = vi.fn();
    const reload = vi.fn();

    recoverLoadFailure({
      message: "Failed to fetch dynamically imported module",
      requiresDocumentReload: true,
    }, retry, reload);

    expect(reload).toHaveBeenCalledOnce();
    expect(retry).not.toHaveBeenCalled();
  });

  it("retries ordinary failures without reloading the document", () => {
    const retry = vi.fn();
    const reload = vi.fn();

    recoverLoadFailure({
      message: "database is temporarily busy",
      requiresDocumentReload: false,
    }, retry, reload);

    expect(retry).toHaveBeenCalledOnce();
    expect(reload).not.toHaveBeenCalled();
  });
});
