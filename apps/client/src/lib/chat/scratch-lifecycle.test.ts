import { describe, expect, it } from "vitest";
import {
  parseChatScratchCleanupPreview,
  parseChatScratchCleanupResult,
  parseChatScratchDirectoryPage,
  parseChatScratchPromotionResult,
  parseChatScratchScope,
} from "./validation/organizational";

const timestamp = "2026-08-17T13:00:00Z";
const digest = "a".repeat(64);

function scratchScopeFixture(): Record<string, unknown> {
  return {
    id: "scratch-scope:test",
    replyThreadId: "reply-thread:test",
    teammateId: "participant:atlas",
    teammateName: "Atlas",
    channelId: "channel:frontend",
    channelName: "frontend",
    projectId: "project:app",
    projectName: "App",
    groupId: "group:engineering",
    groupName: "Engineering",
    lifecycleState: "active",
    revision: 3,
    generations: [{
      id: "scratch-generation:test",
      executionEnvironmentId: "environment:scratch",
      generation: 2,
      lifecycleState: "quarantined",
      byteSize: 4096,
      entryCount: 6,
      sizeTruncated: false,
      deviceAvailability: "unavailableOnThisDevice",
      retainedSources: [{
        channelId: "channel:leadership",
        channelName: "leadership",
        lowerOrderKey: "c004",
        highOrderKey: "c00I",
        audienceRevision: 7,
      }],
      createdAt: timestamp,
      updatedAt: timestamp,
    }],
    createdAt: timestamp,
    updatedAt: timestamp,
  };
}

describe("private scratch lifecycle contracts", () => {
  it("preserves logical scope, quarantine, source, and device state without a path", () => {
    const scope = parseChatScratchScope(scratchScopeFixture());

    expect(scope.generations[0]).toMatchObject({
      lifecycleState: "quarantined",
      deviceAvailability: "unavailableOnThisDevice",
      retainedSources: [{ channelId: "channel:leadership", highOrderKey: "c00I" }],
    });
    expect(JSON.stringify(scope)).not.toContain("/home/");
  });

  it("accepts a bounded page with revision-checked promotable files", () => {
    const page = parseChatScratchDirectoryPage({
      scratchGenerationId: "scratch-generation:test",
      relativePath: "reports",
      entries: [{
        relativePath: "reports/result.md",
        displayName: "result.md",
        kind: "file",
        byteSize: 2048,
        contentRevision: digest,
        promotable: true,
      }],
      nextCursor: "cursor:test",
    });

    expect(page.entries[0]?.contentRevision).toBe(digest);
    expect(() => parseChatScratchDirectoryPage({
      ...page,
      entries: [{ ...page.entries[0], relativePath: "../secret.txt" }],
    })).toThrow("must be a bounded relative path");
    expect(() => parseChatScratchDirectoryPage({
      ...page,
      entries: Array.from({ length: 51 }, (_, index) => ({
        ...page.entries[0],
        relativePath: `reports/${index}.md`,
      })),
    })).toThrow("exceeds the bounded page size");
  });

  it("validates exact promotion destinations and content digests", () => {
    const result = parseChatScratchPromotionResult({
      id: "scratch-promotion:test",
      scratchGenerationId: "scratch-generation:test",
      sourceRelativePath: "result.md",
      sourceSha256: digest,
      destination: {
        kind: "workingFolder",
        workingFolderId: "folder:app",
        relativePath: "docs/result.md",
      },
      createdAt: timestamp,
    });

    expect(result.destination).toEqual({
      kind: "workingFolder",
      workingFolderId: "folder:app",
      relativePath: "docs/result.md",
    });
    expect(parseChatScratchPromotionResult({
      ...result,
      destination: {
        kind: "managedAttachment",
        channelId: "channel:general",
        attachmentId: "attachment:result",
      },
    }).destination).toEqual({
      kind: "managedAttachment",
      channelId: "channel:general",
      attachmentId: "attachment:result",
    });
    expect(() => parseChatScratchPromotionResult({
      ...result,
      sourceSha256: "not-a-digest",
    })).toThrow("must be a SHA-256 digest");
  });

  it("keeps cleanup preview and completion revisions explicit", () => {
    const preview = parseChatScratchCleanupPreview({
      scratchScopeId: "scratch-scope:test",
      scratchGenerationId: "scratch-generation:test",
      expectedScopeRevision: 3,
      lifecycleState: "active",
      byteSize: 4096,
      entryCount: 6,
      sizeTruncated: false,
      deviceAvailability: "available",
      activeRunCount: 0,
      willRemoveScope: true,
    });
    const result = parseChatScratchCleanupResult({
      jobId: "scratch-cleanup:test",
      scratchScopeId: preview.scratchScopeId,
      scratchGenerationId: preview.scratchGenerationId,
      scopeRevision: 4,
      state: "completed",
      removedBytes: preview.byteSize,
      deviceAvailability: "unavailableOnThisDevice",
      completedAt: timestamp,
    });

    expect(result.scopeRevision).toBe(preview.expectedScopeRevision + 1);
    expect(result.state).toBe("completed");
    expect(() => parseChatScratchCleanupPreview({
      ...preview,
      activeRunCount: -1,
    })).toThrow("must not be negative");
  });

  it("rejects retained source ranges that move backwards", () => {
    const scope = scratchScopeFixture();
    const generations = scope.generations as Array<Record<string, unknown>>;
    const sources = generations[0]?.retainedSources as Array<Record<string, unknown>>;
    if (sources[0]) sources[0].highOrderKey = "c003";

    expect(() => parseChatScratchScope(scope)).toThrow(
      "highOrderKey must not precede lowerOrderKey",
    );
  });
});
