import { describe, expect, it } from "vitest";
import {
  defaultChatVaultConfig,
  parseChatConfigRoot,
  parseChatChannel,
  parseChatChannelPage,
  parsePostChatMessageResult,
  parseChatMessageSearchResults,
  parseChatScheduledMessage,
  parseChatScheduledMessageDispatch,
  parseChatVaultConfig,
  parseCanonicalRuntimeEvent,
  parseCanonicalStoredEvent,
  parseChatError,
  parseChatDiagnosticsRead,
  parseChatTimelinePage,
  parseChatThreadShell,
  parseProjectWorkingFolderRead,
  parseProviderFamilyMetadata,
  parseProviderInstanceConfig,
  parseProviderInstanceRead,
  parseProviderModelCatalog,
  parseProviderRefreshResult,
  parseProviderFiles,
} from "./validation";

const timestamp = "2026-07-20T12:00:00Z";

function metadataFixture(): Record<string, unknown> {
  return {
    familyId: "codex",
    displayName: "Codex",
    configurationSchemaVersion: 1,
    supportedPlatforms: ["linux", "windows", "macos"],
    minimumTestedCliVersion: null,
    defaultExecutableCandidates: ["codex"],
    implementationStatus: "metadata_only",
    maturity: "experimental",
    protocolName: "unavailable",
    potentialCapabilities: ["native_resume", "approvals", "structured_questions"],
    unavailableReason: "This provider driver has not been implemented yet.",
  };
}

function runtimeEventFixture(): Record<string, unknown> {
  return {
    schemaVersion: 1,
    eventId: "event-1",
    providerFamilyId: "codex",
    providerInstanceId: "codex-personal",
    threadId: "thread-1",
    createdAt: timestamp,
    turnId: "turn-1",
    providerTurnId: "provider-turn-1",
    providerItemId: "provider-item-1",
    providerRequestId: null,
    providerTaskId: null,
    providerReference: { schemaVersion: 1, value: { sequence: 4 } },
    event: {
      type: "content_delta",
      payload: {
        itemId: "provider-item-1",
        streamKind: "assistant_text",
        contentIndex: 0,
        delta: "Hello",
      },
    },
    redactedDiagnostic: null,
  };
}

