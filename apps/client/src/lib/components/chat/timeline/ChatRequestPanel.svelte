<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import FileText from "@lucide/svelte/icons/file-text";
  import Image from "@lucide/svelte/icons/image";
  import ImagePlus from "@lucide/svelte/icons/image-plus";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ShieldAlert from "@lucide/svelte/icons/shield-alert";
  import * as chatApi from "$lib/api/chat";
  import ChatApprovalSubject from "./ChatApprovalSubject.svelte";
  import { rememberedApprovalDescription } from "$lib/chat/agents/remembered-approvals";
  import type { ChatAttachmentRead, ChatPendingRequestRead, SessionPermissionGrant, UserInputAnswer } from "$lib/chat/contracts";
  import { CHAT_IMAGE_LIMIT, clipboardImageFiles, parseApprovalChoices, parseApprovalDisplay, parseUserInputQuestions, validateImageFiles, validateUserInputAnswers } from "$lib/chat/composer/model";
  import { MAX_TEXT_SNIPPET_BYTES, classifyPastedText, pastedTextLineCount } from "$lib/chat/composer/paste";
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
  /** Attachments per question, kept in the answer draft and sent with the answer. */
  let attached = $state<Record<string, ChatAttachmentRead[]>>({});
  let attaching = $state(false);
  let resolving = $state(false);
  let error = $state<string | null>(null);
  let answerDraftReady = $state(false);
  let answerDraftRequest = 0;
  let answerSaveTimer: ReturnType<typeof setTimeout> | null = null;
  let panel: HTMLElement | undefined = $state();
  const approvalDisplay = $derived(parseApprovalDisplay(pending.safeDisplay));
  // Allow for session is withheld when its grants cannot be shown, so nothing is forwarded unseen.
  const approvalChoices = $derived(parseApprovalChoices(pending.allowedDecisions)
    .filter((choice) => choice.decisionKind !== "allow_session" || approvalDisplay.sessionGrants !== null));
  /** "Allow and don't ask again" sits beside allow once, only when Rust offers a rule to remember. */
  const rememberDescription = $derived(pending.rememberable && approvalChoices.some((choice) => choice.decisionKind === "allow_once") ? rememberedApprovalDescription(t, pending.rememberable) : null);
  const sessionGrants = $derived(approvalChoices.some((choice) => choice.decisionKind === "allow_session") ? approvalDisplay.sessionGrants ?? [] : []);
  const questions = $derived(pending.requestKind === "user_input" ? parseUserInputQuestions(pending.safeDisplay) : []);
  const question = $derived(questions[step] ?? null);
  const answerPreview = $derived(question ? answerLabel(question.id) : "");
  const workingFolderId = $derived(chat.selectedThread?.workingFolderId ?? null);
  const questionAttachments = $derived(question ? attached[question.id] ?? [] : []);

  $effect(() => {
    if (!pending.id || pending.id === requestId) return;
    requestId = pending.id;
    step = 0;
    selected = {};
    freeForm = {};
    attached = {};
    attaching = false;
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

  async function resolveApproval(index: number, remember = false): Promise<void> {
    const choice = approvalChoices[index];
    if (!choice || resolving) return;
    resolving = true;
    error = null;
    try {
      await chat.resolveApproval(pending, { kind: choice.decisionKind, providerOptionId: choice.id, updatedToolInput: null }, remember);
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.requestAction"));
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
        const draftAttachmentIds: Record<string, string[]> = {};
        for (const value of draft.answers.value) {
          if (typeof value !== "object" || value === null || Array.isArray(value)) continue;
          if (typeof value.questionId !== "string") continue;
          nextSelected[value.questionId] = Array.isArray(value.selectedOptionIds)
            ? value.selectedOptionIds.filter((optionId): optionId is string => typeof optionId === "string")
            : [];
          if (typeof value.freeFormText === "string") nextFreeForm[value.questionId] = value.freeFormText;
          if (Array.isArray(value.attachmentIds)) {
            draftAttachmentIds[value.questionId] = value.attachmentIds
              .filter((attachmentId): attachmentId is string => typeof attachmentId === "string")
              .slice(0, CHAT_IMAGE_LIMIT);
          }
        }
        selected = nextSelected;
        freeForm = nextFreeForm;
        if (Object.values(draftAttachmentIds).some((ids) => ids.length > 0)) {
          const nextAttached = await readDraftAttachments(draftAttachmentIds);
          if (request !== answerDraftRequest || pending.id !== id) return;
          attached = nextAttached;
        }
      }
      answerDraftReady = true;
    } catch (cause: unknown) {
      if (request === answerDraftRequest) error = chatErrorMessage(cause, t("chatErrors.actions.requestAction"));
    }
  }

  function answers(): UserInputAnswer[] {
    return questions.map((entry) => ({
      questionId: entry.id,
      selectedOptionIds: [...(selected[entry.id] ?? [])],
      freeFormText: freeForm[entry.id]?.trim() || null,
      attachmentIds: (attached[entry.id] ?? []).map((attachment) => attachment.id),
    }));
  }

  /**
   * Reads the attachments a saved answer draft names, keeping each question's order and
   * dropping attachments that no longer exist.
   */
  async function readDraftAttachments(idsByQuestion: Record<string, string[]>): Promise<Record<string, ChatAttachmentRead[]>> {
    const ids = [...new Set(Object.values(idsByQuestion).flat())];
    if (ids.length === 0 || !workingFolderId) return {};
    const reads = new Map((await chatApi.readChatAttachments(workingFolderId, ids)).map((attachment) => [attachment.id, attachment]));
    return Object.fromEntries(Object.entries(idsByQuestion).map(([questionId, questionIds]) => [
      questionId,
      questionIds.flatMap((attachmentId) => reads.get(attachmentId) ?? []),
    ]));
  }

  /** Runs an attachment import for the current question, refusing it when no folder or room is left. */
  async function attachToQuestion(questionId: string, incoming: number, load: (folder: string, room: number) => Promise<ChatAttachmentRead[]>): Promise<void> {
    if (attaching || resolving) return;
    if (!workingFolderId) { error = t("chat.composer.answerAttachments.unavailable"); return; }
    const room = CHAT_IMAGE_LIMIT - (attached[questionId]?.length ?? 0);
    if (room <= 0 || incoming > room) { error = t("chat.composer.answerAttachments.limit", CHAT_IMAGE_LIMIT); return; }
    attaching = true;
    error = null;
    try {
      const imported = await load(workingFolderId, room);
      if (pending.id !== requestId) return;
      attached = { ...attached, [questionId]: [...(attached[questionId] ?? []), ...imported].slice(0, CHAT_IMAGE_LIMIT) };
      scheduleAnswerDraftSave();
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.requestAction"));
    } finally {
      attaching = false;
    }
  }

  function pickAnswerImages(questionId: string): void {
    void attachToQuestion(questionId, 1, (folder, room) => chatApi.pickChatImages(
      folder,
      Array.from({ length: room }, () => crypto.randomUUID()),
      t("chat.composer.imagePickerTitle"),
    ));
  }

  /** Attaches pasted images, and pasted text too long to read inline, to the question's answer. */
  function handleAnswerPaste(questionId: string, event: ClipboardEvent): void {
    const files = clipboardImageFiles(event.clipboardData);
    if (files.length > 0) {
      event.preventDefault();
      const invalid = validateImageFiles(files, attached[questionId]?.length ?? 0);
      if (invalid) { error = invalid; return; }
      void attachToQuestion(questionId, files.length, async (folder) => {
        const imported: ChatAttachmentRead[] = [];
        for (const file of files) {
          imported.push(await chatApi.importChatImage(folder, crypto.randomUUID(), file.name, new Uint8Array(await file.arrayBuffer())));
        }
        return imported;
      });
      return;
    }
    const text = event.clipboardData?.getData("text/plain") ?? "";
    const placement = classifyPastedText(text);
    if (placement === "inline") return;
    event.preventDefault();
    if (placement === "too_large") {
      error = t("chat.composer.paste.tooLarge", formatNumber(localization.locale, MAX_TEXT_SNIPPET_BYTES / (1024 * 1024), { style: "unit", unit: "megabyte" }));
      return;
    }
    void attachToQuestion(questionId, 1, async (folder) => [
      await chatApi.importChatTextSnippet(folder, crypto.randomUUID(), t("chat.composer.paste.snippetName", pastedTextLineCount(text)), text),
    ]);
  }

  function removeAnswerAttachment(questionId: string, attachmentId: string): void {
    attached = { ...attached, [questionId]: (attached[questionId] ?? []).filter((attachment) => attachment.id !== attachmentId) };
    scheduleAnswerDraftSave();
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
          attachmentIds: answer.attachmentIds ?? [],
        })),
      });
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.requestAction"));
    }
  }

  function answerLabel(questionId: string): string {
    const current = questions.find((entry) => entry.id === questionId);
    if (!current) return "";
    const labels = (selected[questionId] ?? []).map((id) => current.options.find((option) => option.id === id)?.label ?? id);
    const text = freeForm[questionId]?.trim();
    const files = (attached[questionId] ?? []).map((attachment) => attachment.originalDisplayName);
    return formatList(localization.locale, [...labels, ...(text ? [text] : []), ...files]);
  }

  function isAnswerValid(index: number): boolean {
    const current = questions[index];
    if (!current || !current.required) return true;
    return (selected[current.id]?.length ?? 0) > 0 || Boolean(freeForm[current.id]?.trim()) || (attached[current.id]?.length ?? 0) > 0;
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
      await chat.resolveUserInput(pending, response);
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.requestAction"));
      resolving = false;
    }
  }

  function grantLabel(grant: SessionPermissionGrant): string {
    switch (grant.kind) {
      case "allow_rule": return grant.ruleContent === null
        ? t("chat.composer.sessionGrantTool", grant.toolName)
        : t("chat.composer.sessionGrantRule", grant.toolName, grant.ruleContent);
      case "directory": return t("chat.composer.sessionGrantDirectory", grant.path);
      case "accept_edits": return t("chat.composer.sessionGrantAcceptEdits");
    }
  }
