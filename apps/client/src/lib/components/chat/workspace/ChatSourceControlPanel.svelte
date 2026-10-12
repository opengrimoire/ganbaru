<script lang="ts">
  import { onMount, untrack } from "svelte";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import Check from "@lucide/svelte/icons/check";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import GitPullRequest from "@lucide/svelte/icons/git-pull-request";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import Minus from "@lucide/svelte/icons/minus";
  import Plus from "@lucide/svelte/icons/plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import * as chatApi from "$lib/api/chat";
  import type { ChatExecutionEnvironmentRead, GitChangedPathRead, GitStatusRead, HostedChangeRequestRead, HostedSourceControlRead } from "$lib/chat/contracts";
  import { subscribeChatWorkspaceChanges } from "$lib/chat/workspace/observer-client";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";

  const { t } = getLocalization();
  const chat = getChat();
  let status = $state<GitStatusRead | null>(null);
  let loading = $state(false);
  let operation = $state<string | null>(null);
  let commitMessage = $state("");
  let error = $state<string | null>(null);
  let environments = $state<ChatExecutionEnvironmentRead[]>([]);
  let environmentLoading = $state(false);
  let worktreeFormOpen = $state(false);
  let worktreeBranch = $state("");
  let worktreeBase = $state("HEAD");
  let worktreeRemote = $state("origin");
  let fetchRemote = $state(false);
  let hostedProviders = $state<HostedSourceControlRead[]>([]);
  let hostedRequests = $state<HostedChangeRequestRead[]>([]);
  let hostedProviderKind = $state<HostedSourceControlRead["kind"] | null>(null);
  let hostedLoading = $state(false);
  let changeRequestFormOpen = $state(false);
  let changeRequestTitle = $state("");
  let changeRequestBody = $state("");
  let changeRequestBase = $state("");
  let changeRequestHead = $state("");
  let isChangeRequestDraft = $state(false);
  let bitbucketCredentialOpen = $state(false);
  let bitbucketUsername = $state("");
  let bitbucketToken = $state("");
  let repositorySetupOpen = $state(false);
  let initialBranch = $state("main");
  let cloneUrl = $state("");
  let refreshPending = false;
  let liveRefreshTimer: number | null = null;
  let refreshRequest = 0;
  let environmentRequest = 0;
  let providerRequest = 0;
  let hostedRequest = 0;
  let operationRequest = 0;
  const workspace = $derived(chat.selectedWorkingFolderId);
  const staged = $derived(status?.files.filter(isStaged) ?? []);
  const unstaged = $derived(status?.files.filter(isUnstaged) ?? []);
  const hostedProvider = $derived(hostedProviders.find((entry) => entry.kind === hostedProviderKind) ?? null);

  onMount(() => {
    const unsubscribe = subscribeChatWorkspaceChanges((batch) => {
      if (batch.workingFolderId !== workspace
        || batch.executionEnvironmentId !== chat.selectedExecutionEnvironmentId
        || (batch.relativePaths.length === 0 && !batch.gitMetadataChanged && !batch.overflowed)) return;
      if (liveRefreshTimer !== null) window.clearTimeout(liveRefreshTimer);
      liveRefreshTimer = window.setTimeout(() => {
        liveRefreshTimer = null;
        void refresh();
      }, 100);
    });
    return () => {
      unsubscribe();
      if (liveRefreshTimer !== null) window.clearTimeout(liveRefreshTimer);
    };
  });

  $effect(() => {
    const workingFolderId = workspace;
    const executionEnvironmentId = chat.selectedExecutionEnvironmentId;
    untrack(() => {
      refreshRequest += 1;
      environmentRequest += 1;
      providerRequest += 1;
      hostedRequest += 1;
      operationRequest += 1;
      loading = false;
      environmentLoading = false;
      hostedLoading = false;
      operation = null;
      refreshPending = false;
      status = null;
      environments = [];
      hostedProviders = [];
      hostedRequests = [];
      hostedProviderKind = null;
      worktreeFormOpen = false;
      changeRequestFormOpen = false;
      bitbucketCredentialOpen = false;
      repositorySetupOpen = false;
      commitMessage = "";
      worktreeBranch = "";
      changeRequestTitle = "";
      changeRequestBody = "";
      changeRequestBase = "";
      changeRequestHead = "";
      bitbucketUsername = "";
      bitbucketToken = "";
      cloneUrl = "";
      error = null;
      if (!scopeMatches(workingFolderId, executionEnvironmentId)) return;
      void refresh();
      void loadEnvironments();
      void discoverProviders();
    });
  });

  function scopeMatches(workingFolderId: string | null, executionEnvironmentId: string | null): boolean {
    return workspace === workingFolderId
      && chat.selectedExecutionEnvironmentId === executionEnvironmentId;
  }

  function isStaged(file: GitChangedPathRead): boolean {
    return file.indexStatus !== "." && file.indexStatus !== "?" && file.indexStatus !== "!";
  }

  function isUnstaged(file: GitChangedPathRead): boolean {
    return file.untracked || (file.worktreeStatus !== "." && file.worktreeStatus !== "!");
  }

  function message(reason: unknown): string {
    if (reason instanceof Error) return reason.message;
    if (typeof reason === "object" && reason !== null && "message" in reason) {
      const value = (reason as { message?: unknown }).message;
      if (typeof value === "string") return value;
    }
    return t("chat.sourceControl.operationFailed");
  }

  async function refresh(): Promise<void> {
    const workingFolderId = workspace;
    const executionEnvironmentId = chat.selectedExecutionEnvironmentId;
    if (!workingFolderId) return;
    if (loading || operation) {
      refreshPending = true;
      return;
    }
    const request = ++refreshRequest;
    loading = true;
    error = null;
    try {
      const nextStatus = await chatApi.readGitStatus(workingFolderId, executionEnvironmentId);
      if (request === refreshRequest && scopeMatches(workingFolderId, executionEnvironmentId)) {
        status = nextStatus;
      }
    } catch (reason) {
      if (request === refreshRequest && scopeMatches(workingFolderId, executionEnvironmentId)) {
        error = message(reason);
      }
    } finally {
      if (request === refreshRequest) {
        loading = false;
        if (refreshPending) {
          refreshPending = false;
          queueMicrotask(() => void refresh());
        }
      }
    }
  }

  function openReview(mode: "staged" | "unstaged", relativePath: string): void {
    window.dispatchEvent(new CustomEvent("ganbaru-ai:chat-open-review", {
      detail: { source: { kind: "working_tree", mode }, relativePath },
    }));
  }

  async function loadEnvironments(): Promise<void> {
    const workingFolderId = workspace;
    const executionEnvironmentId = chat.selectedExecutionEnvironmentId;
    if (!workingFolderId || environmentLoading) return;
    const request = ++environmentRequest;
    environmentLoading = true;
    try {
      const nextEnvironments = await chatApi.listChatExecutionEnvironments(workingFolderId);
      if (request === environmentRequest && scopeMatches(workingFolderId, executionEnvironmentId)) {
        environments = nextEnvironments;
      }
    } catch (reason: unknown) {
      if (request === environmentRequest && scopeMatches(workingFolderId, executionEnvironmentId)) {
        error = message(reason);
      }
    } finally {
      if (request === environmentRequest) environmentLoading = false;
    }
  }

  async function discoverProviders(): Promise<void> {
    const workingFolderId = workspace;
    const executionEnvironmentId = chat.selectedExecutionEnvironmentId;
    if (!workingFolderId) return;
    const request = ++providerRequest;
    try {
      const providers = await chatApi.discoverHostedSourceControl(workingFolderId, executionEnvironmentId);
      if (request !== providerRequest || !scopeMatches(workingFolderId, executionEnvironmentId)) return;
      hostedProviders = providers;
      const selected = providers.find((provider) => provider.kind === hostedProviderKind && provider.detectedForRepository)
        ?? providers.find((provider) => provider.detectedForRepository && provider.status === "available")
        ?? providers.find((provider) => provider.detectedForRepository)
        ?? null;
      hostedProviderKind = selected?.kind ?? null;
      await loadHostedRequests();
    } catch (reason: unknown) {
      if (request === providerRequest && scopeMatches(workingFolderId, executionEnvironmentId)) {
        error = message(reason);
      }
    }
  }

  async function loadHostedRequests(): Promise<void> {
    const provider = hostedProviders.find((entry) => entry.kind === hostedProviderKind);
    const workingFolderId = workspace;
    const executionEnvironmentId = chat.selectedExecutionEnvironmentId;
    if (!workingFolderId || !provider?.repositorySlug || provider.status !== "available") {
      hostedRequest += 1;
      hostedLoading = false;
      hostedRequests = [];
      return;
    }
    const request = ++hostedRequest;
    const providerKind = provider.kind;
    const repositorySlug = provider.repositorySlug;
    hostedLoading = true;
    try {
      const nextRequests = await chatApi.listHostedChangeRequests(
        workingFolderId,
        executionEnvironmentId,
        providerKind,
        repositorySlug,
      );
      if (request === hostedRequest
        && scopeMatches(workingFolderId, executionEnvironmentId)
        && hostedProviderKind === providerKind) {
        hostedRequests = nextRequests;
      }
    } catch (reason: unknown) {
      if (request === hostedRequest
        && scopeMatches(workingFolderId, executionEnvironmentId)
        && hostedProviderKind === providerKind) {
        hostedRequests = [];
        error = message(reason);
      }
    } finally {
      if (request === hostedRequest) hostedLoading = false;
    }
  }

  async function createChangeRequest(): Promise<void> {
    const provider = hostedProviders.find((entry) => entry.kind === hostedProviderKind);
    const workingFolderId = workspace;
    const executionEnvironmentId = chat.selectedExecutionEnvironmentId;
    if (!workingFolderId || !provider?.repositorySlug || operation) return;
    const request = ++operationRequest;
    const providerKind = provider.kind;
    const repositorySlug = provider.repositorySlug;
    operation = "create-change-request";
    error = null;
    try {
      const created = await chatApi.createHostedChangeRequest({
        workingFolderId,
        executionEnvironmentId,
        providerKind,
        repositorySlug,
        title: changeRequestTitle.trim(),
        body: changeRequestBody,
        baseBranch: changeRequestBase.trim(),
        headBranch: changeRequestHead.trim(),
        draft: isChangeRequestDraft,
      });
      if (request !== operationRequest || !scopeMatches(workingFolderId, executionEnvironmentId)) return;
      hostedRequests = [created, ...hostedRequests.filter((entry) => entry.number !== created.number)];
      changeRequestFormOpen = false;
      changeRequestTitle = "";
      changeRequestBody = "";
    } catch (reason: unknown) {
      if (request === operationRequest && scopeMatches(workingFolderId, executionEnvironmentId)) error = message(reason);
    } finally {
      if (request === operationRequest) operation = null;
    }
  }

  async function checkoutChangeRequest(request: HostedChangeRequestRead): Promise<void> {
    const provider = hostedProviders.find((entry) => entry.kind === request.providerKind);
    const workingFolderId = workspace;
    const executionEnvironmentId = chat.selectedExecutionEnvironmentId;
    if (!workingFolderId || operation) return;
    const operationId = ++operationRequest;
    operation = "checkout-change-request";
    error = null;
    try {
      const nextStatus = await chatApi.checkoutHostedChangeRequest(
        workingFolderId,
        executionEnvironmentId,
        request.providerKind,
        String(request.number),
        provider?.remoteName ?? null,
      );
      if (operationId === operationRequest && scopeMatches(workingFolderId, executionEnvironmentId)) status = nextStatus;
    } catch (reason: unknown) {
      if (operationId === operationRequest && scopeMatches(workingFolderId, executionEnvironmentId)) error = message(reason);
    } finally {
      if (operationId === operationRequest) operation = null;
    }
  }

  async function configureBitbucket(): Promise<void> {
    const workingFolderId = workspace;
    const executionEnvironmentId = chat.selectedExecutionEnvironmentId;
    const repositorySlug = hostedProvider?.repositorySlug;
    if (!workingFolderId || !repositorySlug || operation) return;
    const request = ++operationRequest;
    operation = "configure-bitbucket";
    error = null;
    try {
      await chatApi.configureBitbucketCredential(
        repositorySlug,
        bitbucketUsername.trim(),
        bitbucketToken,
      );
      if (request !== operationRequest || !scopeMatches(workingFolderId, executionEnvironmentId)) return;
      bitbucketToken = "";
      bitbucketCredentialOpen = false;
      await discoverProviders();
    } catch (reason: unknown) {
      if (request === operationRequest && scopeMatches(workingFolderId, executionEnvironmentId)) error = message(reason);
    } finally {
      if (request === operationRequest) operation = null;
    }
  }

  async function removeBitbucketCredential(): Promise<void> {
    const workingFolderId = workspace;
    const executionEnvironmentId = chat.selectedExecutionEnvironmentId;
    const repositorySlug = hostedProvider?.repositorySlug;
    if (!workingFolderId || !repositorySlug || operation) return;
    const request = ++operationRequest;
    operation = "remove-bitbucket-credential";
    error = null;
    try {
      await chatApi.removeBitbucketCredential(repositorySlug);
      if (request !== operationRequest || !scopeMatches(workingFolderId, executionEnvironmentId)) return;
      hostedRequests = [];
      await discoverProviders();
    } catch (reason: unknown) {
      if (request === operationRequest && scopeMatches(workingFolderId, executionEnvironmentId)) error = message(reason);
    } finally {
      if (request === operationRequest) operation = null;
    }
  }

  async function createWorktree(): Promise<void> {
    const workingFolderId = workspace;
    const executionEnvironmentId = chat.selectedExecutionEnvironmentId;
    const branchName = worktreeBranch.trim();
    const baseReference = worktreeBase.trim();
    if (!workingFolderId || operation || !branchName || !baseReference) return;
    const request = ++operationRequest;
    operation = "create-worktree";
    error = null;
    try {
      const environment = await chatApi.createChatWorktreeEnvironment({
        environmentId: crypto.randomUUID(),
        workingFolderId,
        displayName: branchName,
        branchName,
        baseReference,
        remoteName: worktreeRemote.trim() || null,
        fetchRemote,
      });
      if (request !== operationRequest || !scopeMatches(workingFolderId, executionEnvironmentId)) return;
      environments = [...environments, environment];
      chat.setExecutionEnvironment(environment.id);
      worktreeFormOpen = false;
      worktreeBranch = "";
    } catch (reason: unknown) {
      if (request === operationRequest && scopeMatches(workingFolderId, executionEnvironmentId)) error = message(reason);
    } finally {
      if (request === operationRequest) operation = null;
    }
  }

  async function removeWorktree(environment: ChatExecutionEnvironmentRead): Promise<void> {
    const workingFolderId = workspace;
    const executionEnvironmentId = chat.selectedExecutionEnvironmentId;
    if (!workingFolderId || operation || !window.confirm(t("chat.sourceControl.confirmRemoveWorktree", environment.displayName))) return;
    const request = ++operationRequest;
    operation = "remove-worktree";
    error = null;
    try {
      await chatApi.removeChatWorktreeEnvironment(workingFolderId, environment.id, true);
      if (request !== operationRequest || !scopeMatches(workingFolderId, executionEnvironmentId)) return;
      environments = environments.filter((entry) => entry.id !== environment.id);
      if (chat.selectedExecutionEnvironmentId === environment.id) chat.setExecutionEnvironment(null);
    } catch (reason: unknown) {
      if (request === operationRequest && scopeMatches(workingFolderId, executionEnvironmentId)) error = message(reason);
    } finally {
      if (request === operationRequest) operation = null;
    }
  }

  async function run(
    name: string,
    action: (workspaceId: string, executionEnvironmentId: string | null) => Promise<GitStatusRead>,
  ): Promise<boolean> {
    const workingFolderId = workspace;
    const executionEnvironmentId = chat.selectedExecutionEnvironmentId;
    if (!workingFolderId || operation) return false;
    const request = ++operationRequest;
    operation = name;
    error = null;
    try {
      const nextStatus = await action(workingFolderId, executionEnvironmentId);
      if (request !== operationRequest || !scopeMatches(workingFolderId, executionEnvironmentId)) return false;
      status = nextStatus;
      return true;
    } catch (reason) {
      if (request === operationRequest && scopeMatches(workingFolderId, executionEnvironmentId)) {
        error = message(reason);
      }
      return false;
    } finally {
      if (request === operationRequest) {
        operation = null;
        if (refreshPending) {
          refreshPending = false;
          queueMicrotask(() => void refresh());
        }
      }
    }
  }

  async function commit(): Promise<void> {
    const nextMessage = commitMessage.trim();
    if (!nextMessage) return;
    if (await run("commit", (workspaceId, environmentId) => chatApi.commitGitChanges(workspaceId, nextMessage, environmentId))) {
      commitMessage = "";
    }
  }

  async function initializeRepository(): Promise<void> {
    const succeeded = await run("initialize", (workspaceId, environmentId) => chatApi.initializeGitRepository(
      workspaceId,
      initialBranch.trim() || null,
      environmentId,
    ));
    if (succeeded) repositorySetupOpen = false;
  }

  async function cloneRepository(): Promise<void> {
    const nextUrl = cloneUrl.trim();
    if (!nextUrl) return;
    const succeeded = await run("clone", (workspaceId, environmentId) => chatApi.cloneGitRepository(
      workspaceId,
      nextUrl,
      "origin",
      environmentId,
    ));
    if (succeeded) {
      cloneUrl = "";
      repositorySetupOpen = false;
      await discoverProviders();
    }
  }

  async function discard(file: GitChangedPathRead): Promise<void> {
    const confirmed = window.confirm(t("chat.sourceControl.confirmDiscard", file.relativePath));
    if (!confirmed) return;
    await run("discard", (workspaceId, environmentId) => chatApi.discardGitPaths(
      workspaceId,
      [file.relativePath],
      true,
      environmentId,
    ));
  }

  function statusLabel(file: GitChangedPathRead): string {
    if (file.untracked) return "U";
    const value = isStaged(file) ? file.indexStatus : file.worktreeStatus;
    return value === "." ? "" : value;
  }