describe("Chat provider contracts", () => {
  it("validates organizational channel summaries without execution targets", () => {
    const channel = parseChatChannel({
      id: "channel:general",
      conversationId: "conversation:general",
      projectId: "project-1",
      name: "general",
      topic: "Project coordination",
      isDefault: true,
      memberships: [],
      messageCount: 18,
      unreadCount: 2,
      latestPreview: "Review is ready",
      lastActivityAt: timestamp,
      attentionState: "ready_for_review",
      revision: 4,
      archivedAt: null,
      createdAt: timestamp,
      updatedAt: timestamp,
    });

    expect(channel.conversationId).toBe("conversation:general");
    expect(channel.attentionState).toBe("ready_for_review");
    expect(() => parseChatChannel({ ...channel, unreadCount: -1 }))
      .toThrow("unreadCount must not be negative");
  });

  it("removes absolute duplicate file summaries from stored timeline turns", () => {
    const relative = {
      relativePath: "hello.py",
      previousRelativePath: null,
      additions: 10,
      deletions: 0,
      binary: false,
      status: "added",
    };
    const page = parseChatTimelinePage({
      threadId: "thread-1",
      items: [],
      turns: [{
        turnId: "turn-1",
        state: "completed",
        startedAt: timestamp,
        completedAt: timestamp,
        stopReason: null,
        modelId: null,
        modelOptions: [],
        modes: { safetyMode: "ask_for_approval", interactionMode: "build" },
        usage: null,
        changedFiles: [
          { ...relative, relativePath: "/home/user/workspace/hello.py", additions: 0 },
          relative,
        ],
      }],
      previousCursor: null,
      nextCursor: null,
      threadRevision: 1,
    });

    expect(page.turns[0]?.changedFiles).toEqual([relative]);
  });

  it("validates one organizational channel cursor page", () => {
    const page = parseChatChannelPage({
      channelId: "channel:general",
      messages: [{
        itemId: "message-1",
        conversationId: "conversation:general",
        replyThreadId: null,
        revisionId: "revision-1",
        revision: 1,
        author: {
          id: "participant:local-owner",
          kind: "local_user",
          displayName: "You",
          avatar: { schemaVersion: 1, value: {} },
          revision: 1,
          archivedAt: null,
        },
        authorLabelSnapshot: "You",
        normalizedMarkdown: "Done",
        richContent: { schemaVersion: 1, value: {} },
        attachmentIds: [],
        references: [],
        replyThread: null,
        ordinal: 1,
        editedAt: null,
        createdAt: timestamp,
      }],
      previousCursor: "1",
      revision: 8,
    });

    expect(page.messages[0]?.normalizedMarkdown).toBe("Done");
    expect(parsePostChatMessageResult({
      message: page.messages[0],
      replyThreadId: null,
      assignment: null,
      assignmentInputQueued: false,
    }).replyThreadId).toBeNull();
    expect(() => parseChatChannelPage({ ...page, revision: -1 }))
      .toThrow("revision must not be negative");

    const referenced = parseChatChannelPage({
      ...page,
      messages: [{
        ...page.messages[0]!,
        normalizedMarkdown: "Ask @Ágata",
        references: [{
          kind: "participant",
          metadata: {
            referenceId: "reference:agata",
            labelSnapshot: "Ágata",
            startOffset: 4,
            endOffset: 11,
            plainTextProjection: "@Ágata",
          },
          participantId: "participant:agata",
          participantKind: "ai_teammate",
        }],
      }],
    });
    expect(referenced.messages[0]?.references[0]?.metadata.endOffset).toBe(11);
    expect(() => parseChatChannelPage({
      ...referenced,
      messages: [{
        ...referenced.messages[0]!,
        references: [{
          ...referenced.messages[0]!.references[0]!,
          metadata: {
            ...referenced.messages[0]!.references[0]!.metadata,
            endOffset: 10,
          },
        }],
      }],
    })).toThrow("invalid, overlapping, or duplicate reference");
  });

  it("retains stable author identity in organizational message search results", () => {
    const result = {
      projectId: "project-1",
      channelId: "channel:general",
      channelName: "general",
      conversationId: "conversation:general",
      replyThreadId: null,
      messageItemId: "message-1",
      ordinal: 1,
      authorParticipantId: "participant:local-owner",
      authorKind: "local_user",
      authorDisplayName: "You",
      excerpt: "Done",
      createdAt: timestamp,
    };

    expect(parseChatMessageSearchResults([result])).toEqual([result]);
  });

  it("validates scheduled messages and dispatch refresh metadata", () => {
    const scheduled = {
      id: "scheduled-message-1",
      channelId: "channel:general",
      replyThreadId: null,
      normalizedMarkdown: "Post the update",
      richContent: { schemaVersion: 1, value: {} },
      attachmentIds: [],
      references: [],
      alsoSendToChannel: false,
      state: "scheduled",
      scheduledFor: timestamp,
      lastError: null,
      createdAt: timestamp,
    };
    expect(parseChatScheduledMessage(scheduled)).toEqual(scheduled);
    expect(parseChatScheduledMessageDispatch({
      processedCount: 2,
      dispatchedCount: 1,
      dispatchedChannelIds: ["channel:general"],
      nextDispatchAt: timestamp,
    })).toEqual({
      processedCount: 2,
      dispatchedCount: 1,
      dispatchedChannelIds: ["channel:general"],
      nextDispatchAt: timestamp,
    });
    expect(() => parseChatScheduledMessage({ ...scheduled, state: "lost" }))
      .toThrow("state has an unsupported value");
  });

  it("parses metadata-only provider registry entries", () => {
    expect(parseProviderFamilyMetadata(metadataFixture())).toEqual(metadataFixture());
  });

  it("validates provider discovery refresh counts", () => {
    const fixture = {
      familiesScanned: 4,
      providersChecked: 2,
      providersDiscovered: 1,
      issues: 1,
    };

    expect(parseProviderRefreshResult(fixture)).toEqual(fixture);
    expect(() => parseProviderRefreshResult({ ...fixture, issues: -1 })).toThrow(
      "issues must not be negative",
    );
  });

  it("validates bounded provider file DTOs", () => {
    const fixture = [{
      fileId: "configuration",
      name: "config.toml",
      kind: "configuration",
      format: "toml",
      path: "/home/user/.codex/config.toml",
      exists: true,
      contents: "sandbox_mode = \"workspace-write\"\n",
      revision: "revision-1",
    }];

    expect(parseProviderFiles(fixture)).toEqual(fixture);
    expect(() => parseProviderFiles([{ ...fixture[0], format: "yaml" }])).toThrow(
      "format has an unsupported value",
    );
    expect(() => parseProviderFiles([{ ...fixture[0], exists: "yes" }])).toThrow(
      "exists must be a boolean",
    );
  });

  it("validates internal checkpoint restore audit events", () => {
    const fixture = runtimeEventFixture();
    fixture.turnId = null;
    fixture.event = {
      type: "thread_reverted",
      payload: {
        checkpointId: "checkpoint:1",
        revertedTurnIds: ["turn:2"],
        providerHistoryAction: "fork_required",
      },
    };
    expect(parseCanonicalRuntimeEvent(fixture).event).toEqual(fixture.event);
  });

  it("preserves unknown provider instance fields and versioned configuration", () => {
    const fixture = {
      schemaVersion: 1,
      instanceId: "future-instance",
      familyId: "future-provider",
      label: "Future provider",
      enabled: true,
      executable: "future-agent",
      providerHome: null,
      launchArguments: ["serve"],
      environment: { SAFE_MODE: "1" },
      credentialReferences: { API_TOKEN: "credential-1" },
      visibleModelIds: ["model-1"],
      favoriteModelIds: [],
      providerConfig: {
        schemaVersion: 7,
        value: { futureOption: [true, 4, "value"] },
      },
      futureCommonField: { retained: true },
    };

    expect(parseProviderInstanceConfig(fixture)).toEqual(fixture);
  });

  it("parses typed model options and provider capability data", () => {
    const fixture = {
      instanceId: "codex-personal",
      models: [{
        id: "gpt-5-codex",
        displayName: "GPT-5 Codex",
        description: null,
        contextLimit: 200_000,
        availability: "available",
        capabilities: ["images", "context_usage"],
        options: [{
          kind: "choice",
          key: "reasoning_effort",
          label: "Reasoning effort",
          description: null,
          options: [{ value: "high", label: "High", description: null }],
          defaultValue: "high",
        }],
        custom: false,
      }],
      source: "provider",
      discoveredAt: timestamp,
      stale: false,
    };

    expect(parseProviderModelCatalog(fixture)).toEqual(fixture);
  });

  it("removes deprecated models from validated catalogs", () => {
    const parsed = parseProviderModelCatalog({
      instanceId: "codex-personal",
      models: [{
        id: "gpt-old",
        displayName: "GPT Old",
        description: null,
        contextLimit: null,
        availability: "deprecated",
        capabilities: [],
        options: [],
        custom: false,
      }],
      source: "provider",
      discoveredAt: timestamp,
      stale: false,
    });

    expect(parsed.models).toEqual([]);
  });

  it.each([false, true])("preserves native Claude routes and capabilities with stale=%s", (stale) => {
    const catalog = {
      instanceId: "claude",
      models: [
        {
          id: "default",
          displayName: "Default (Opus 4.8)",
          description: "Use the default model (currently Opus 4.8)",
          contextLimit: null,
          availability: "available",
          capabilities: [],
          options: [{
            kind: "boolean",
            key: "fastMode",
            label: "Fast mode",
            description: null,
            defaultValue: false,
          }],
          custom: false,
        },
        {
          id: "opus",
          displayName: "Opus 4.8",
          description: null,
          contextLimit: null,
          availability: "available",
          capabilities: [],
          options: [],
          custom: false,
        },
        {
          id: "claude-opus-9",
          displayName: "Opus 4.8",
          description: null,
          contextLimit: null,
          availability: "available",
          capabilities: [],
          options: [],
          custom: true,
        },
        {
          id: "custom-unicode",
          displayName: "Default (研究用 Opus 9)",
          description: "Opus 9",
          contextLimit: null,
          availability: "unknown",
          capabilities: [],
          options: [],
          custom: true,
        },
      ],
      source: "provider",
      discoveredAt: timestamp,
      stale,
    };
    const provider = parseProviderInstanceRead({
      configuration: {
        schemaVersion: 1,
        instanceId: "claude",
        familyId: "claude",
        label: "Claude",
        enabled: true,
        executable: "claude",
        providerHome: null,
        launchArguments: [],
        environment: {},
        credentialReferences: {},
        visibleModelIds: [],
        favoriteModelIds: [],
        providerConfig: { schemaVersion: 1, value: {} },
      },
      lastProbe: null,
      lastSuccessfulProbeAt: null,
      modelCatalog: catalog,
    });

    expect(provider.modelCatalog).toEqual(catalog);
  });

  it("rejects partial metadata and unsupported capability values", () => {
    const missing = metadataFixture();
    delete missing.displayName;
    expect(() => parseProviderFamilyMetadata(missing)).toThrow("displayName must be a string");

    expect(() => parseProviderFamilyMetadata({
      ...metadataFixture(),
      potentialCapabilities: ["silent_auto_approval"],
    })).toThrow("potentialCapabilities[0] has an unsupported value");
  });

  it("rejects unsafe numeric model data", () => {
    expect(() => parseProviderModelCatalog({
      instanceId: "codex-personal",
      models: [{
        id: "model-1",
        displayName: "Model",
        description: null,
        contextLimit: Number.MAX_SAFE_INTEGER + 1,
        availability: "available",
        capabilities: [],
        options: [],
        custom: false,
      }],
      source: "provider",
      discoveredAt: timestamp,
      stale: false,
    })).toThrow("contextLimit must be a safe integer");
  });
});