</script>

<section bind:this={panel} class="chat-request-panel" tabindex="-1">
  {#if pending.requestKind === "approval"}
    <header><ShieldAlert size={16} /><div><strong>{approvalDisplay.title ?? t("chat.composer.approval")}</strong>{#if approvalDisplay.detail}<p>{approvalDisplay.detail}</p>{/if}</div></header>
    {#if approvalDisplay.subject}<ChatApprovalSubject subject={approvalDisplay.subject} />{/if}
    {#if sessionGrants.length > 0}
      <div class="session-grants" data-testid="session-grants"><small>{t("chat.composer.sessionGrantsTitle")}</small><ul>{#each sessionGrants as grant}<li>{grantLabel(grant)}</li>{/each}</ul></div>
    {/if}
    {#if approvalDisplay.payload}<details><summary>{t("chat.composer.details")}<ChevronDown size={13} /></summary><pre>{approvalDisplay.payload}</pre></details>{/if}
    {#if rememberDescription}<p id={`remember-hint-${pending.id}`} class="remember-hint" data-testid="approval-remember-hint">{rememberDescription}</p>{/if}
    <div class="request-actions">{#each approvalChoices as choice, index}<button type="button" disabled={resolving} class:danger={choice.decisionKind === "allow_session"} title={choice.description ?? undefined} onclick={() => void resolveApproval(index)}>{choice.label}</button>{#if choice.decisionKind === "allow_once" && rememberDescription}<button type="button" disabled={resolving} aria-describedby={`remember-hint-${pending.id}`} data-testid="approval-remember" onclick={() => void resolveApproval(index, true)}>{t("chat.composer.allowAlways")}</button>{/if}{/each}</div>
  {:else if question}
    <header><div><strong>{t("chat.composer.question")}</strong><p>{t("chat.composer.questionProgress", formatNumber(localization.locale, step + 1), formatNumber(localization.locale, questions.length))}</p></div></header>
    {#if question.header}<small>{question.header}</small>{/if}<p class="question-text">{question.question}</p>
    <div class="question-options">{#each question.options as option}<label>{#if question.multiple}<Checkbox name={`question-${question.id}`} checked={(selected[question.id] ?? []).includes(option.id)} onChange={() => toggleOption(question.id, option.id, question.multiple)} />{:else}<input type="radio" name={`question-${question.id}`} checked={(selected[question.id] ?? []).includes(option.id)} onchange={() => toggleOption(question.id, option.id, question.multiple)} />{/if}<span><strong>{option.label}</strong>{#if option.description}<small>{option.description}</small>{/if}</span></label>{/each}</div>
    {#if question.freeFormAllowed}
      {@const questionId = question.id}
      <label class="free-form"><span>{t("chat.composer.freeForm")}</span><textarea class="field" value={freeForm[questionId] ?? ""} onpaste={(event) => handleAnswerPaste(questionId, event)} oninput={(event) => { freeForm = { ...freeForm, [questionId]: event.currentTarget.value }; scheduleAnswerDraftSave(); }}></textarea></label>
      <div class="answer-attachments" data-chat-answer-attachments>
        {#each questionAttachments as attachment (attachment.id)}
          <button type="button" class="answer-attachment" disabled={resolving} aria-label={t("chat.composer.removeAttachment", attachment.originalDisplayName)} onclick={() => removeAnswerAttachment(questionId, attachment.id)}>{#if attachment.kind === "text_snippet"}<FileText size={12} />{:else}<Image size={12} />{/if}<span class="answer-attachment-label">{attachment.originalDisplayName}</span><span aria-hidden="true">×</span></button>
        {/each}
        <button type="button" class="attach-answer" disabled={resolving || attaching || questionAttachments.length >= CHAT_IMAGE_LIMIT} onclick={() => pickAnswerImages(questionId)}>{#if attaching}<LoaderCircle size={12} class="animate-spin" />{:else}<ImagePlus size={12} />{/if}{t("chat.composer.answerAttachments.attach")}</button>
      </div>
    {/if}
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
  .session-grants { display: grid; gap: 0.2rem; }
  .session-grants small { color: var(--muted-foreground); font-size: calc(0.666667rem * var(--type-scale)); }
  .session-grants ul { list-style: disc; padding-left: 1.1rem; font-size: calc(0.7rem * var(--type-scale)); overflow-wrap: anywhere; }
  .remember-hint { color: var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); overflow-wrap: anywhere; }
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
  .answer-attachments { display: flex; flex-wrap: wrap; gap: 0.3rem; }
  .answer-attachments button { display: flex; max-width: 14rem; align-items: center; gap: 0.25rem; border-radius: 999px; padding: 0.22rem 0.45rem; font-size: calc(0.68rem * var(--type-scale)); }
  .answer-attachment { background: var(--accent); }
  .answer-attachment span { color: var(--muted-foreground); }
  .answer-attachment .answer-attachment-label { overflow: hidden; color: inherit; text-overflow: ellipsis; white-space: nowrap; }
  .attach-answer { border: 1px solid var(--border); color: var(--muted-foreground); }
  .attach-answer:hover:not(:disabled) { background: var(--accent); color: var(--foreground); }
  .answer-preview { border-left: 2px solid var(--primary); padding-left: 0.5rem; }
  .answer-preview span { color: var(--muted-foreground); font-size: calc(0.666667rem * var(--type-scale)); }
  .answer-preview p { white-space: pre-wrap; font-size: calc(0.733333rem * var(--type-scale)); }
  .resolving { display: inline-flex; align-items: center; gap: 0.35rem; color: var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); }
</style>