</script>

<section class="source-panel" aria-label={t("chat.inspector.sourceControl")}>
  <header>
    <div class="branch-summary">
      <strong>{status?.detached ? t("chat.sourceControl.detached") : status?.branch ?? t("chat.sourceControl.noBranch")}</strong>
      {#if status?.upstream}<small>{status.upstream}</small>{/if}
      {#if status && (status.ahead > 0 || status.behind > 0)}
        <span>{#if status.ahead > 0}<ArrowUp size={12} />{status.ahead}{/if}{#if status.behind > 0}<ArrowDown size={12} />{status.behind}{/if}</span>
      {/if}
    </div>
    <button type="button" class="chat-icon-button" disabled={loading || operation !== null} aria-label={t("chat.sourceControl.refresh")} title={t("chat.sourceControl.refresh")} onclick={() => void refresh()}><RefreshCw size={14} /></button>
  </header>

  <div class="network-actions">
    <button type="button" disabled={operation !== null} onclick={() => void run("fetch", (id, environmentId) => chatApi.fetchGitRemote(id, null, environmentId))}>{t("chat.sourceControl.fetch")}</button>
    <button type="button" disabled={operation !== null} onclick={() => void run("pull", (id, environmentId) => chatApi.pullGitBranch(id, null, null, environmentId))}>{t("chat.sourceControl.pull")}</button>
    <button type="button" disabled={operation !== null} onclick={() => void run("push", (id, environmentId) => chatApi.pushGitBranch(id, null, null, false, false, environmentId))}>{t("chat.sourceControl.push")}</button>
  </div>

  <section class="environments">
    <div class="section-heading">
      <strong>{t("chat.sourceControl.executionEnvironment")}</strong>
      <button type="button" disabled={chat.selectedThreadId !== null || operation !== null} onclick={() => { worktreeFormOpen = !worktreeFormOpen; }}><Plus size={13} />{t("chat.sourceControl.newWorktree")}</button>
    </div>
    <div class="environment-list">
      {#each environments as environment (environment.id)}
        <label class:unavailable={environment.lifecycleState !== "available"}>
          <input
            type="radio"
            name="chat-execution-environment"
            checked={(chat.selectedExecutionEnvironmentId ?? environments.find((entry) => entry.kind === "current_folder")?.id) === environment.id}
            disabled={chat.selectedThreadId !== null || environment.lifecycleState !== "available"}
            onchange={() => chat.setExecutionEnvironment(environment.kind === "current_folder" ? null : environment.id)}
          />
          <span><strong>{environment.displayName}</strong><small>{environment.branchName ?? t("chat.sourceControl.currentFolder")}</small></span>
          {#if environment.kind === "worktree"}<button type="button" class="chat-icon-button" disabled={operation !== null} aria-label={t("chat.sourceControl.removeWorktree", environment.displayName)} onclick={(event) => { event.preventDefault(); void removeWorktree(environment); }}><Trash2 size={13} /></button>{/if}
        </label>
      {/each}
    </div>
    {#if worktreeFormOpen}
      <form class="worktree-form" onsubmit={(event) => { event.preventDefault(); void createWorktree(); }}>
        <input class="field" bind:value={worktreeBranch} maxlength="1024" placeholder={t("chat.sourceControl.branchName")} aria-label={t("chat.sourceControl.branchName")} />
        <input class="field" bind:value={worktreeBase} maxlength="1024" placeholder={t("chat.sourceControl.baseReference")} aria-label={t("chat.sourceControl.baseReference")} />
        <input class="field" bind:value={worktreeRemote} maxlength="240" placeholder={t("chat.sourceControl.remoteName")} aria-label={t("chat.sourceControl.remoteName")} />
        <label><Checkbox bind:checked={fetchRemote} />{t("chat.sourceControl.fetchBeforeCreate")}</label>
        <button type="submit" class="chat-primary-button" disabled={!worktreeBranch.trim() || !worktreeBase.trim() || operation !== null}>{t("chat.sourceControl.createWorktree")}</button>
      </form>
    {/if}
  </section>

  <section class="hosted-providers">
    <div class="section-heading">
      <strong>{t("chat.sourceControl.hostedProviders")}</strong>
      {#if hostedProvider?.status === "available"}<button type="button" disabled={operation !== null} onclick={() => { changeRequestFormOpen = !changeRequestFormOpen; }}>{t("chat.sourceControl.newChangeRequest")}</button>{/if}
    </div>
    {#if hostedProvider?.kind === "bitbucket"}
      <div class="credential-actions">
        <button type="button" disabled={operation !== null} onclick={() => { bitbucketCredentialOpen = !bitbucketCredentialOpen; }}><KeyRound size={13} />{hostedProvider.status === "available" ? t("chat.sourceControl.replaceBitbucketCredential") : t("chat.sourceControl.configureBitbucket")}</button>
        {#if hostedProvider.status === "available"}<button type="button" disabled={operation !== null} onclick={() => void removeBitbucketCredential()}>{t("chat.sourceControl.removeBitbucketCredential")}</button>{/if}
      </div>
    {/if}
    {#if bitbucketCredentialOpen && hostedProvider?.kind === "bitbucket"}
      <form class="credential-form" onsubmit={(event) => { event.preventDefault(); void configureBitbucket(); }}>
        <input class="field" bind:value={bitbucketUsername} autocomplete="username" maxlength="512" placeholder={t("chat.sourceControl.bitbucketUsername")} aria-label={t("chat.sourceControl.bitbucketUsername")} />
        <input class="field" type="password" bind:value={bitbucketToken} autocomplete="off" maxlength="8192" placeholder={t("chat.sourceControl.bitbucketToken")} aria-label={t("chat.sourceControl.bitbucketToken")} />
        <p>{t("chat.sourceControl.bitbucketCredentialNotice")}</p>
        <button type="submit" class="chat-primary-button" disabled={!bitbucketUsername.trim() || !bitbucketToken || operation !== null}>{t("chat.sourceControl.saveBitbucketCredential")}</button>
      </form>
    {/if}
    <div class="provider-list">
      {#each hostedProviders as provider (provider.kind)}
        <button type="button" class:detected={provider.detectedForRepository} class:selected={hostedProviderKind === provider.kind} title={provider.unavailableReason ?? provider.version ?? ""} onclick={() => { hostedProviderKind = provider.kind; void loadHostedRequests(); }}>
          <i class:available={provider.status === "available"}></i>{provider.label}
          {#if provider.repositorySlug}<small>{provider.repositorySlug}</small>{/if}
        </button>
      {/each}
    </div>
    {#if changeRequestFormOpen}
      <form class="change-request-form" onsubmit={(event) => { event.preventDefault(); void createChangeRequest(); }}>
        <input class="field" bind:value={changeRequestTitle} maxlength="1024" placeholder={t("chat.sourceControl.changeRequestTitle")} aria-label={t("chat.sourceControl.changeRequestTitle")} />
        <div><input class="field" bind:value={changeRequestBase} maxlength="1024" placeholder={t("chat.sourceControl.baseBranch")} aria-label={t("chat.sourceControl.baseBranch")} /><input class="field" bind:value={changeRequestHead} maxlength="1024" placeholder={t("chat.sourceControl.headBranch")} aria-label={t("chat.sourceControl.headBranch")} /></div>
        <textarea class="field" bind:value={changeRequestBody} maxlength="262144" rows="3" placeholder={t("chat.sourceControl.changeRequestBody")} aria-label={t("chat.sourceControl.changeRequestBody")}></textarea>
        <label><Checkbox bind:checked={isChangeRequestDraft} />{t("chat.sourceControl.draftChangeRequest")}</label>
        <button type="submit" class="chat-primary-button" disabled={!changeRequestTitle.trim() || !changeRequestBase.trim() || !changeRequestHead.trim() || operation !== null}>{t("chat.sourceControl.createChangeRequest")}</button>
      </form>
    {/if}
    {#if hostedLoading}
      <p class="empty compact">{t("common.loading")}</p>
    {:else if hostedProviderKind && hostedRequests.length === 0}
      <p class="empty compact">{t("chat.sourceControl.noChangeRequests")}</p>
    {:else}
      <div class="change-request-list">
        {#each hostedRequests as request (`${request.providerKind}:${request.number}`)}
          <article>
            <GitPullRequest size={13} />
            <button type="button" class="change-request-title" onclick={() => void chatApi.openChatExternalUrl(request.url)}><strong>#{request.number} {request.title}</strong><small>{request.headBranch} → {request.baseBranch}</small></button>
            <button type="button" class="chat-icon-button" disabled={operation !== null} aria-label={t("chat.sourceControl.checkoutChangeRequest", request.number)} title={t("chat.sourceControl.checkoutChangeRequest", request.number)} onclick={() => void checkoutChangeRequest(request)}><Check size={13} /></button>
            <button type="button" class="chat-icon-button" aria-label={t("chat.sourceControl.openChangeRequest", request.number)} title={t("chat.sourceControl.openChangeRequest", request.number)} onclick={() => void chatApi.openChatExternalUrl(request.url)}><ExternalLink size={13} /></button>
          </article>
        {/each}
      </div>
    {/if}
  </section>

  {#if error}<Notice class="error" message={error} />{/if}
  {#if !status && !loading}
    <section class="repository-setup">
      <button type="button" onclick={() => { repositorySetupOpen = !repositorySetupOpen; }}>
        {t("chat.sourceControl.repositorySetup")}
      </button>
      {#if repositorySetupOpen}
        <form onsubmit={(event) => { event.preventDefault(); void initializeRepository(); }}>
          <input class="field" bind:value={initialBranch} maxlength="1024" aria-label={t("chat.sourceControl.initialBranch")} placeholder={t("chat.sourceControl.initialBranch")} />
          <button type="submit" class="chat-secondary-button" disabled={operation !== null}>{t("chat.sourceControl.initializeRepository")}</button>
        </form>
        <form onsubmit={(event) => { event.preventDefault(); void cloneRepository(); }}>
          <input class="field" bind:value={cloneUrl} maxlength="8192" aria-label={t("chat.sourceControl.cloneUrl")} placeholder={t("chat.sourceControl.cloneUrl")} />
          <button type="submit" class="chat-primary-button" disabled={!cloneUrl.trim() || operation !== null}>{t("chat.sourceControl.cloneRepository")}</button>
        </form>
        <p>{t("chat.sourceControl.cloneEmptyFolderNotice")}</p>
      {/if}
    </section>
  {/if}
  {#if loading && !status}
    <p class="empty">{t("common.loading")}</p>
  {:else if status}
    <div class="changes">
      <section>
        <h3><span>{t("chat.sourceControl.staged")}</span><small>{staged.length}</small></h3>
        {#each staged as file (file.relativePath)}
          <div class="file-row">
            <code>{statusLabel(file)}</code><button type="button" class="file-link" title={file.relativePath} onclick={() => openReview("staged", file.relativePath)}>{file.relativePath}</button>
            <button type="button" class="chat-icon-button" disabled={operation !== null} aria-label={t("chat.sourceControl.unstageFile", file.relativePath)} onclick={() => void run("unstage", (id, environmentId) => chatApi.unstageGitPaths(id, [file.relativePath], environmentId))}><Minus size={13} /></button>
          </div>
        {:else}<p class="empty compact">{t("chat.sourceControl.noneStaged")}</p>{/each}
      </section>
      <section>
        <h3><span>{t("chat.sourceControl.changes")}</span><small>{unstaged.length}</small></h3>
        {#each unstaged as file (file.relativePath)}
          <div class="file-row">
            <code>{statusLabel(file)}</code><button type="button" class="file-link" title={file.relativePath} onclick={() => openReview("unstaged", file.relativePath)}>{file.relativePath}</button>
            <span class="file-actions">
              <button type="button" class="chat-icon-button" disabled={operation !== null} aria-label={t("chat.sourceControl.discardFile", file.relativePath)} onclick={() => void discard(file)}><Trash2 size={13} /></button>
              <button type="button" class="chat-icon-button" disabled={operation !== null} aria-label={t("chat.sourceControl.stageFile", file.relativePath)} onclick={() => void run("stage", (id, environmentId) => chatApi.stageGitPaths(id, [file.relativePath], environmentId))}><Plus size={13} /></button>
            </span>
          </div>
        {:else}<p class="empty compact">{t("chat.sourceControl.clean")}</p>{/each}
      </section>
    </div>
    <form class="commit" onsubmit={(event) => { event.preventDefault(); void commit(); }}>
      <label for="chat-commit-message">{t("chat.sourceControl.commitMessage")}</label>
      <textarea class="field" id="chat-commit-message" maxlength="4000" rows="3" bind:value={commitMessage} placeholder={t("chat.sourceControl.commitPlaceholder")}></textarea>
      <button type="submit" class="chat-primary-button" disabled={!commitMessage.trim() || staged.length === 0 || operation !== null}><Check size={14} />{t("chat.sourceControl.commit")}</button>
    </form>
  {/if}
</section>

<style>
  .source-panel { display: flex; height: 100%; min-height: 0; flex-direction: column; overflow: hidden; }
  header { display: flex; align-items: center; gap: 0.5rem; border-bottom: 1px solid var(--border); padding: 0.55rem 0.7rem; }
  .branch-summary { display: flex; min-width: 0; flex: 1; align-items: center; gap: 0.45rem; }
  .branch-summary strong, .branch-summary small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .branch-summary strong { font-size: calc(0.78rem * var(--type-scale)); }
  .branch-summary small { color: var(--muted-foreground); font-size: calc(0.66rem * var(--type-scale)); }
  .branch-summary span { display: inline-flex; align-items: center; gap: 0.1rem; color: var(--muted-foreground); font-size: calc(0.66rem * var(--type-scale)); }
  .network-actions { display: grid; grid-template-columns: repeat(3, 1fr); gap: 0.35rem; border-bottom: 1px solid var(--border); padding: 0.5rem 0.7rem; }
  .network-actions button { border: 1px solid var(--border); border-radius: 0.45rem; padding: 0.35rem; font-size: calc(0.7rem * var(--type-scale)); }
  .network-actions button:hover:not(:disabled) { background: var(--accent); }
  .environments { border-bottom: 1px solid var(--border); padding: 0.45rem 0.7rem; }
  .section-heading { display: flex; align-items: center; justify-content: space-between; gap: 0.5rem; }
  .section-heading strong { font-size: calc(0.7rem * var(--type-scale)); }
  .section-heading button { display: inline-flex; align-items: center; gap: 0.2rem; border-radius: 0.35rem; padding: 0.25rem 0.35rem; color: var(--muted-foreground); font-size: calc(0.66rem * var(--type-scale)); }
  .section-heading button:hover:not(:disabled) { background: var(--accent); color: var(--foreground); }
  .environment-list { display: grid; gap: 0.15rem; margin-top: 0.35rem; }
  .environment-list > label { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 0.4rem; border-radius: 0.35rem; padding: 0.25rem; }
  .environment-list > label:hover { background: color-mix(in srgb, var(--accent) 55%, transparent); }
  .environment-list > label.unavailable { opacity: 0.55; }
  .environment-list span { display: flex; min-width: 0; justify-content: space-between; gap: 0.4rem; }
  .environment-list span strong, .environment-list span small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: calc(0.66rem * var(--type-scale)); }
  .environment-list span small { color: var(--muted-foreground); }
  .worktree-form { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 0.35rem; margin-top: 0.45rem; }
  .worktree-form > input { min-width: 0; font-size: var(--panel-detail-font-size); }
  .worktree-form > label { display: flex; align-items: center; gap: 0.3rem; color: var(--muted-foreground); font-size: calc(0.66rem * var(--type-scale)); }
  .worktree-form > button { grid-column: -2 / -1; }
  .hosted-providers { display: grid; gap: 0.35rem; border-bottom: 1px solid var(--border); padding: 0.45rem 0.7rem; }
  .provider-list { display: flex; min-width: 0; flex-wrap: wrap; gap: 0.3rem; }
  .provider-list > button { display: inline-flex; max-width: 100%; align-items: center; gap: 0.25rem; border: 1px solid var(--border); border-radius: 999px; padding: 0.2rem 0.4rem; color: var(--muted-foreground); font-size: calc(0.64rem * var(--type-scale)); }
  .provider-list > button.detected { color: var(--foreground); }
  .provider-list > button.selected { border-color: var(--primary); }
  .hosted-providers i { width: 0.4rem; height: 0.4rem; flex: 0 0 auto; border-radius: 999px; background: var(--destructive); }
  .hosted-providers i.available { background: var(--status-confirmed); }
  .hosted-providers small { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--muted-foreground); }
  .credential-actions { display: flex; flex-wrap: wrap; gap: 0.3rem; }
  .credential-actions button { display: inline-flex; align-items: center; gap: 0.25rem; border-radius: 0.35rem; padding: 0.25rem 0.35rem; color: var(--muted-foreground); font-size: calc(0.66rem * var(--type-scale)); }
  .credential-actions button:hover:not(:disabled) { background: var(--accent); color: var(--foreground); }
  .credential-form { display: grid; gap: 0.35rem; border-top: 1px solid var(--border); padding-top: 0.45rem; }
  .credential-form input { min-width: 0; font-size: var(--panel-detail-font-size); }
  .credential-form p { color: var(--muted-foreground); font-size: calc(0.62rem * var(--type-scale)); }
  .change-request-form { display: grid; gap: 0.35rem; border-top: 1px solid var(--border); padding-top: 0.45rem; }
  .change-request-form > input, .change-request-form textarea, .change-request-form div input { min-width: 0; font-size: var(--panel-detail-font-size); }
  .change-request-form > div { display: grid; grid-template-columns: 1fr 1fr; gap: 0.35rem; }
  .change-request-form > label { display: flex; align-items: center; gap: 0.3rem; color: var(--muted-foreground); font-size: calc(0.66rem * var(--type-scale)); }
  .change-request-list { display: grid; max-height: 11rem; gap: 0.15rem; overflow: auto; }
  .change-request-list article { display: grid; grid-template-columns: auto minmax(0, 1fr) auto auto; align-items: center; gap: 0.25rem; border-radius: 0.35rem; padding: 0.25rem; }
  .change-request-list article:hover { background: color-mix(in srgb, var(--accent) 55%, transparent); }
  .change-request-title { display: grid; min-width: 0; text-align: left; }
  .change-request-title strong, .change-request-title small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: calc(0.66rem * var(--type-scale)); }
  .change-request-title small { color: var(--muted-foreground); }
  .changes { min-height: 0; flex: 1; overflow: auto; padding: 0.35rem; }
  .repository-setup { display: grid; gap: 0.35rem; border-bottom: 1px solid var(--border); padding: 0.55rem 0.7rem; }
  .repository-setup > button { justify-self: start; border-radius: 0.35rem; color: var(--muted-foreground); font-size: calc(0.68rem * var(--type-scale)); }
  .repository-setup form { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 0.35rem; }
  .repository-setup input { min-width: 0; font-size: var(--panel-detail-font-size); }
  .repository-setup p { color: var(--muted-foreground); font-size: calc(0.62rem * var(--type-scale)); }
  .changes section { margin-bottom: 0.4rem; }
  h3 { display: flex; align-items: center; gap: 0.35rem; padding: 0.4rem; font-size: calc(0.72rem * var(--type-scale)); font-weight: 600; }
  h3 small { color: var(--muted-foreground); font-weight: 400; }
  .file-row { display: grid; grid-template-columns: 1.2rem minmax(0, 1fr) auto; align-items: center; gap: 0.35rem; border-radius: 0.4rem; padding: 0.2rem 0.35rem; }
  .file-row:hover { background: color-mix(in srgb, var(--accent) 55%, transparent); }
  .file-row code { color: var(--primary); font-size: calc(0.68rem * var(--type-scale)); }
  .file-row > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: "SF Mono", "SFMono-Regular", Consolas, monospace; font-size: calc(0.68rem * var(--type-scale)); }
  .file-link { min-width: 0; overflow: hidden; border-radius: 0.25rem; padding: 0.15rem 0.2rem; text-align: left; text-overflow: ellipsis; white-space: nowrap; font-family: "SF Mono", "SFMono-Regular", Consolas, monospace; font-size: calc(0.68rem * var(--type-scale)); }
  .file-link:hover, .file-link:focus-visible { background: var(--accent); color: var(--accent-foreground); }
  .file-row > .file-actions { display: flex; overflow: visible; font-family: inherit; }
  .commit { display: grid; gap: 0.4rem; border-top: 1px solid var(--border); padding: 0.6rem 0.7rem; }
  .commit label { font-size: calc(0.68rem * var(--type-scale)); font-weight: 600; }
  .commit textarea { min-height: 3.8rem; resize: vertical; font-size: var(--panel-font-size); }
  .commit button { display: inline-flex; align-items: center; justify-content: center; gap: 0.3rem; }
  .empty { padding: 1rem; text-align: center; color: var(--muted-foreground); font-size: calc(0.72rem * var(--type-scale)); }
  .empty.compact { padding: 0.4rem; }
  .source-panel > :global(.error) { border-bottom: 1px solid var(--border); padding: 0.5rem 0.7rem; font-size: calc(0.7rem * var(--type-scale)); }
</style>
