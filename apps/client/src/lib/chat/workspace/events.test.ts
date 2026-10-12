import { describe, expect, it } from "vitest";
import {
  chatWorkspaceRequest,
  isChatWorkspaceRequest,
} from "./events";

describe("Chat workspace event routing", () => {
  it("routes existing file and review events without changing their payload", () => {
    const fileDetail = { relativePath: "src/main.ts" };
    expect(chatWorkspaceRequest("ganbaru-ai:chat-open-file", fileDetail)).toEqual({
      source: "file",
      detail: fileDetail,
    });
    expect(chatWorkspaceRequest("ganbaru-ai:chat-open-review", null)).toEqual({
      source: "review",
      detail: null,
    });
  });

  it("rejects unknown or malformed internal requests", () => {
    expect(chatWorkspaceRequest("ganbaru-ai:unknown", {})).toBeNull();
    expect(isChatWorkspaceRequest({ source: "panel", detail: null })).toBe(false);
    expect(isChatWorkspaceRequest({ source: "files", detail: {} })).toBe(false);
  });
});
