import { describe, expect, it } from "vitest";
import type { TimelineActivityRow } from "./model";
import {
  activityFilePath,
  commandActivityPresentation,
  fileReadActivityPresentation,
  fileSearchActivityPresentation,
  fileChangePresentation,
  summarizeActivityKinds,
  transientActivitySummary,
} from "./activity-presentation";

function activity(overrides: Partial<TimelineActivityRow> = {}): TimelineActivityRow {
  return {
    id: "command-1",
    kind: "activity",
    turnId: "turn-1",
    sequence: 1,
    createdAt: "2026-07-27T23:00:00.000Z",
    activityKind: "command_execution",
    status: "completed",
    title: "/usr/bin/bash -lc 'printf 4'",
    detail: "4\n",
    metadata: {
      schemaVersion: 1,
      value: { cwd: "/workspace", exitCode: 0, durationMs: 18 },
    },
    ...overrides,
  };
}

describe("command activity presentation", () => {
  it("keeps command lifecycle data separate from streamed output", () => {
    expect(commandActivityPresentation(activity())).toEqual({
      command: "/usr/bin/bash -lc 'printf 4'",
      cwd: "/workspace",
      output: "4",
      exitCode: 0,
      durationMs: 18,
      running: false,
    });
  });

  it("represents an active command without fabricating output", () => {
    expect(commandActivityPresentation(activity({ status: "active", detail: null }))).toMatchObject({
      output: null,
      running: true,
    });
  });
});

describe("file change presentation", () => {
  it("parses file paths, kinds, diffs, and line counts", () => {
    expect(fileChangePresentation(activity({
      activityKind: "file_change",
      metadata: {
        schemaVersion: 1,
        value: {
          changes: [{
            path: "src/app.ts",
            kind: "update",
            diff: "--- a/src/app.ts\n+++ b/src/app.ts\n-old\n+new",
          }],
        },
      },
    }))).toEqual([{
      path: "src/app.ts",
      kind: "update",
      diff: "--- a/src/app.ts\n+++ b/src/app.ts\n-old\n+new",
      additions: 1,
      deletions: 1,
    }]);
  });

  it("uses provider input metadata when a structured diff is not available", () => {
    expect(activityFilePath(activity({
      activityKind: "file_change",
      title: "Edit",
      metadata: {
        schemaVersion: 1,
        value: { toolInput: { file_path: "src/provider.ts" } },
      },
    }))).toBe("src/provider.ts");
  });
});

describe("activity summaries", () => {
  it("combines mixed consecutive actions and counts repeated kinds", () => {
    expect(summarizeActivityKinds([
      activity({ id: "edit", activityKind: "file_change", title: "Edit file" }),
      activity({ id: "read-one", activityKind: "dynamic_tool_call", title: "read_file" }),
      activity({ id: "read-two", activityKind: "mcp_tool_call", title: "workspace_read_file" }),
      activity({ id: "command", activityKind: "command_execution", title: "pnpm test" }),
    ])).toEqual([
      { kind: "file_changes", count: 1 },
      { kind: "file_reads", count: 2 },
      { kind: "commands", count: 1 },
    ]);
  });

  it("uses generic semantic kinds instead of exposing provider tool names", () => {
    expect(summarizeActivityKinds([
      activity({ id: "search", activityKind: "web_search", title: "Svelte snippets" }),
      activity({ id: "image", activityKind: "dynamic_tool_call", title: "view_image" }),
      activity({ id: "tool", activityKind: "mcp_tool_call", title: "custom_provider_tool" }),
    ])).toEqual([
      { kind: "web_searches", count: 1 },
      { kind: "image_views", count: 1 },
      { kind: "tools", count: 1 },
    ]);
  });

  it("classifies local shell search and read commands by their user-visible action", () => {
    const search = activity({
      id: "search",
      title: "rg -n 'Worked for|Ran commands' apps/client/src/lib/components/chat",
    });
    const read = activity({
      id: "read",
      title: "/usr/bin/bash -lc \"sed -n '1,220p' apps/client/src/lib/components/chat/ChatTimeline.svelte\"",
    });

    expect(fileSearchActivityPresentation(search)).toEqual({
      query: "Worked for|Ran commands",
      scope: "apps/client/src/lib/components/chat",
    });
    expect(fileReadActivityPresentation(read)).toEqual({
      path: "apps/client/src/lib/components/chat/ChatTimeline.svelte",
    });
    expect(summarizeActivityKinds([search, read])).toEqual([
      { kind: "file_searches", count: 1 },
      { kind: "file_reads", count: 1 },
    ]);
  });

  it("recognizes provider tool names and input metadata across canonical tool kinds", () => {
    const claudeRead = activity({
      id: "claude-read",
      activityKind: "dynamic_tool_call",
      title: "Read",
      metadata: {
        schemaVersion: 1,
        value: { toolInput: { file_path: "src/claude.ts" } },
      },
    });
    const openCodeSearch = activity({
      id: "opencode-search",
      activityKind: "dynamic_tool_call",
      title: "Grep",
      metadata: {
        schemaVersion: 1,
        value: { state: { input: { pattern: "timeline", path: "src/chat" } } },
      },
    });

    expect(fileReadActivityPresentation(claudeRead)).toEqual({ path: "src/claude.ts" });
    expect(fileSearchActivityPresentation(openCodeSearch)).toEqual({
      query: "timeline",
      scope: "src/chat",
    });
  });

  it("distinguishes ACP workspace search from external fetch metadata", () => {
    const cursorSearch = activity({
      id: "cursor-search",
      activityKind: "web_search",
      title: "Search workspace",
      metadata: {
        schemaVersion: 1,
        value: { toolKind: "search", locations: [{ relativePath: "src" }] },
      },
    });
    const cursorFetch = activity({
      id: "cursor-fetch",
      activityKind: "web_search",
      title: "Fetch documentation",
      metadata: { schemaVersion: 1, value: { toolKind: "fetch" } },
    });

    expect(fileSearchActivityPresentation(cursorSearch)).toEqual({ query: null, scope: "src" });
    expect(fileSearchActivityPresentation(cursorFetch)).toBeNull();
    expect(summarizeActivityKinds([cursorSearch, cursorFetch])).toEqual([
      { kind: "file_searches", count: 1 },
      { kind: "web_searches", count: 1 },
    ]);
  });

  it("does not classify transient reasoning as tool use", () => {
    expect(summarizeActivityKinds([
      activity({ id: "reasoning", activityKind: "reasoning", title: "Reasoning" }),
      activity({ id: "reasoning-summary", activityKind: "reasoning_summary", title: "Reasoning" }),
    ])).toEqual([{ kind: "thinking", count: 2 }]);
  });

  it("shows only provider-designated reasoning summaries as bounded live status", () => {
    expect(transientActivitySummary(activity({
      activityKind: "reasoning_summary",
      detail: "Inspecting the current timeline\n\nRefining the attachment preview",
    }))).toBe("Refining the attachment preview");
    expect(transientActivitySummary(activity({
      activityKind: "reasoning_text",
      detail: "Private provider reasoning",
    }))).toBeNull();
    expect(transientActivitySummary(activity({
      activityKind: "reasoning_summary",
      detail: "x".repeat(200),
    }))).toHaveLength(160);
  });
});
