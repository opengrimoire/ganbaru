import { describe, expect, it } from "vitest";
import utcTimestamps from "../../../test-fixtures/utc-timestamps.json";
import {
  defaultChatConfiguration,
  parseChatAgentRun,
  parseChatChannel,
  parseChatChannelPage,
  parseChatChannelNote,
  parseChatChannelNotes,
  parseChatNoteProposalStates,
  parseChatPushApprovals,
  parseChatPushRequest,
  parseChatPushRequestStates,
  parsePostChatMessageResult,
  parseChatMessageSearchResults,
  parseChatScheduledMessage,
  parseChatScheduledMessageDispatch,
  parseChatQuietPeriodPreferences,
  parseChatQuietState,
  parseChatRecurrenceSpec,
  parseChatScheduleProposal,
  parseChatScheduleProposalStates,
  parseChatAppAgentSettings,
  parseChatChannelAgentSettings,
  parseChatProjectAgentSettings,
  parseChatConfiguration,
  parseCanonicalRuntimeEvent,
  parseCanonicalStoredEvent,
  parseChatError,
  parseChatDiagnosticsRead,
  parseChatTimelinePage,
  parseChatThreadShell,
  parseProjectWorkingFolderRead,
  parseProviderFamilyMetadata,
  parseProviderUpdateRunRead,
  parseProviderUpdates,
  parseProviderInstanceConfig,
  parseProviderInstanceRead,
  parseProviderModelCatalog,
  parseProviderProbeResult,
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
    maximumTestedCliVersion: null,
    updateCommand: null,
    installGuide: null,
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
      unreadReplyCount: 1,
      unseenFailure: true,
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
    expect(channel.unreadReplyCount).toBe(1);
    expect(channel.unseenFailure).toBe(true);
    expect(() => parseChatChannel({ ...channel, unreadCount: -1 }))
      .toThrow("unreadCount must not be negative");
    expect(() => parseChatChannel({ ...channel, unseenFailure: "yes" })).toThrow("unseenFailure");
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
        errorCode: null,
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
      changeCursor: 0,
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
        orderKey: "c001",
        awaitingDelivery: false,
        editedAt: null,
        createdAt: timestamp,
      }],
      previousCursor: "before:c001:message-1",
      revision: 8,
    });

    expect(page.messages[0]?.normalizedMarkdown).toBe("Done");
    expect(parsePostChatMessageResult({
      message: page.messages[0],
      replyThreadId: null,
      assignment: null,
      assignmentInputQueued: false,
      routedAs: null,
    }).replyThreadId).toBeNull();
    const queuedResult = {
      message: page.messages[0],
      replyThreadId: null,
      assignment: null,
      assignmentInputQueued: true,
      routedAs: "queue",
    };
    expect(parsePostChatMessageResult(queuedResult).routedAs).toBe("queue");
    expect(() => parsePostChatMessageResult({ ...queuedResult, routedAs: "later" }))
      .toThrow("routedAs");
    expect(() => parsePostChatMessageResult({ ...queuedResult, routedAs: undefined }))
      .toThrow("routedAs");
    expect(() => parseChatChannelPage({ ...page, revision: -1 }))
      .toThrow("revision must not be negative");
    for (const orderKey of ["", "c00:1", "c".repeat(129), 1]) {
      expect(() => parseChatChannelPage({
        ...page,
        messages: [{ ...page.messages[0]!, orderKey }],
      })).toThrow("orderKey must");
    }
    const unattributed = parseChatChannelPage({
      ...page,
      messages: [{ ...page.messages[0]!, author: null, authorLabelSnapshot: "Former member" }],
    });
    expect(unattributed.messages[0]?.author).toBeNull();
    expect(() => parseChatChannelPage({
      ...page,
      messages: [{ ...page.messages[0]!, author: "participant:local-owner" }],
    })).toThrow("author");

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
          participantKind: "ai_agent",
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
      anchorCursor: "through:c001:message-1",
      authorParticipantId: "participant:local-owner",
      authorKind: "local_user",
      authorDisplayName: "You",
      excerpt: "Done",
      createdAt: timestamp,
    };

    expect(parseChatMessageSearchResults([result])).toEqual([result]);

    const unattributed = {
      ...result,
      authorParticipantId: null,
      authorKind: null,
      authorDisplayName: "Former member",
    };
    expect(parseChatMessageSearchResults([unattributed])).toEqual([unattributed]);
    expect(() => parseChatMessageSearchResults([{ ...result, authorKind: null }])).toThrow("authorKind");
    expect(() => parseChatMessageSearchResults([{ ...result, authorParticipantId: null }])).toThrow("authorKind");
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
      recurrence: null,
    };
    expect(parseChatScheduledMessage(scheduled)).toEqual(scheduled);
    const repeating = {
      ...scheduled,
      recurrence: {
        rule: { kind: "fixed_time", localMinute: 480, weekdays: 31 },
        timeZone: "Europe/Madrid",
        quietPolicy: "defer",
        nextOccurrenceAt: timestamp,
        firedCount: 3,
        skippedOccurrences: 1,
        deferredUntil: null,
      },
    };
    expect(parseChatScheduledMessage(repeating)).toEqual(repeating);
    expect(() => parseChatScheduledMessage({ ...scheduled, recurrence: undefined })).toThrow("recurrence");
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
      preferences: null,
    }];

    expect(parseProviderFiles({ files: fixture, notes: [] })).toEqual({ files: fixture, notes: [] });
    expect(() => parseProviderFiles({ files: [{ ...fixture[0], format: "yaml" }], notes: [] })).toThrow(
      "format has an unsupported value",
    );
    expect(() => parseProviderFiles({ files: [{ ...fixture[0], exists: "yes" }], notes: [] })).toThrow(
      "exists must be a boolean",
    );
    const notes = [{ kind: "codex_override", paths: ["~/.codex/AGENTS.override.md"] }];
    expect(parseProviderFiles({ files: fixture, notes }).notes).toEqual(notes);
    expect(() => parseProviderFiles({ files: fixture, notes: [{ kind: "future_source", paths: [] }] })).toThrow(
      "kind has an unsupported value",
    );
    expect(() => parseProviderFiles({ files: fixture, notes: [{ kind: "claude_rules", paths: [1] }] })).toThrow(
      "paths[0] must be a string",
    );
    expect(() => parseProviderFiles(fixture)).toThrow("provider files must be an object");
  });

  it("validates typed agent preferences on provider files", () => {
    const entry = {
      id: "CLAUDE_CODE_DISABLE_1M_CONTEXT",
      valueType: "switch",
      variable: true,
      value: true,
      ignored: false,
      maxBytes: null,
      multiline: false,
      emptyAllowed: false,
      options: [] as string[],
    };
    const fixture = [{
      fileId: "configuration",
      name: "settings.json",
      kind: "configuration",
      format: "json",
      path: "/home/user/.claude/settings.json",
      exists: true,
      contents: "{}\n",
      revision: "revision-1",
      preferences: { editable: true, entries: [entry, { ...entry, id: "language", valueType: "text", variable: false, value: null, maxBytes: 64 }] },
    }];

    expect(parseProviderFiles({ files: fixture, notes: [] })).toEqual({ files: fixture, notes: [] });
    expect(() => parseProviderFiles({ files: [{ ...fixture[0], preferences: { editable: true, entries: [{ ...entry, value: "1" }] } }], notes: [] })).toThrow(
      "value must be a boolean",
    );
    expect(() => parseProviderFiles({ files: [{ ...fixture[0], preferences: { editable: true, entries: [{ ...entry, valueType: "list" }] } }], notes: [] })).toThrow(
      "valueType has an unsupported value",
    );
  });

  it("keeps choice preferences to the options they list", () => {
    const choice = {
      id: "web_search",
      valueType: "choice",
      variable: false,
      value: "live",
      ignored: false,
      maxBytes: null,
      multiline: false,
      emptyAllowed: false,
      options: ["disabled", "cached", "live"],
    };
    const files = (entry: Record<string, unknown>) => ({
      files: [{
        fileId: "configuration",
        name: "config.toml",
        kind: "configuration",
        format: "toml",
        path: "~/.codex/config.toml",
        exists: true,
        contents: "web_search = \"live\"\n",
        revision: "revision-1",
        preferences: { editable: true, entries: [entry] },
      }],
      notes: [],
    });

    expect(parseProviderFiles(files(choice)).files[0].preferences?.entries[0]).toEqual(choice);
    expect(parseProviderFiles(files({ ...choice, value: null })).files[0].preferences?.entries[0].value).toBeNull();
    expect(() => parseProviderFiles(files({ ...choice, value: "indexed" }))).toThrow("must be one of the options");
    expect(() => parseProviderFiles(files({ ...choice, options: [] }))).toThrow("options must list the choices");
    expect(() => parseProviderFiles(files({ ...choice, options: [1] }))).toThrow("options[0] must be a string");
    const { options: _options, ...withoutOptions } = choice;
    expect(() => parseProviderFiles(files(withoutOptions))).toThrow("options must be an array");
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
      recommendedModelId: null,
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
      recommendedModelId: "gpt-old",
      source: "provider",
      discoveredAt: timestamp,
      stale: false,
    });

    expect(parsed.models).toEqual([]);
    expect(parsed.recommendedModelId).toBeNull();
  });

  it.each([false, true])("preserves native Claude aliases and capabilities with stale=%s", (stale) => {
    const catalog = {
      instanceId: "claude",
      models: [
        {
          id: "opus",
          displayName: "Opus 4.8",
          description: "Opus 4.8 · Most capable for complex work",
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
      recommendedModelId: "opus",
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
    expect(provider.runtimeReview).toEqual([]);

    const reviewed = parseProviderInstanceRead({
      ...provider,
      runtimeReview: [{ label: "Environment: NODE_OPTIONS=--require ./x.js", widensPermissions: true }],
    });
    expect(reviewed.runtimeReview).toEqual([
      { label: "Environment: NODE_OPTIONS=--require ./x.js", widensPermissions: true },
    ]);
    expect(() => parseProviderInstanceRead({
      ...provider,
      runtimeReview: [{ label: 3, widensPermissions: true }],
    })).toThrow();
    expect(provider).toMatchObject({ usageLimits: null, lastUsedAt: null, duplicateOf: null });
    expect(parseProviderInstanceRead({ ...provider, duplicateOf: "claude-default" }).duplicateOf).toBe("claude-default");
    expect(() => parseProviderInstanceRead({ ...provider, duplicateOf: 3 })).toThrow();
    const observed = parseProviderInstanceRead({
      ...provider,
      usageLimits: {
        observedAt: timestamp,
        status: { limited: false, resetsAt: null, detail: null, providerData: null },
      },
      lastUsedAt: timestamp,
    });
    expect(observed.usageLimits?.observedAt).toBe(timestamp);
    expect(observed.lastUsedAt).toBe(timestamp);
    expect(() => parseProviderInstanceRead({
      ...provider,
      usageLimits: { observedAt: "yesterday", status: { limited: false, resetsAt: null, detail: null, providerData: null } },
    })).toThrow();
  });

  it("parses a probe's version admission and refuses unknown admissions", () => {
    const probe = {
      instanceId: "claude",
      state: "healthy",
      version: "2.1.400",
      negotiatedProtocolVersion: "stream-json",
      accountLabel: null,
      authSource: "api_key",
      capabilities: { entries: [] },
      authoritySupport: {
        isolatedConversation: true,
        internalHostTools: true,
        denyShell: false,
        readOnlyRoot: false,
        writableRoot: false,
        confinedCommands: false,
        networkBoundary: false,
        classifiedPublish: false,
        personApprovedCommands: false,
      },
      checkedAt: timestamp,
      detail: "Newer than the tested version 2.1.322",
      versionAdmission: "untested_newer",
      agentMinimumVersion: null,
    };
    expect(parseProviderProbeResult(probe)).toEqual(probe);
    expect(parseProviderProbeResult({ ...probe, versionAdmission: null }).versionAdmission).toBeNull();
    expect(() => parseProviderProbeResult({ ...probe, versionAdmission: "too_new" }))
      .toThrow("versionAdmission has an unsupported value");
    expect(parseProviderProbeResult({ ...probe, authSource: "account_login" }).authSource).toBe("account_login");
    expect(parseProviderProbeResult({ ...probe, authSource: null }).authSource).toBeNull();
    expect(() => parseProviderProbeResult({ ...probe, authSource: "browser" }))
      .toThrow("authSource has an unsupported value");
    expect(parseProviderProbeResult({ ...probe, agentMinimumVersion: "2.1.325" }).agentMinimumVersion).toBe("2.1.325");
    expect(() => parseProviderProbeResult({ ...probe, agentMinimumVersion: 2 }))
      .toThrow("agentMinimumVersion must be a string");
  });

  it("reads the newest tested CLI version from provider metadata", () => {
    expect(parseProviderFamilyMetadata({ ...metadataFixture(), maximumTestedCliVersion: "0.147.0" }).maximumTestedCliVersion)
      .toBe("0.147.0");
    expect(() => parseProviderFamilyMetadata({ ...metadataFixture(), maximumTestedCliVersion: 147 }))
      .toThrow("maximumTestedCliVersion must be a string");
    expect(parseProviderFamilyMetadata({ ...metadataFixture(), updateCommand: "claude update" }).updateCommand)
      .toBe("claude update");
  });

  it("reads an install guide and refuses a documentation link that is not https", () => {
    const installGuide = { shell: "terminal", command: "curl -fsSL https://claude.ai/install.sh | bash", signInCommand: "claude auth login", docsUrl: "https://code.claude.com/docs/en/setup" };
    expect(parseProviderFamilyMetadata({ ...metadataFixture(), installGuide }).installGuide).toEqual(installGuide);
    expect(() => parseProviderFamilyMetadata({ ...metadataFixture(), installGuide: { ...installGuide, docsUrl: "http://example.com" } }))
      .toThrow("installGuide.docsUrl must use https");
    expect(() => parseProviderFamilyMetadata({ ...metadataFixture(), installGuide: { ...installGuide, command: undefined } }))
      .toThrow("installGuide.command must be a string");
    expect(() => parseProviderFamilyMetadata({ ...metadataFixture(), installGuide: { ...installGuide, shell: "cmd" } }))
      .toThrow("installGuide.shell");
  });

  it("reads provider updates and their run results", () => {
    const update = { instanceId: "codex", currentVersion: "0.167.1", latestVersion: "0.169.0", updateCommand: "codex update", runsInApp: false };
    expect(parseProviderUpdates([update])).toEqual([update]);
    expect(() => parseProviderUpdates([{ ...update, runsInApp: "yes" }])).toThrow("runsInApp must be a boolean");
    expect(() => parseProviderUpdates({})).toThrow();
    expect(parseProviderUpdateRunRead({ successful: false, output: "denied" })).toEqual({ successful: false, output: "denied" });
    expect(() => parseProviderUpdateRunRead({ successful: true })).toThrow("output");
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
      recommendedModelId: null,
      source: "provider",
      discoveredAt: timestamp,
      stale: false,
    })).toThrow("contextLimit must be a safe integer");
  });
});