describe("Chat vault configuration", () => {
  it("uses explicit safe defaults when the Chat branch is absent", () => {
    const defaults = defaultChatVaultConfig();
    expect(parseChatConfigRoot({ language: "en" })).toEqual(defaults);
    expect(() => parseChatConfigRoot({ chat: {} })).toThrow("chat.schemaVersion");
    expect(defaults.panels).toEqual({ inspectorWidthPx: 520 });
  });

  it("preserves portable unknown fields while validating known fields", () => {
    const fixture = {
      schemaVersion: 1,
      providers: [{
        schemaVersion: 1,
        instanceId: "codex-personal",
        familyId: "codex",
        label: "Personal Codex",
        enabled: true,
        launchArguments: [],
        environment: {},
        credentialReferences: { API_TOKEN: "credential-1" },
        visibleModelIds: ["gpt-5-codex"],
        favoriteModelIds: ["gpt-5-codex"],
        providerConfig: { schemaVersion: 1, value: {} },
        futurePortableOption: { enabled: true },
      }],
      automaticProviderSetupDisabled: [],
      rememberedSelections: [{
        workingFolderId: "workspace-1",
        providerInstanceId: "codex-personal",
        modelId: "gpt-5-codex",
        providerManagedModel: false,
        modelOptions: [],
        safetyMode: "ask_for_approval",
        interactionMode: "build",
      }],
      workingFolderProviderPreferences: {},
      panels: { inspectorWidthPx: 420 },
      behavior: {
        sendKey: "enter",
        restoreLastSelectedThread: true,
        showReasoningSummaries: true,
        automaticallyFoldSettledWork: true,
        terminalScrollbackLines: 12_000,
        idleSessionTimeoutSeconds: 900,
        confirmMultilineTerminalPaste: true,
      },
      futureRootOption: "preserved",
    };

    expect(parseChatVaultConfig(fixture)).toEqual(fixture);
  });

  it("requires every current field inside an existing Chat branch", () => {
    const rootFields = [
      "schemaVersion",
      "providers",
      "automaticProviderSetupDisabled",
      "rememberedSelections",
      "workingFolderProviderPreferences",
      "panels",
      "behavior",
    ] as const;
    for (const field of rootFields) {
      const incomplete: Record<string, unknown> = { ...defaultChatVaultConfig() };
      delete incomplete[field];
      expect(() => parseChatVaultConfig(incomplete), field).toThrow();
    }

    const provider: Record<string, unknown> = {
      schemaVersion: 1,
      instanceId: "codex-personal",
      familyId: "codex",
      label: "Personal Codex",
      enabled: true,
      launchArguments: [],
      environment: {},
      credentialReferences: {},
      visibleModelIds: [],
      favoriteModelIds: [],
      providerConfig: { schemaVersion: 1, value: {} },
    };
    for (const field of [
      "enabled",
      "launchArguments",
      "environment",
      "credentialReferences",
      "visibleModelIds",
      "favoriteModelIds",
    ] as const) {
      const incomplete = { ...provider };
      delete incomplete[field];
      expect(() => parseChatVaultConfig({
        ...defaultChatVaultConfig(),
        providers: [incomplete],
      }), `provider.${field}`).toThrow();
    }

    const selection: Record<string, unknown> = {
      workingFolderId: "workspace-1",
      providerInstanceId: "codex-personal",
      modelId: "gpt-5-codex",
      providerManagedModel: false,
      modelOptions: [],
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    };
    for (const field of ["providerManagedModel", "modelOptions"] as const) {
      const incomplete = { ...selection };
      delete incomplete[field];
      expect(() => parseChatVaultConfig({
        ...defaultChatVaultConfig(),
        rememberedSelections: [incomplete],
      }), `selection.${field}`).toThrow();
    }

    expect(() => parseChatVaultConfig({
      ...defaultChatVaultConfig(),
      panels: {},
    })).toThrow("inspectorWidthPx");

    for (const field of Object.keys(defaultChatVaultConfig().behavior)) {
      const behavior: Record<string, unknown> = { ...defaultChatVaultConfig().behavior };
      delete behavior[field];
      expect(() => parseChatVaultConfig({
        ...defaultChatVaultConfig(),
        behavior,
      }), `behavior.${field}`).toThrow();
    }
  });

  it("rejects machine-specific paths and implicit model selection", () => {
    expect(() => parseChatVaultConfig({
      ...defaultChatVaultConfig(),
      providers: [{
        schemaVersion: 1,
        instanceId: "codex-personal",
        familyId: "codex",
        label: "Personal Codex",
        enabled: true,
        executable: "/usr/bin/codex",
        launchArguments: [],
        environment: {},
        credentialReferences: {},
        visibleModelIds: [],
        favoriteModelIds: [],
        providerConfig: { schemaVersion: 1, value: {} },
      }],
    })).toThrow("contains machine-specific field executable");

    expect(() => parseChatVaultConfig({
      ...defaultChatVaultConfig(),
      rememberedSelections: [{
        workingFolderId: "workspace-1",
        providerInstanceId: "codex-personal",
        modelId: null,
        providerManagedModel: false,
        modelOptions: [],
        safetyMode: "ask_for_approval",
        interactionMode: "build",
      }],
    })).toThrow("modelId or provider-managed model state is required");
  });
});

