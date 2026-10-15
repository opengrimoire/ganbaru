<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ShieldAlert from "@lucide/svelte/icons/shield-alert";
  import * as chatApi from "$lib/api/chat";
  import type { ChatPendingRequestRead, UserInputAnswer } from "$lib/chat/contracts";
  import { parseApprovalChoices, parseUserInputQuestions, validateUserInputAnswers } from "$lib/chat/composer/model";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import { formatList, formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChat } from "$lib/stores/chat.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { OPENING_MAY_FOCUS_TEXT } from "$lib/mobile/keyboard";

  const { pending } = $props<{ pending: ChatPendingRequestRead }>();
  const localization = getLocalization();
  const { t } = localization;
  const chat = useChat();
  let requestId = $state("");
  let step = $state(0);
  let selected = $state<Record<string, string[]>>({});
  let freeForm = $state<Record<string, string>>({});
  let resolving = $state(false);
  let error = $state<string | null>(null);
  let answerDraftReady = $state(false);
  let answerDraftRequest = 0;
  let answerSaveTimer: ReturnType<typeof setTimeout> | null = null;
  let panel: HTMLElement | undefined = $state();
  const approvalChoices = $derived(parseApprovalChoices(pending.allowedDecisions));
  const questions = $derived(pending.requestKind === "user_input" ? parseUserInputQuestions(pending.safeDisplay) : []);
  const question = $derived(questions[step] ?? null);
  const approvalDisplay = $derived(readApprovalDisplay(pending));
  const answerPreview = $derived(question ? answerLabel(question.id) : "");

  $effect(() => {
    if (!pending.id || pending.id === requestId) return;
    requestId = pending.id;
    step = 0;
    selected = {};
    freeForm = {};
    error = null;
    resolving = false;
    answerDraftReady = false;
    if (answerSaveTimer !== null) clearTimeout(answerSaveTimer);
    if (pending.requestKind === "user_input") void loadAnswerDraft(pending.id);
    void tick().then(() => {
      // On a phone the panel takes focus instead of its first answer field, so a new request does not open the keyboard.
      if (pending.requestKind === "user_input" && OPENING_MAY_FOCUS_TEXT) panel?.querySelector<HTMLElement>("input, textarea")?.focus();
      else panel?.focus();
    });
  });

  onDestroy(() => {
    if (answerSaveTimer !== null) clearTimeout(answerSaveTimer);
    if (answerDraftReady && pending.requestKind === "user_input") void saveAnswerDraft();
  });

  async function resolveApproval(index: number): Promise<void> {
    const choice = approvalChoices[index];
    if (!choice || resolving) return;
    resolving = true;
    error = null;
    try {
      await chat.resolveApproval({ kind: choice.decisionKind, providerOptionId: choice.id, updatedToolInput: null });
    } catch (cause: unknown) {
      error = chatErrorMessage(cause);
      resolving = false;
    }
  }

  function toggleOption(questionId: string, optionId: string, multiple: boolean): void {
    const current = selected[questionId] ?? [];
    selected = {
      ...selected,
      [questionId]: multiple
        ? current.includes(optionId) ? current.filter((id) => id !== optionId) : [...current, optionId]
        : [optionId],
    };
    scheduleAnswerDraftSave();
  }

  async function loadAnswerDraft(id: string): Promise<void> {
    const request = ++answerDraftRequest;
    try {
      const draft = await chatApi.readChatUserInputDraft(id);
      if (request !== answerDraftRequest || pending.id !== id) return;
      if (draft && Array.isArray(draft.answers.value)) {
        const nextSelected: Record<string, string[]> = {};
        const nextFreeForm: Record<string, string> = {};
        for (const value of draft.answers.value) {
          if (typeof value !== "object" || value === null || Array.isArray(value)) continue;
          if (typeof value.questionId !== "string") continue;
          nextSelected[value.questionId] = Array.isArray(value.selectedOptionIds)
            ? value.selectedOptionIds.filter((optionId): optionId is string => typeof optionId === "string")
            : [];
          if (typeof value.freeFormText === "string") nextFreeForm[value.questionId] = value.freeFormText;
        }
        selected = nextSelected;
        freeForm = nextFreeForm;
      }
      answerDraftReady = true;
    } catch (cause: unknown) {
      if (request === answerDraftRequest) error = chatErrorMessage(cause);
    }
  }

  function answers(): UserInputAnswer[] {
    return questions.map((entry) => ({
      questionId: entry.id,
      selectedOptionIds: [...(selected[entry.id] ?? [])],
      freeFormText: freeForm[entry.id]?.trim() || null,
    }));
  }

  function scheduleAnswerDraftSave(): void {
    if (!answerDraftReady || pending.requestKind !== "user_input") return;
    if (answerSaveTimer !== null) clearTimeout(answerSaveTimer);
    answerSaveTimer = setTimeout(() => { void saveAnswerDraft(); }, 300);
  }

  async function saveAnswerDraft(): Promise<void> {
    if (!answerDraftReady || pending.requestKind !== "user_input") return;
    if (answerSaveTimer !== null) clearTimeout(answerSaveTimer);
    answerSaveTimer = null;
    try {
      await chatApi.saveChatUserInputDraft(pending.id, {
        schemaVersion: 1,
        value: answers().map((answer) => ({
          questionId: answer.questionId,
          selectedOptionIds: answer.selectedOptionIds,
          freeFormText: answer.freeFormText,
        })),
      });
    } catch (cause: unknown) {
      error = chatErrorMessage(cause);
    }
  }

  function answerLabel(questionId: string): string {
    const current = questions.find((entry) => entry.id === questionId);
    if (!current) return "";
    const labels = (selected[questionId] ?? []).map((id) => current.options.find((option) => option.id === id)?.label ?? id);
    const text = freeForm[questionId]?.trim();
    return formatList(localization.locale, [...labels, ...(text ? [text] : [])]);
  }

  function isAnswerValid(index: number): boolean {
    const current = questions[index];
    if (!current || !current.required) return true;
    return (selected[current.id]?.length ?? 0) > 0 || Boolean(freeForm[current.id]?.trim());
  }

  async function advanceOrSubmit(): Promise<void> {
    if (!question || resolving) return;
    if (!isAnswerValid(step)) {
      error = t("chat.composer.requiredAnswer");
      void tick().then(() => panel?.querySelector<HTMLElement>("input, textarea")?.focus());
      return;
    }
    if (step < questions.length - 1) {
      step += 1;
      error = null;
      return;
    }
    const response = answers();
    const invalid = validateUserInputAnswers(questions, response);
    if (invalid.length > 0) {
      step = Math.max(0, questions.findIndex((entry) => entry.id === invalid[0]));
      error = t("chat.composer.requiredAnswer");
      return;
    }
    resolving = true;
    error = null;
    try {
      await chat.resolveUserInput(response);
    } catch (cause: unknown) {
      error = chatErrorMessage(cause);
      resolving = false;
    }
  }

  function readApprovalDisplay(request: ChatPendingRequestRead): { title: string; detail: string | null; payload: string | null } {
    const value = request.safeDisplay.value;
    if (typeof value !== "object" || value === null || Array.isArray(value)) {
      return { title: t("chat.composer.approval"), detail: null, payload: null };
    }
    return {
      title: typeof value.title === "string" ? value.title : t("chat.composer.approval"),
      detail: typeof value.detail === "string" ? value.detail : null,
      payload: value.payload === undefined ? null : JSON.stringify(value.payload, null, 2),
    };
  }
