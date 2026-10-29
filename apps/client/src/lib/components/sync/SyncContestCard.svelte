<script lang="ts">
  import type { SyncContest, SyncContestCandidate } from "$lib/api/sync";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getSyncAuthorityState } from "$lib/stores/sync-authority.svelte";
  import {
    syncCandidateActions,
    syncCandidateDetails,
    syncContestCauseLine,
    syncContestDeviceName,
  } from "$lib/sync/contests";

  type ContestChoice = "yield" | "object" | "withdraw";

  let {
    buttonClass,
    contest,
  }: {
    buttonClass: string;
    contest: SyncContest;
  } = $props();

  const { t } = getLocalization();
  const authorityState = getSyncAuthorityState();
  let busy = $state(false);
  let message = $state("");
  let failed = $state(false);
  let pending = $state<{ choice: ContestChoice; candidate: SyncContestCandidate } | null>(null);

  const heldLine = $derived(
    contest.held.length > 0
      ? t("sync.contest.held", contest.held.map((device) => syncContestDeviceName(t, device)).join(", "))
      : "",
  );
  const kitOffered = $derived(contest.actions.some((action) => action.kind === "useKit"));
  const pendingName = $derived(pending ? syncContestDeviceName(t, pending.candidate.target) : "");

  function dialogText(choice: ContestChoice, name: string): { title: string; message: string; confirm: string } {
    switch (choice) {
      case "yield":
        return { title: t("sync.contest.yieldTitle"), message: t("sync.contest.yieldMessage", name), confirm: t("sync.contest.yieldTitle") };
      case "object":
        return { title: t("sync.contest.objectTitle"), message: t("sync.contest.objectMessage", name), confirm: t("sync.contest.objectTitle") };
      case "withdraw":
        return { title: t("sync.contest.withdrawTitle"), message: t("sync.contest.withdrawMessage", name), confirm: t("sync.contest.withdrawTitle") };
    }
  }

  /** Records the confirmed choice; a withdrawn objection is a yield to the same candidate. */
  async function confirmChoice(): Promise<void> {
    const current = pending;
    pending = null;
    if (!current || busy) return;
    busy = true;
    message = "";
    failed = false;
    try {
      if (current.choice === "object") await authorityState.objectContest(current.candidate.hash);
      else await authorityState.yieldContest(current.candidate.hash);
      message = t("sync.contest.done");
    } catch (cause: unknown) {
      console.warn("contest choice could not be recorded", cause);
      failed = true;
      message = t("sync.contest.failed");
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex min-w-0 flex-col gap-2">
  <div class="px-1">
    <div class="text-[0.866667rem] text-foreground">{t("sync.contest.heading")}</div>
    <Notice tone="warning" role="status" copy={false} class="mt-1 text-[0.8rem] leading-5" message={syncContestCauseLine(t, contest)} />
    <div class="mt-1 text-[0.8rem] text-muted-foreground">
      {t("sync.contest.anchor", syncContestDeviceName(t, contest.anchor))}
    </div>
  </div>
  {#each contest.candidates as candidate (candidate.hash)}
    {@const offered = syncCandidateActions(contest, candidate)}
    {@const name = syncContestDeviceName(t, candidate.target)}
    <div class="flex min-w-0 flex-col gap-1.5 px-1">
      <div class="min-w-0">
        <div class="truncate text-[0.866667rem] font-medium text-foreground">{t("sync.contest.candidate", name)}</div>
        {#each syncCandidateDetails(t, contest, candidate) as line (line)}
          <div class="text-[0.75rem] text-muted-foreground">{line}</div>
        {/each}
      </div>
      {#if offered.yieldTo || offered.object || offered.withdrawObjection}
        <div class="flex flex-wrap gap-2">
          {#if offered.yieldTo}
            <button type="button" class={buttonClass} disabled={busy} onclick={() => { pending = { choice: "yield", candidate }; }}>
              {t("sync.contest.yield", name)}
            </button>
          {/if}
          {#if offered.withdrawObjection}
            <button type="button" class={buttonClass} disabled={busy} onclick={() => { pending = { choice: "withdraw", candidate }; }}>
              {t("sync.contest.withdraw", name)}
            </button>
          {/if}
          {#if offered.object}
            <button type="button" class={buttonClass} disabled={busy} onclick={() => { pending = { choice: "object", candidate }; }}>
              {t("sync.contest.object", name)}
            </button>
          {/if}
        </div>
      {/if}
    </div>
  {/each}
  {#if heldLine}
    <p class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{heldLine}</p>
  {/if}
  {#if !kitOffered}
    <p class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{t("sync.contest.kitHint")}</p>
  {/if}
  {#if message}
    {#if failed}
      <Notice class="px-1 text-[0.8rem] leading-5" {message} copy={false} />
    {:else}
      <p role="status" class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{message}</p>
    {/if}
  {/if}
</div>

{#if pending}
  {@const text = dialogText(pending.choice, pendingName)}
  <ConfirmDialog
    title={text.title}
    message={text.message}
    confirmLabel={text.confirm}
    cancelLabel={t("common.cancel")}
    onConfirm={() => void confirmChoice()}
    onCancel={() => { pending = null; }}
  />
{/if}