describe("project working-folder contracts", () => {
  it("parses portable folder identity separately from its device binding", () => {
    const fixture = {
      workingFolder: {
        id: "workspace-1",
        projectId: "project-1",
        displayName: "Frontend",
        kind: "external",
        managedRelativePath: null,
        sortOrder: 10,
        repositoryKind: "git",
        repositoryIdentity: "git-sha256:abc123",
        createdAt: timestamp,
        updatedAt: timestamp,
        archivedAt: null,
        revision: 2,
      },
      bindingStatus: "available",
      canonicalPath: "/home/user/project",
      lastVerifiedAt: timestamp,
      currentBranch: "feat/chat",
    };

    expect(parseProjectWorkingFolderRead(fixture)).toEqual(fixture);
  });

  it("rejects unknown binding states and malformed timestamps", () => {
    const fixture = {
      workingFolder: {
        id: "workspace-1",
        projectId: "project-1",
        displayName: "Frontend",
        kind: "external",
        managedRelativePath: null,
        sortOrder: 10,
        repositoryKind: "none",
        repositoryIdentity: null,
        createdAt: timestamp,
        updatedAt: timestamp,
        archivedAt: null,
        revision: 1,
      },
      bindingStatus: "trusted_forever",
      canonicalPath: null,
      lastVerifiedAt: null,
      currentBranch: null,
    };
    expect(() => parseProjectWorkingFolderRead(fixture)).toThrow("bindingStatus has an unsupported value");
    expect(() => parseProjectWorkingFolderRead({
      ...fixture,
      bindingStatus: "unbound",
      workingFolder: { ...fixture.workingFolder, updatedAt: "tomorrow" },
    })).toThrow("updatedAt must be an RFC 3339 UTC timestamp");
  });
});