describe("Chat configuration", () => {
  it("uses explicit safe defaults that parse as a complete read", () => {
    const defaults = defaultChatConfiguration();
    expect(defaults.panels).toEqual({ inspectorWidthPx: 520, replyThreadWidthPx: 440 });
    expect(defaults.behavior.sendKey).toBe("enter");
    expect(defaults.behavior.timelineDensity).toBe("comfortable");
    expect(parseChatConfiguration(defaults)).toEqual(defaults);
  });

  it("refuses a timeline density outside the known choices", () => {
    const defaults = defaultChatConfiguration();
    for (const timelineDensity of ["dense", "Compact", 1]) {
      expect(() => parseChatConfiguration({
        ...defaults,
        behavior: { ...defaults.behavior, timelineDensity },
      }), String(timelineDensity)).toThrow("timelineDensity");
    }
  });

  it("drops retired and unknown top-level fields from a read", () => {
    const parsed = parseChatConfiguration({
      ...defaultChatConfiguration(),
      schemaVersion: 1,
      futureRootOption: "dropped",
    });
    expect(parsed).toEqual(defaultChatConfiguration());
  });

  it("preserves provider unknown fields while validating known fields", () => {
    const fixture = {
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
      panels: { inspectorWidthPx: 420, replyThreadWidthPx: 520 },
      behavior: {
        sendKey: "enter",
        restoreLastSelectedThread: true,
        showReasoningSummaries: true,
        automaticallyFoldSettledWork: true,
        timelineDensity: "compact",
        terminalScrollbackLines: 12_000,
        idleSessionTimeoutSeconds: 1_800,
        confirmMultilineTerminalPaste: true,
        notifyWaitingForYou: true,
        notifyReviewReady: false,
        notifyFailures: true,
        announceBackgroundWork: false,
        notificationIncludeContent: true,
      },
    };

    expect(parseChatConfiguration(fixture)).toEqual(fixture);
  });

  it("defaults notification preferences to notify without content and refuses other shapes", () => {
    const configuration = defaultChatConfiguration();

    expect(configuration.behavior).toMatchObject({
      notifyWaitingForYou: true,
      notifyReviewReady: true,
      notifyFailures: true,
      announceBackgroundWork: true,
      notificationIncludeContent: false,
    });
    expect(() => parseChatConfiguration({
      ...configuration,
      behavior: { ...configuration.behavior, notifyFailures: "false" },
    })).toThrow("notifyFailures");
  });

  it("requires every current field inside an existing Chat branch", () => {
    const rootFields = [
      "providers",
      "automaticProviderSetupDisabled",
      "rememberedSelections",
      "workingFolderProviderPreferences",
      "panels",
      "behavior",
    ] as const;
    for (const field of rootFields) {
      const incomplete: Record<string, unknown> = { ...defaultChatConfiguration() };
      delete incomplete[field];
      expect(() => parseChatConfiguration(incomplete), field).toThrow();
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
      expect(() => parseChatConfiguration({
        ...defaultChatConfiguration(),
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
      expect(() => parseChatConfiguration({
        ...defaultChatConfiguration(),
        rememberedSelections: [incomplete],
      }), `selection.${field}`).toThrow();
    }

    expect(() => parseChatConfiguration({
      ...defaultChatConfiguration(),
      panels: {},
    })).toThrow("inspectorWidthPx");
    expect(() => parseChatConfiguration({
      ...defaultChatConfiguration(),
      panels: { inspectorWidthPx: 520 },
    })).toThrow("replyThreadWidthPx");
    for (const replyThreadWidthPx of [279, 801]) {
      expect(() => parseChatConfiguration({
        ...defaultChatConfiguration(),
        panels: { inspectorWidthPx: 520, replyThreadWidthPx },
      })).toThrow("replyThreadWidthPx is out of range");
    }

    for (const field of Object.keys(defaultChatConfiguration().behavior)) {
      const behavior: Record<string, unknown> = { ...defaultChatConfiguration().behavior };
      delete behavior[field];
      expect(() => parseChatConfiguration({
        ...defaultChatConfiguration(),
        behavior,
      }), `behavior.${field}`).toThrow();
    }
  });

  it("rejects machine-specific paths and implicit model selection", () => {
    expect(() => parseChatConfiguration({
      ...defaultChatConfiguration(),
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

    expect(() => parseChatConfiguration({
      ...defaultChatConfiguration(),
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
      trust: "trusted",
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

  describe("agrees with the Rust timestamp rule on the shared fixture", () => {
    const folderRead = (updatedAt: string) => ({
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
        updatedAt,
        archivedAt: null,
        revision: 1,
      },
      bindingStatus: "unbound",
      canonicalPath: null,
      lastVerifiedAt: null,
      currentBranch: null,
      trust: "untrusted",
    });

    it.each(utcTimestamps.accepted)("accepts %j", (timestamp) => {
      expect(parseProjectWorkingFolderRead(folderRead(timestamp)).workingFolder.updatedAt).toBe(timestamp);
    });

    it.each(utcTimestamps.refused)("refuses %j", (timestamp) => {
      expect(() => parseProjectWorkingFolderRead(folderRead(timestamp)))
        .toThrow("updatedAt must be an RFC 3339 UTC timestamp");
    });
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

  it("parses each agent run attempt reason and rejects unknown or inconsistent ones", () => {
    const fixture = {
      id: "run:1",
      assignmentId: "assignment:1",
      projectId: "project-1",
      workingFolderId: "workspace-1",
      executionEnvironmentId: "current-folder:workspace-1",
      scratchGenerationId: null,
      providerExecutionTurnId: "turn-1",
      providerExecutionThreadId: "thread-1",
      state: "completed",
      runOrdinal: 1,
      attemptReason: "fresh",
      handoffReason: null,
      createdAt: timestamp,
      updatedAt: timestamp,
    };
    for (const attemptReason of ["fresh", "continuation", "retry"]) {
      expect(parseChatAgentRun({ ...fixture, attemptReason })).toEqual({ ...fixture, attemptReason });
    }
    for (const handoffReason of ["key_changed", "authority_revoked", "native_session_lost"]) {
      const handoff = { ...fixture, attemptReason: "handoff", handoffReason };
      expect(parseChatAgentRun(handoff)).toEqual(handoff);
    }
    expect(() => parseChatAgentRun({ ...fixture, attemptReason: "resumed" })).toThrow("attemptReason has an unsupported value");
    expect(() => parseChatAgentRun({ ...fixture, attemptReason: undefined })).toThrow("attemptReason");
    expect(() => parseChatAgentRun({
      ...fixture, attemptReason: "handoff", handoffReason: "provider_changed",
    })).toThrow("handoffReason has an unsupported value");
    expect(() => parseChatAgentRun({ ...fixture, attemptReason: "handoff" })).toThrow("set exactly when");
    expect(() => parseChatAgentRun({ ...fixture, handoffReason: "key_changed" })).toThrow("set exactly when");
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

  it("parses channel agent settings and refuses unknown access levels", () => {
    const settings = {
      channelId: "channel:general",
      conversationId: "conversation:general",
      access: null,
      instructions: "Prefer small commits.",
      effective: { access: "read", source: "project" },
      agents: [
        { agentId: "participant:codex", requested: "read", effective: "read", downgradeReason: null, installed: true, ownOffered: true },
        { agentId: "participant:cursor", requested: "ask", effective: "read", downgradeReason: "cannot_ask_before_commands", installed: true, ownOffered: false },
        { agentId: "participant:claude", requested: "ask", effective: null, downgradeReason: null, installed: false, ownOffered: false },
      ],
      modelOverrides: [{ agentId: "participant:codex", model: { providerManagedModel: false, modelId: "gpt-5.6-sol", modelOptions: [] } }],
      pushApprovals: [],
      revision: 4,
    };

    expect(parseChatChannelAgentSettings(settings)).toEqual(settings);
    expect(parseChatChannelAgentSettings({ ...settings, access: "own" }).access).toBe("own");
    expect(() => parseChatChannelAgentSettings({ ...settings, access: "write" })).toThrow("access");
    expect(() => parseChatChannelAgentSettings({ ...settings, effective: { access: "read", source: "device" } })).toThrow("source");
    expect(() => parseChatChannelAgentSettings({
      ...settings,
      agents: [{ ...settings.agents[0], effective: "write" }],
    })).toThrow("effective");
    expect(() => parseChatChannelAgentSettings({
      ...settings,
      agents: [{ ...settings.agents[1], downgradeReason: "unknown" }],
    })).toThrow("downgradeReason");
  });

  it("fills an absent override model with the provider default", () => {
    const parsed = parseChatChannelAgentSettings({
      channelId: "channel:general",
      conversationId: "conversation:general",
      access: "ask",
      instructions: "",
      effective: { access: "ask", source: "channel" },
      agents: [],
      modelOverrides: [{ agentId: "participant:codex", model: {} }],
      pushApprovals: [],
      revision: 0,
    });

    expect(parsed.modelOverrides[0]?.model).toEqual({ providerManagedModel: false, modelId: null, modelOptions: [] });
  });

  it("requires a project's effective access and an app scope's own access", () => {
    const project = { projectId: "project-1", access: null, instructions: "", effective: { access: "ask", source: "app" }, revision: 0 };
    expect(parseChatProjectAgentSettings(project)).toEqual(project);
    expect(() => parseChatProjectAgentSettings({ ...project, effective: undefined })).toThrow("effective");

    const app = { access: "talk", instructions: "Be brief.", revision: 2, updatedAt: timestamp };
    expect(parseChatAppAgentSettings(app)).toEqual(app);
    expect(() => parseChatAppAgentSettings({ ...app, access: null })).toThrow("access");
  });
});

describe("chat scheduling contracts", () => {
  const interval = { rule: { kind: "interval", minutes: 90 }, timeZone: "UTC", quietPolicy: "ignore" };
  const fixedTime = { rule: { kind: "fixed_time", localMinute: 1_439, weekdays: 127 }, timeZone: "America/Mexico_City", quietPolicy: "defer" };

  it("accepts repeat rules at the backend bounds", () => {
    expect(parseChatRecurrenceSpec(interval)).toEqual(interval);
    expect(parseChatRecurrenceSpec(fixedTime)).toEqual(fixedTime);
    expect(parseChatRecurrenceSpec({ ...interval, rule: { kind: "interval", minutes: 43_200 } }).rule)
      .toEqual({ kind: "interval", minutes: 43_200 });
  });

  it("refuses repeat rules outside the backend bounds", () => {
    expect(() => parseChatRecurrenceSpec({ ...interval, rule: { kind: "interval", minutes: 59 } })).toThrow("minutes");
    expect(() => parseChatRecurrenceSpec({ ...interval, rule: { kind: "interval", minutes: 43_201 } })).toThrow("minutes");
    expect(() => parseChatRecurrenceSpec({ ...fixedTime, rule: { ...fixedTime.rule, localMinute: 1_440 } })).toThrow("localMinute");
    expect(() => parseChatRecurrenceSpec({ ...fixedTime, rule: { ...fixedTime.rule, weekdays: 0 } })).toThrow("weekdays");
    expect(() => parseChatRecurrenceSpec({ ...fixedTime, rule: { ...fixedTime.rule, weekdays: 128 } })).toThrow("weekdays");
    expect(() => parseChatRecurrenceSpec({ ...interval, rule: { kind: "monthly" } })).toThrow("kind");
    expect(() => parseChatRecurrenceSpec({ ...interval, timeZone: " " })).toThrow("timeZone");
    expect(() => parseChatRecurrenceSpec({ ...interval, timeZone: "Z".repeat(65) })).toThrow("timeZone");
    expect(() => parseChatRecurrenceSpec({ ...interval, quietPolicy: "skip" })).toThrow("quietPolicy");
  });

  it("validates quiet windows and their count", () => {
    const window = { weekdays: 31, startMinute: 1_320, endMinute: 420 };
    expect(parseChatQuietPeriodPreferences({ windows: [window] })).toEqual({ windows: [window] });
    expect(() => parseChatQuietPeriodPreferences({ windows: [{ ...window, endMinute: 1_320 }] })).toThrow("another minute");
    expect(() => parseChatQuietPeriodPreferences({ windows: Array.from({ length: 9 }, () => window) })).toThrow("at most 8");
    expect(() => parseChatQuietPeriodPreferences({ windows: [{ ...window, weekdays: 0 }] })).toThrow("weekdays");
  });

  it("requires a quiet state to name its reason exactly when quiet", () => {
    expect(parseChatQuietState({ quiet: false, reason: null, endsAt: null })).toEqual({ quiet: false, reason: null, endsAt: null });
    expect(parseChatQuietState({ quiet: true, reason: "focus", endsAt: timestamp }))
      .toEqual({ quiet: true, reason: "focus", endsAt: timestamp });
    expect(parseChatQuietState({ quiet: true, reason: "window", endsAt: null }).endsAt).toBeNull();
    expect(() => parseChatQuietState({ quiet: true, reason: null, endsAt: null })).toThrow("reason");
    expect(() => parseChatQuietState({ quiet: false, reason: "window", endsAt: null })).toThrow("reason");
    expect(() => parseChatQuietState({ quiet: false, reason: null, endsAt: timestamp })).toThrow("reason");
  });

  it("parses schedule proposals and refuses malformed ones", () => {
    const proposal = { message: " Post the summary ", firstAt: timestamp, recurrence: fixedTime, reason: " You asked ", source: "host_tool" };
    expect(parseChatScheduleProposal(proposal)).toEqual({
      message: "Post the summary",
      firstAt: timestamp,
      recurrence: fixedTime,
      reason: "You asked",
    });
    expect(parseChatScheduleProposal({ ...proposal, recurrence: null, reason: "" }).recurrence).toBeNull();
    expect(() => parseChatScheduleProposal({ ...proposal, message: "  " })).toThrow("message");
    expect(() => parseChatScheduleProposal({ ...proposal, message: "é".repeat(4_001) })).toThrow("message");
    expect(() => parseChatScheduleProposal({ ...proposal, reason: "a".repeat(501) })).toThrow("reason");
    expect(() => parseChatScheduleProposal({ ...proposal, firstAt: "tomorrow" })).toThrow("firstAt");
    expect(() => parseChatScheduleProposal({ ...proposal, recurrence: undefined })).toThrow("recurrence");
  });

  it("parses proposal states", () => {
    const states = [{ proposalItemId: "item:a", state: "pending" }, { proposalItemId: "item:b", state: "dismissed" }];
    expect(parseChatScheduleProposalStates(states)).toEqual(states);
    expect(() => parseChatScheduleProposalStates([{ proposalItemId: "item:a", state: "expired" }])).toThrow("state");
  });
});

describe("chat push request contracts", () => {
  const push = {
    workingFolderId: "folder:1",
    executionEnvironmentId: "environment:1",
    localBranch: "topic",
    remote: "origin",
    destinationBranch: "topic",
    pushUrl: "git@example.invalid:org/repo.git",
    objectId: "a".repeat(64),
    gitConfigDigest: "b".repeat(64),
  };

  it("parses a push target and accepts a user name without a password", () => {
    expect(parseChatPushRequest(push)).toEqual(push);
    for (const pushUrl of ["ssh://git@example.invalid/r", "https://example.invalid/r?a=b@c:d", "/srv/repo.git"]) {
      expect(parseChatPushRequest({ ...push, pushUrl }).pushUrl).toBe(pushUrl);
    }
  });

  it("refuses passwords, unprintable URLs, and malformed ids", () => {
    for (const pushUrl of ["https://me:secret@example.invalid/r", "ssh://a:b@c@example.invalid/r", "", "a b", "a\u0007", "x".repeat(2_049)]) {
      expect(() => parseChatPushRequest({ ...push, pushUrl }), pushUrl).toThrow("pushUrl");
    }
    expect(() => parseChatPushRequest({ ...push, objectId: "a".repeat(41) })).toThrow("objectId");
    expect(() => parseChatPushRequest({ ...push, objectId: "A".repeat(40) })).toThrow("objectId");
    expect(() => parseChatPushRequest({ ...push, gitConfigDigest: "b".repeat(40) })).toThrow("gitConfigDigest");
    expect(() => parseChatPushRequest({ ...push, remote: "" })).toThrow("remote");
    expect(() => parseChatPushRequest({ ...push, executionEnvironmentId: undefined })).toThrow("executionEnvironmentId");
  });

  it("parses a commit the request asks Ganbaru to make", () => {
    const commit = {
      message: "feat: add search\n\nIndexes titles.",
      treeId: "c".repeat(40),
      author: "Ada Lovelace <ada@example.invalid>",
      signed: true,
      changedPaths: ["src/search.ts", "docs/search.md"],
      changedPathCount: 2,
    };
    expect(parseChatPushRequest({ ...push, commit })).toEqual({ ...push, commit });
    expect(parseChatPushRequest(push)).not.toHaveProperty("commit");
    const unlisted = { ...commit, changedPaths: [], changedPathCount: 400 };
    expect(parseChatPushRequest({ ...push, commit: unlisted }).commit).toEqual(unlisted);
  });

  it("refuses commits whose message, author, or paths Rust never writes", () => {
    const commit = {
      message: "fix: guard",
      treeId: "c".repeat(40),
      author: "Ada <ada@example.invalid>",
      signed: false,
      changedPaths: ["a.ts"],
      changedPathCount: 1,
    };
    for (const message of ["", " fix: guard", "fix\u0007", "fix\r\nguard", "x".repeat(5_001)]) {
      expect(() => parseChatPushRequest({ ...push, commit: { ...commit, message } }), message).toThrow("message");
    }
    expect(() => parseChatPushRequest({ ...push, commit: { ...commit, author: "Ada\u001b[31m" } })).toThrow("author");
    expect(() => parseChatPushRequest({ ...push, commit: { ...commit, author: "" } })).toThrow("author");
    expect(() => parseChatPushRequest({ ...push, commit: { ...commit, changedPaths: ["a\nb"] } })).toThrow("changedPaths");
    expect(() => parseChatPushRequest({ ...push, commit: { ...commit, changedPaths: ["é".repeat(513)] } })).toThrow("changedPaths");
    const tooMany = Array.from({ length: 51 }, (_, index) => `f${index}`);
    expect(() => parseChatPushRequest({ ...push, commit: { ...commit, changedPaths: tooMany, changedPathCount: 51 } })).toThrow("changedPaths");
    expect(() => parseChatPushRequest({ ...push, commit: { ...commit, changedPathCount: 0 } })).toThrow("changedPaths");
    expect(() => parseChatPushRequest({ ...push, commit: { ...commit, changedPaths: [], changedPathCount: 0 } })).toThrow("changedPaths");
    expect(() => parseChatPushRequest({ ...push, commit: { ...commit, treeId: "c".repeat(39) } })).toThrow("treeId");
    expect(() => parseChatPushRequest({ ...push, commit: { ...commit, signed: "yes" } })).toThrow("signed");
  });

  it("parses push request states", () => {
    const states = [{ requestItemId: "item:a", state: "waiting" }, { requestItemId: "item:b", state: "pushed" }];
    expect(parseChatPushRequestStates(states)).toEqual(states);
    expect(() => parseChatPushRequestStates([{ requestItemId: "item:a", state: "pending" }])).toThrow("state");
  });

  it("parses standing push approvals and refuses a push URL with a password", () => {
    const approval = {
      approvalId: "push-approval-1",
      channelId: "channel:1",
      agentId: "participant:atlas",
      workingFolderId: "folder:1",
      workingFolderName: "App",
      pushUrl: "git@example.invalid:org/repo.git",
      destinationBranch: "main",
      approvedAt: "2026-11-14T12:00:00.000Z",
    };
    expect(parseChatPushApprovals([approval])).toEqual([approval]);
    expect(() => parseChatPushApprovals([{ ...approval, pushUrl: "https://me:secret@example.invalid/r" }])).toThrow("pushUrl");
    expect(() => parseChatPushApprovals([{ ...approval, approvalId: "" }])).toThrow("approvalId");
    expect(() => parseChatPushApprovals([{ ...approval, destinationBranch: "main\u0007" }])).toThrow("destinationBranch");
    expect(() => parseChatPushApprovals([{ ...approval, approvedAt: "yesterday" }])).toThrow("approvedAt");
  });
});

describe("chat channel note contracts", () => {
  const note = {
    id: "note:1",
    channelId: "channel:general",
    body: "Reply in English",
    sourceItemId: "item:proposal",
    proposedBy: "participant:atlas",
    editedBy: null,
    revision: 2,
    createdAt: timestamp,
    updatedAt: timestamp,
  };

  it("parses a note with and without provenance", () => {
    expect(parseChatChannelNote(note)).toEqual(note);
    const written = { ...note, sourceItemId: null, proposedBy: null, editedBy: "participant:local-owner" };
    expect(parseChatChannelNotes([written, note])).toEqual([written, note]);
    expect(parseChatChannelNote({ ...note, body: "é".repeat(1_000) }).body).toHaveLength(1_000);
  });

  it("rejects an oversize or blank body, unknown fields, and malformed provenance", () => {
    expect(() => parseChatChannelNote({ ...note, body: "é".repeat(1_001) })).toThrow("body");
    expect(() => parseChatChannelNote({ ...note, body: " \n " })).toThrow("body");
    expect(() => parseChatChannelNote({ ...note, deletedAt: null })).toThrow("deletedAt");
    expect(() => parseChatChannelNote({ ...note, proposedBy: undefined })).toThrow("proposedBy");
    expect(() => parseChatChannelNote({ ...note, revision: -1 })).toThrow("revision");
    expect(() => parseChatChannelNote({ ...note, updatedAt: "today" })).toThrow("updatedAt");
  });

  it("parses note proposal states", () => {
    const states = [
      { proposalItemId: "item:a", state: "pending" },
      { proposalItemId: "item:b", state: "saved" },
      { proposalItemId: "item:c", state: "dismissed" },
    ];
    expect(parseChatNoteProposalStates(states)).toEqual(states);
    expect(() => parseChatNoteProposalStates([{ proposalItemId: "item:a", state: "confirmed" }])).toThrow("state");
  });
});
