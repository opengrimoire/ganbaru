<script lang="ts">
  import KeyRound from "@lucide/svelte/icons/key-round";
  import { declineChatSecret, listChatSecretRequests, provideChatSecret } from "$lib/api/chat";
  import type { ChatReplyThreadId, ChatSecretRequestRead } from "$lib/chat/contracts";
  import { chatErrorDetail, chatErrorMessage } from "$lib/chat/error-presentation";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  /** Refusal codes the backend returns as the error message when it refuses an answer. */
  const SECRET_REFUSAL_CODES = [
    "secret_request_not_pending",
    "secret_request_expired",
    "secret_value_too_short",
    "secret_value_too_large",
    "secret_value_matches_request",
  ] as const;
  type SecretRefusalCode = (typeof SECRET_REFUSAL_CODES)[number];
  /** Refusals after which the request is no longer open, so the list is read again instead. */
  const CLOSED_REQUEST_CODES: readonly SecretRefusalCode[] = ["secret_request_not_pending", "secret_request_expired"];

  /**
   * Lists the open secret requests of a reply thread with a private field for each value. A value never
   * leaves the field except in the provide call, and the field clears after every answer.
   */
  let {
    replyThreadId,
    refreshKey,
    agentName,
  }: {
    replyThreadId: ChatReplyThreadId;
    /** Changes whenever the reply thread was read again, so the open requests are read again too. */
    refreshKey: unknown;
    /** The agent the thread's work belongs to, or `null` when it is unknown. */
    agentName: string | null;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  let requests = $state<ChatSecretRequestRead[]>([]);
  let values = $state<Record<string, string>>({});
  let busyRequestId = $state<string | null>(null);
  let error = $state<string | null>(null);
  let listRequest = 0;
  /** The reply thread whose requests and typed values are shown. */
  let shownThreadId: ChatReplyThreadId | null = null;

  function refusalCode(cause: unknown): SecretRefusalCode | null {
    const message = chatErrorDetail(cause);
    return SECRET_REFUSAL_CODES.find((code) => code === message) ?? null;
  }

  function errorText(cause: unknown): string {
    const code = refusalCode(cause);
    return code ? t(`chat.secrets.refusals.${code}`) : chatErrorMessage(cause, t("chatErrors.actions.secretAction"));
  }

  function stillOpen(request: ChatSecretRequestRead, now: number): boolean {
    return Date.parse(request.expiresAt) > now;
  }

  async function loadRequests(threadId: ChatReplyThreadId): Promise<void> {
    const request = ++listRequest;
    try {
      const listed = await listChatSecretRequests(threadId);
      if (request !== listRequest) return;
      const now = Date.now();
      requests = listed.filter((entry) => stillOpen(entry, now));
      const openIds = new Set(requests.map((entry) => entry.id));
      values = Object.fromEntries(Object.entries(values).filter(([id]) => openIds.has(id)));
    } catch (cause: unknown) {
      if (request === listRequest) error = errorText(cause);
    }
  }

  async function answer(request: ChatSecretRequestRead, provide: boolean): Promise<void> {
    if (busyRequestId) return;
    const threadId = replyThreadId;
    const value = values[request.id] ?? "";
    values = { ...values, [request.id]: "" };
    busyRequestId = request.id;
    error = null;
    try {
      if (provide) await provideChatSecret(request.id, value);
      else await declineChatSecret(request.id);
      requests = requests.filter((entry) => entry.id !== request.id);
    } catch (cause: unknown) {
      const code = refusalCode(cause);
      if (code && CLOSED_REQUEST_CODES.includes(code)) {
        await loadRequests(replyThreadId);
      } else if (threadId === replyThreadId) {
        // An answer that fails after another reply thread opened does not report into that thread.
        error = errorText(cause);
      }
    } finally {
      busyRequestId = null;
    }
  }

  $effect(() => {
    const threadId = replyThreadId;
    void refreshKey;
    error = null;
    if (threadId !== shownThreadId) {
      // Another thread's requests and typed values never show under this one, even when its list fails to load.
      shownThreadId = threadId;
      requests = [];
      values = {};
    }
    void loadRequests(threadId);
  });

  $effect(() => {
    if (requests.length === 0) return;
    const nextExpiry = Math.min(...requests.map((request) => Date.parse(request.expiresAt)));
    const timer = setTimeout(() => {
      const now = Date.now();
      requests = requests.filter((request) => stillOpen(request, now));
    }, Math.max(0, nextExpiry - Date.now()));
    return () => clearTimeout(timer);
  });
</script>

{#if requests.length > 0 || error}
  <div class="secret-requests" data-secret-requests>
    {#each requests as request (request.id)}
      <form
        class="secret-request"
        data-secret-request={request.id}
        aria-busy={busyRequestId === request.id || undefined}
        onsubmit={(event) => { event.preventDefault(); void answer(request, true); }}
      >
        <header>
          <KeyRound size={14} />
          <div>
            <strong>{agentName ? t("chat.secrets.title", agentName) : t("chat.secrets.titleWithoutName")}</strong>
            <p class="purpose">{request.purpose}</p>
          </div>
        </header>
        <label class="value-field">
          <span>{t("chat.secrets.valueLabel", request.label)}</span>
          <input
            class="field"
            type="password"
            autocomplete="off"
            spellcheck="false"
            value={values[request.id] ?? ""}
            disabled={busyRequestId !== null}
            oninput={(event) => { values = { ...values, [request.id]: event.currentTarget.value }; }}
          />
        </label>
        <small>{t("chat.secrets.privacy")}</small>
        <small>{t("chat.secrets.expires", formatDateTime(localization.locale, Date.parse(request.expiresAt), { timeStyle: "short" }))}</small>
        <div class="secret-actions">
          <button type="button" disabled={busyRequestId !== null} onclick={() => void answer(request, false)}>{t("chat.secrets.decline")}</button>
          <button type="submit" disabled={busyRequestId !== null || !(values[request.id] ?? "")}>{t("chat.secrets.provide")}</button>
        </div>
      </form>
    {/each}
    {#if error}<Notice class="text-xs" message={error} onDismiss={() => { error = null; }} />{/if}
  </div>
{/if}

<style>
  .secret-requests { display:grid; gap:0.5rem; }
  .secret-request { display:grid; gap:0.45rem; border:1px solid color-mix(in oklab, var(--status-tentative) 50%, var(--border)); border-radius:0.5rem; background:var(--background); padding:0.7rem; }
  header { display:flex; align-items:flex-start; gap:0.5rem; }
  header :global(svg) { flex:0 0 auto; margin-top:0.15rem; }
  header div { min-width:0; flex:1; }
  header strong { font-size: calc(0.8rem * var(--type-scale)); }
  .purpose { overflow-wrap:anywhere; white-space:pre-wrap; font-size: calc(0.733333rem * var(--type-scale)); }
  .value-field { display:grid; gap:0.25rem; color:var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); }
  .value-field input { color:var(--foreground); }
  small { color:var(--muted-foreground); font-size: calc(0.666667rem * var(--type-scale)); }
  .secret-actions { display:flex; flex-wrap:wrap; justify-content:flex-end; gap:0.4rem; }
  .secret-actions button { min-height:2rem; border:1px solid var(--border); border-radius:0.35rem; padding:0.3rem 0.6rem; font-size: calc(0.733333rem * var(--type-scale)); }
  .secret-actions button:hover:not(:disabled) { background:var(--accent); }
  .secret-actions button:disabled { cursor:not-allowed; opacity:0.5; }
  @media (pointer:coarse) { .secret-actions button { min-height:2.75rem; } }
</style>