describe("Chat event contracts", () => {
  it("parses a canonical content delta envelope", () => {
    expect(parseCanonicalRuntimeEvent(runtimeEventFixture())).toEqual(runtimeEventFixture());
  });

  it("parses the explicit bounded unknown-event fallback", () => {
    const fixture = runtimeEventFixture();
    fixture.event = {
      type: "unknown",
      payload: {
        sourceType: "provider.future_event",
        summary: "A future provider event was received.",
        safePayload: { schemaVersion: 1, value: { safeType: "future_event" } },
      },
    };

    expect(parseCanonicalRuntimeEvent(fixture)).toEqual(fixture);
  });

  it("rejects unknown canonical event names instead of trusting their payload", () => {
    const fixture = runtimeEventFixture();
    fixture.event = { type: "provider_raw_event", payload: { secret: "must not cross" } };
    expect(() => parseCanonicalRuntimeEvent(fixture)).toThrow("event.type has an unsupported value");
  });

  it("rejects malformed nested event payloads", () => {
    const fixture = runtimeEventFixture();
    fixture.event = {
      type: "content_delta",
      payload: {
        itemId: "provider-item-1",
        streamKind: "assistant_text",
        contentIndex: -1,
        delta: "Hello",
      },
    };
    expect(() => parseCanonicalRuntimeEvent(fixture)).toThrow("contentIndex must not be negative");
  });

  it("validates flattened stored-event sequence fields", () => {
    const fixture = { ...runtimeEventFixture(), sequence: 5, ingestedAt: timestamp };
    expect(parseCanonicalStoredEvent(fixture)).toEqual(fixture);
    expect(() => parseCanonicalStoredEvent({ ...fixture, sequence: 1.5 })).toThrow("sequence must be a safe integer");
  });

  it("rejects invalid UTC timestamps and provider identifiers", () => {
    expect(() => parseCanonicalRuntimeEvent({
      ...runtimeEventFixture(),
      createdAt: "2026-07-20T07:00:00-05:00",
    })).toThrow("createdAt must be an RFC 3339 UTC timestamp");

    expect(() => parseCanonicalRuntimeEvent({
      ...runtimeEventFixture(),
      providerFamilyId: "codex\u0000hidden",
    })).toThrow("providerFamilyId contains a control character");
  });
});

