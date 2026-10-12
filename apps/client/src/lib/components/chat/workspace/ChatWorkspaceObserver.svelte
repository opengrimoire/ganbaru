<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import {
    unwatchChatWorkspace,
    watchChatWorkspace,
  } from "$lib/api/chat/workspace-observer";
  import type {
    ChatWorkspaceChangeBatch,
    ChatWorkspaceObserverStatusRead,
  } from "$lib/chat/contracts";
  import {
    publishChatWorkspaceChange,
    publishChatWorkspaceObserverStatus,
  } from "$lib/chat/workspace/observer-client";
  import { mergeWorkspaceChangeBatches } from "$lib/chat/workspace/change-model";
  import { parseChatWorkspaceChangeBatch } from "$lib/chat/validation";
  import { getChat } from "$lib/stores/chat.svelte";

  const chat = getChat();
  const WORKSPACE_CHANGE_EVENT = "chat://workspace-change";
  const UNAVAILABLE_POLL_DELAYS = [3_000, 5_000, 8_000, 12_000, 15_000] as const;
  let mounted = $state(false);
  let scopeRevision = 0;
  let activeStatus: ChatWorkspaceObserverStatusRead | null = null;
  let unlisten: UnlistenFn | null = null;
  let transition = Promise.resolve();
  let fallbackTimer: number | null = null;
  let fallbackIndex = 0;
  let listenerFailed = false;
  let listenerReady = $state(false);
  let pendingBatch: ChatWorkspaceChangeBatch | null = null;

  onMount(() => {
    mounted = true;
    void listen<unknown>(WORKSPACE_CHANGE_EVENT, (event) => {
      try {
        const batch = parseChatWorkspaceChangeBatch(event.payload);
        if (!activeStatus) {
          if (batch.workingFolderId !== chat.selectedWorkingFolderId
            || batch.executionEnvironmentId !== chat.selectedExecutionEnvironmentId) return;
          pendingBatch = mergeWorkspaceChangeBatches(pendingBatch, batch);
          return;
        }
        handleBatch(batch);
      } catch (error: unknown) {
        console.error("Invalid Chat workspace change notification", error);
      }
    }).then((dispose) => {
      if (mounted) {
        unlisten = dispose;
        listenerReady = true;
      }
      else dispose();
    }).catch((error: unknown) => {
      listenerFailed = true;
      listenerReady = true;
      console.error("Chat workspace change listener could not start", error);
      const status = activeStatus;
      if (status) {
        activeStatus = { ...status, mode: "unavailable", degradedReason: "listener_unavailable" };
        publishChatWorkspaceObserverStatus(activeStatus);
        scheduleUnavailableFallback(activeStatus);
      }
    });
  });

  $effect(() => {
    const routeMounted = mounted;
    const eventListenerReady = listenerReady;
    const workingFolderId = chat.selectedWorkingFolderId;
    const executionEnvironmentId = chat.selectedExecutionEnvironmentId;
    const revision = ++scopeRevision;
    transition = transition.then(async () => {
      await stopActiveObserver();
      if (!routeMounted || !eventListenerReady || !mounted
        || revision !== scopeRevision || !workingFolderId) return;
      try {
        const status = await watchChatWorkspace(workingFolderId, executionEnvironmentId);
        if (!mounted || revision !== scopeRevision) {
          await unwatchChatWorkspace(status.generation).catch(() => false);
          return;
        }
        const effectiveStatus: ChatWorkspaceObserverStatusRead = listenerFailed
          ? { ...status, mode: "unavailable", degradedReason: "listener_unavailable" }
          : status;
        activeStatus = effectiveStatus;
        publishChatWorkspaceObserverStatus(effectiveStatus);
        scheduleUnavailableFallback(effectiveStatus);
        const queued = pendingBatch;
        pendingBatch = null;
        if (queued) handleBatch(queued);
      } catch (error: unknown) {
        if (!mounted || revision !== scopeRevision) return;
        console.error("Chat workspace observer could not start", error);
        const unavailable: ChatWorkspaceObserverStatusRead = {
          workingFolderId,
          executionEnvironmentId,
          generation: 0,
          mode: "unavailable",
          degradedReason: "watch_unavailable",
        };
        activeStatus = unavailable;
        publishChatWorkspaceObserverStatus(unavailable);
        scheduleUnavailableFallback(unavailable);
      }
    });
  });

  onDestroy(() => {
    mounted = false;
    scopeRevision += 1;
    clearFallbackTimer();
    unlisten?.();
    unlisten = null;
    pendingBatch = null;
    transition = transition.then(stopActiveObserver);
  });

  function matchesActiveScope(batch: ChatWorkspaceChangeBatch): boolean {
    return activeStatus !== null
      && batch.generation === activeStatus.generation
      && batch.workingFolderId === activeStatus.workingFolderId
      && batch.executionEnvironmentId === activeStatus.executionEnvironmentId;
  }

  function handleBatch(batch: ChatWorkspaceChangeBatch): void {
    if (!matchesActiveScope(batch)) return;
    const status = activeStatus;
    if (!status) return;
    if (batch.degradedReason) {
      activeStatus = {
        ...status,
        mode: "unavailable",
        degradedReason: batch.degradedReason,
      };
      publishChatWorkspaceObserverStatus(activeStatus);
      scheduleUnavailableFallback(activeStatus);
    }
    publishChatWorkspaceChange(batch);
  }

  async function stopActiveObserver(): Promise<void> {
    clearFallbackTimer();
    const previous = activeStatus;
    activeStatus = null;
    publishChatWorkspaceObserverStatus(null);
    if (previous) await unwatchChatWorkspace(previous.generation).catch(() => false);
  }

  function scheduleUnavailableFallback(status: ChatWorkspaceObserverStatusRead): void {
    clearFallbackTimer();
    if (status.mode !== "unavailable") return;
    fallbackIndex = 0;
    const schedule = () => {
      const delay = UNAVAILABLE_POLL_DELAYS[Math.min(fallbackIndex, UNAVAILABLE_POLL_DELAYS.length - 1)];
      fallbackTimer = window.setTimeout(() => {
        if (!mounted
          || activeStatus?.generation !== status.generation
          || activeStatus.workingFolderId !== status.workingFolderId
          || activeStatus.executionEnvironmentId !== status.executionEnvironmentId) return;
        publishChatWorkspaceChange({
          workingFolderId: status.workingFolderId,
          executionEnvironmentId: status.executionEnvironmentId,
          generation: status.generation,
          relativePaths: [],
          affectedParentDirectories: [],
          renames: [],
          gitMetadataChanged: true,
          overflowed: true,
          degradedReason: status.degradedReason ?? "watch_unavailable",
        });
        fallbackIndex += 1;
        schedule();
      }, delay);
    };
    schedule();
  }

  function clearFallbackTimer(): void {
    if (fallbackTimer !== null) window.clearTimeout(fallbackTimer);
    fallbackTimer = null;
  }
</script>