</script>

<section bind:this={panel} class="chat-request-panel" tabindex="-1">
  {#if pending.requestKind === "approval"}
    <header><ShieldAlert size={16} /><div><strong>{approvalDisplay.title}</strong>{#if approvalDisplay.detail}<p>{approvalDisplay.detail}</p>{/if}</div></header>
    {#if approvalDisplay.payload}<details><summary>{t("chat.composer.details")}<ChevronDown size={13} /></summary><pre>{approvalDisplay.payload}</pre></details>{/if}
    <div class="request-actions">{#each approvalChoices as choice, index}<button type="button" disabled={resolving} class:danger={choice.decisionKind === "allow_session"} title={choice.description ?? undefined} onclick={() => void resolveApproval(index)}>{choice.label}</button>{/each}</div>
  {:else if question}
    <header><div><strong>{t("chat.composer.question")}</strong><p>{t("chat.composer.questionProgress", formatNumber(localization.locale, step + 1), formatNumber(localization.locale, questions.length))}</p></div></header>
    {#if question.header}<small>{question.header}</small>{/if}<p class="question-text">{question.question}</p>
    <div class="question-options">{#each question.options as option}<label>{#if question.multiple}<Checkbox name={`question-${question.id}`} checked={(selected[question.id] ?? []).includes(option.id)} onChange={() => toggleOption(question.id, option.id, question.multiple)} />{:else}<input type="radio" name={`question-${question.id}`} checked={(selected[question.id] ?? []).includes(option.id)} onchange={() => toggleOption(question.id, option.id, question.multiple)} />{/if}<span><strong>{option.label}</strong>{#if option.description}<small>{option.description}</small>{/if}</span></label>{/each}</div>
    {#if question.freeFormAllowed}<label class="free-form"><span>{t("chat.composer.freeForm")}</span><textarea class="field" value={freeForm[question.id] ?? ""} oninput={(event) => { freeForm = { ...freeForm, [question.id]: event.currentTarget.value }; scheduleAnswerDraftSave(); }}></textarea></label>{/if}
    {#if answerPreview}<div class="answer-preview"><span>{t("chat.composer.responsePreview")}</span><p>{answerPreview}</p></div>{/if}
    <div class="request-actions">{#if step > 0}<button type="button" disabled={resolving} onclick={() => { step -= 1; error = null; }}>{t("chat.composer.back")}</button>{/if}<button type="button" disabled={resolving} onclick={() => void advanceOrSubmit()}>{step < questions.length - 1 ? t("chat.save") : t("chat.composer.submitAnswer")}</button></div>
  {/if}
  {#if resolving}<p class="resolving"><LoaderCircle size={13} class="animate-spin" />{t("chat.composer.resolving")}</p>{/if}
  {#if error}<Notice class="text-xs" message={error} />{/if}
</section>

<style>
  .chat-request-panel { display: grid; gap: 0.65rem; border: 1px solid color-mix(in oklab, var(--status-tentative) 50%, var(--border)); border-radius: 0.5rem; background: var(--background); padding: 0.7rem; }
  header { display: flex; align-items: flex-start; gap: 0.5rem; }
  header div { min-width: 0; flex: 1; }
  header strong { font-size: calc(0.8rem * var(--type-scale)); }
  header p, .chat-request-panel > small { color: var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); }
  details summary { display: inline-flex; cursor: pointer; align-items: center; gap: 0.25rem; color: var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); }
  pre { margin-top: 0.4rem; max-height: 10rem; overflow: auto; white-space: pre-wrap; border-radius: 0.35rem; background: var(--muted); padding: 0.5rem; font-size: calc(0.666667rem * var(--type-scale)); }
  .request-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 0.4rem; }
  .request-actions button { min-height: 2rem; border: 1px solid var(--border); border-radius: 0.35rem; padding: 0.3rem 0.6rem; font-size: calc(0.733333rem * var(--type-scale)); }
  .request-actions button:hover { background: var(--accent); }
  .request-actions .danger { border-color: color-mix(in oklab, var(--destructive) 50%, var(--border)); }
  .question-text { font-size: calc(0.8rem * var(--type-scale)); }
  .question-options { display: grid; gap: 0.35rem; }
  .question-options label { display: flex; align-items: flex-start; gap: 0.45rem; border: 1px solid var(--border); border-radius: 0.4rem; padding: 0.45rem; }
  .question-options span, .question-options strong, .question-options small { display: block; }
  .question-options strong { font-size: calc(0.733333rem * var(--type-scale)); }
  .question-options small { color: var(--muted-foreground); font-size: calc(0.666667rem * var(--type-scale)); }
  .free-form { display: grid; gap: 0.25rem; color: var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); }
  .free-form textarea { min-height: 4rem; resize: vertical; color: var(--foreground); }
  .answer-preview { border-left: 2px solid var(--primary); padding-left: 0.5rem; }
  .answer-preview span { color: var(--muted-foreground); font-size: calc(0.666667rem * var(--type-scale)); }
  .answer-preview p { white-space: pre-wrap; font-size: calc(0.733333rem * var(--type-scale)); }
  .resolving { display: inline-flex; align-items: center; gap: 0.35rem; color: var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); }
</style>