describe("Chat read and error contracts", () => {
  it("parses bounded Chat diagnostics and rejects invalid retention", () => {
    const fixture = {
      preferences: { captureEnabled: false, retentionDays: 7 },
      capturedFields: ["event type"], excludedFields: ["credentials"], storageLocation: "SQLite",
      projectionHealthy: true, inconsistentProjectionCount: 0, credentialStoreAvailable: true,
      healthyProviderProbes: 1, unhealthyProviderProbes: 0, unknownProviderProbes: 1,
      liveProviderProcesses: 1, activeTurns: 0, liveTerminals: 0,
      counts: {
        retainedEvents: 0, retainedBytes: 0, attachmentCount: 1, attachmentBytes: 12,
        pendingAttachmentCleanup: 0, failedAttachmentCleanup: 0, commandOutputEvents: 2,
        commandOutputBytes: 20, checkpointFailures: 0, pendingCheckpointCleanup: 0,
        failedCheckpointCleanup: 0,
      },
    };
    expect(parseChatDiagnosticsRead(fixture)).toEqual(fixture);
    expect(() => parseChatDiagnosticsRead({ ...fixture, preferences: { captureEnabled: true, retentionDays: 31 } })).toThrow("between 1 and 30");
  });

  it("parses lightweight thread shells without message history", () => {
    const fixture = {
      id: "thread-1",
      workingFolderId: "workspace-1",
      executionEnvironmentId: "current-folder:workspace-1",
      scratchGenerationId: null,
      projectId: "project-1",
      title: "Implement Chat contracts",
      providerFamilyId: "codex",
      providerInstanceId: "codex-personal",
      providerThreadId: null,
      modelId: "gpt-5-codex",
      modelOptions: [],
      modes: { safetyMode: "ask_for_approval", interactionMode: "build" },
      state: "idle",
      latestTurnState: "completed",
      latestPreview: "Contracts compile.",
      messageCount: 2,
      revision: 4,
      lastEventSequence: 12,
      lastActivityAt: timestamp,
      unreadAt: null,
      archivedAt: null,
    };
    expect(parseChatThreadShell(fixture)).toEqual(fixture);
    const scratch = {
      ...fixture,
      id: "thread-scratch",
      workingFolderId: null,
      executionEnvironmentId: "scratch-environment:1",
      scratchGenerationId: "scratch-generation:1",
    };
    expect(parseChatThreadShell(scratch)).toEqual(scratch);
    expect(() => parseChatThreadShell({
      ...scratch,
      workingFolderId: "workspace-1",
    })).toThrow("exactly one working folder or scratch generation");
  });

  it("parses structured errors and rejects unbounded unknown detail types", () => {
    const fixture = {
      code: "driver_unavailable",
      message: "The provider driver is not implemented.",
      field: null,
      recoverable: true,
      details: { providerFamilyId: "codex" },
    };
    expect(parseChatError(fixture)).toEqual(fixture);
    expect(() => parseChatError({ ...fixture, details: undefined })).toThrow("details is not valid JSON");
  });
});
