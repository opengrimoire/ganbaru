<script lang="ts" module>
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";

  /** Canvas geometry in logical pixels: a width and a height the room chooses between these bounds. */
  const MIN_CANVAS_W = BUILD_PLATFORM_PROFILE.shell === "mobile" ? 360 : 480;
  const MAX_CANVAS_W = 760;
  const MIN_CANVAS_H = 470;
  const MAX_CANVAS_H = 540;
  /** The bezel around the canvas, matching the other welcome scenes (`p-1.5`). */
  const BEZEL = 6;

  export const CHAT_SCENE_MIN_WIDTH = MIN_CANVAS_W + 2 * BEZEL;
  export const CHAT_SCENE_MAX_WIDTH = MAX_CANVAS_W + 2 * BEZEL;
  export const CHAT_SCENE_MIN_HEIGHT = MIN_CANVAS_H + 2 * BEZEL;
  export const CHAT_SCENE_MAX_HEIGHT = MAX_CANVAS_H + 2 * BEZEL;
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import Bold from "@lucide/svelte/icons/bold";
  import Bot from "@lucide/svelte/icons/bot";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Clock3 from "@lucide/svelte/icons/clock-3";
  import Feather from "@lucide/svelte/icons/feather";
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import Hash from "@lucide/svelte/icons/hash";
  import Heart from "@lucide/svelte/icons/heart";
  import Italic from "@lucide/svelte/icons/italic";
  import MessageCircle from "@lucide/svelte/icons/message-circle";
  import MessageSquareShare from "@lucide/svelte/icons/message-square-share";
  import MessagesSquare from "@lucide/svelte/icons/messages-square";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import Plus from "@lucide/svelte/icons/plus";
  import Search from "@lucide/svelte/icons/search";
  import Square from "@lucide/svelte/icons/square";
  import X from "@lucide/svelte/icons/x";
  import { chatLayoutDecision, CHAT_REPLY_THREAD_WIDTH_PX } from "$lib/chat/responsive-layout";
  import ChatModelAvatar from "$lib/components/chat/identity/ChatModelAvatar.svelte";
  import LocalPersonAvatar from "$lib/components/contacts/LocalPersonAvatar.svelte";
  import WorkspaceBreadcrumbTerminalIcon from "$lib/components/ui/WorkspaceBreadcrumbTerminalIcon.svelte";
  import WelcomeCaret from "$lib/components/vault/welcome/WelcomeCaret.svelte";
  import WelcomeCursor from "$lib/components/vault/welcome/WelcomeCursor.svelte";
  import WelcomePhoneTopBar from "$lib/components/vault/welcome/WelcomePhoneTopBar.svelte";
  import WelcomeTitleBar, { type WelcomeTitleTab } from "$lib/components/vault/welcome/WelcomeTitleBar.svelte";
  import WelcomeTouch from "$lib/components/vault/welcome/WelcomeTouch.svelte";
  import { formatDateTime, formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { cn } from "$lib/utils";
  import {
    CHAT_BEATS,
    CHAT_CURSOR,
    CHAT_SCENE_AGENTS,
    CHAT_SCENE_MENTIONED,
    CHAT_SCENE_WORKED_SECONDS,
    chatActiveSince,
    chatComposerCaretOn,
    chatComposerDraft,
    chatMessageIn,
    chatPickerNarrowed,
    chatPickerShown,
    chatPreviewShown,
    chatReplyIn,
    chatReplyStripShown,
    chatSent,
    chatShimmerPosition,
    chatStepState,
    chatStepsFolded,
    chatStreamedReply,
    chatThinkingShown,
    chatThreadShown,
    chatTouches,
    chatWorking,
    type ChatSceneAgent,
  } from "$lib/vault/welcome-chat-scene";
  import { welcomeCursorFrame, type WelcomeCursorPoint } from "$lib/vault/welcome-cursor";
  import { filmPressTransform, filmSpan, filmTap } from "$lib/vault/welcome-film";
  import { WELCOME_NO_HOVER, welcomeHoverAt, welcomeHoverSame } from "$lib/vault/welcome-hover";

  /**
   * One frame of the welcome scene on Chat, drawn with the workspace's own surfaces: in the Novel project's #general
   * channel, Claude is mentioned through the reference picker in a question that is then sent, and the message's reply
   * thread opens beside the channel, or over it where both do not fit, where Claude thinks, reads the chapter, searches
   * the manuscript, folds that work into one line, and streams its reply. Every style is a function of `elapsed`, with no
   * CSS transitions.
   */
  let { elapsed, width, height }: { elapsed: number; width: number; height: number } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const preferences = getPreferences();
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";

  /** The title bar on desktop, or the phone's top bar, tall enough for its context switcher. */
  const TOP_BAR_H = mobileShell ? 48 : 36;
  /** The workspace header and the thread header, as `--toolbar-h` sizes them on each shell. */
  const HEADER_H = mobileShell ? 56 : 40;
  /** The title bar's end padding, as the title bar draws it. */
  const TITLE_BAR_END_PX = 6;
  /** The title bar's tabs: the Calendar, and the Novel project's, which is showing. */
  const TITLE_TABS: readonly WelcomeTitleTab[] = [{ kind: "calendar", active: false }, { kind: "chat", active: true }];
  /** The header's identity icons, as the breadcrumb draws them, and the phone header's icons. */
  const IDENTITY_ICON_SIZE = 14;
  const IDENTITY_ICON_STROKE_WIDTH = 1.5;
  const MOBILE_ICON_SIZE = 16;
  const MOBILE_ICON_STROKE_WIDTH = 1.75;
  /** The members trigger's avatars, as the header stacks them. */
  const MEMBER_AVATAR_SIZE = 18;
  /** A message's avatar, a reference picker row's, and a reply strip's, as Chat draws them. */
  const MESSAGE_AVATAR_SIZE = 34;
  const PICKER_AVATAR_SIZE = 28;
  const STRIP_AVATAR_SIZE = 20;
  /** The width the workspace's global actions keep at the top right, as `--chat-global-actions-width` sets it. */
  const GLOBAL_ACTIONS_REM = 6.5;
  /** The conversation gutter, and the width at or below which it narrows, as the channel feed sets them. */
  const GUTTER_REM = 1;
  const GUTTER_NARROW_REM = 0.5;
  const GUTTER_NARROW_MAX_PX = 440;
  /** How far a new message rises into its feed, and the reference picker as it opens, in rem. */
  const MESSAGE_RISE_REM = 0.2;
  const PICKER_RISE_REM = 0.25;

  const canvasW = $derived(width - 2 * BEZEL);
  const canvasH = $derived(height - 2 * BEZEL);

  /** The time and day the scene's messages are sent on, as the feed and the thread show them. */
  const sentAt = new Date();
  const sentTime = $derived(formatDateTime(localization.locale, sentAt, { timeStyle: "short" }));
  const sentDay = $derived(formatDateTime(localization.locale, sentAt, { dateStyle: "full" }));
  const userName = $derived(preferences.profileDisplayName.trim() || t("chat.timeline.you"));
  const projectName = $derived(t("vaultSetup.projects.project"));
  const channelName = $derived(t("vaultSetup.chat.channel"));
  const question = $derived(t("vaultSetup.chat.prompt"));
  const reply = $derived(t("vaultSetup.chat.reply"));
  const chapterPath = $derived(t("vaultSetup.chat.chapterPath"));
  const searchQuery = $derived(t("vaultSetup.notes.characters.mateo"));
  const mention = `@${CHAT_SCENE_MENTIONED.name}`;
  const agentRoles = $derived<Readonly<Record<ChatSceneAgent["key"], string>>>({
    claude: t("vaultSetup.chat.roles.claude"),
    codex: t("vaultSetup.chat.roles.codex"),
  });

  const draft = $derived(chatComposerDraft(elapsed, question));
  const draftEmpty = $derived(draft.query.length === 0 && !draft.mention && draft.text.length === 0);
  const caretOn = $derived(chatComposerCaretOn(elapsed));
  const sent = $derived(chatSent(elapsed));
  const working = $derived(chatWorking(elapsed));
  const pickerShown = $derived(chatPickerShown(elapsed));
  const narrowed = $derived(chatPickerNarrowed(elapsed));
  const previewShown = $derived(chatPreviewShown(elapsed));
  const messageIn = $derived(chatMessageIn(elapsed));
  const threadShown = $derived(chatThreadShown(elapsed));
  const replyIn = $derived(chatReplyIn(elapsed));
  const stripShown = $derived(chatReplyStripShown(elapsed));
  const thinking = $derived(chatThinkingShown(elapsed));
  const readState = $derived(chatStepState("read", elapsed));
  const searchState = $derived(chatStepState("search", elapsed));
  const folded = $derived(chatStepsFolded(elapsed));
  const streamed = $derived(chatStreamedReply(elapsed, reply));
  const shimmer = $derived(chatShimmerPosition(elapsed, chatActiveSince(elapsed)));
  const touches = $derived(chatTouches(elapsed));
  const sendPress = $derived(filmTap(elapsed, CHAT_BEATS.sendTap));
  /** A phone taps Claude in the picker; the desktop accepts it with Enter, so nothing presses there. */
  const pickPress = $derived(mobileShell ? filmTap(elapsed, CHAT_BEATS.pick) : 0);

  /** Where the reply thread opens, as the workspace decides for a canvas this size: beside the channel or over it. */
  const threadPresentation = $derived(
    chatLayoutDecision({
      containerWidth: canvasW,
      containerHeight: canvasH - TOP_BAR_H,
      fontScale: 1,
      railOpen: false,
      inspectorOpen: false,
      inspectorWidth: 0,
      replyThreadOpen: true,
    }).replyThreadPresentation,
  );
  const threadColumn = $derived(!mobileShell && threadPresentation === "column");
  /** The thread column's width as it opens, and the channel's, which gives up that room. */
  const threadColumnW = $derived(threadColumn ? CHAT_REPLY_THREAD_WIDTH_PX * threadShown : 0);
  const mainW = $derived(canvasW - threadColumnW);
  const gutterRem = $derived(mainW <= GUTTER_NARROW_MAX_PX ? GUTTER_NARROW_REM : GUTTER_REM);

  /** The picker's rows: the channel's agents, then the project's working folder, narrowed by the typed query. */
  const pickerAgents = $derived(narrowed ? [CHAT_SCENE_MENTIONED] : CHAT_SCENE_AGENTS);

  /** The assignment preview's measured height, which it grows into above the composer as it shows. */
  let previewH = $state(0);

  /** Whether the desktop pointer has left the canvas, where it hovers the named parts under it. A phone has touches and no hover. */
  const cursorAway = $derived(welcomeCursorFrame(CHAT_CURSOR, elapsed).away);
  let cursorPoint = $state<WelcomeCursorPoint | null>(null);
  let hover = $state<readonly string[]>(WELCOME_NO_HOVER);
  let canvas = $state<HTMLDivElement>();

  /** Follow the desktop pointer as it moves and as the scene changes under it. */
  $effect(() => {
    void elapsed;
    const next = mobileShell || cursorAway || !canvas ? WELCOME_NO_HOVER : welcomeHoverAt(canvas, canvasW, cursorPoint);
    if (!welcomeHoverSame(next, untrack(() => hover))) hover = next;
  });

  /** Whether the part named `name` is under the desktop pointer. */
  function hovered(name: string): boolean {
    return hover.includes(name);
  }

  /** The picker row that is highlighted: the one under the pointer, or the first, as the picker starts. */
  const highlightedAgent = $derived(pickerAgents.find((agent) => hovered(`pick:${agent.key}`))?.key ?? (hovered("pick:folder") ? null : pickerAgents[0].key));

  /** Something rising into its place as far as `progress` says. */
  function riseStyle(progress: number, rise = MESSAGE_RISE_REM): string | undefined {
    if (progress >= 1) return undefined;
    return `opacity: ${progress}; transform: translateY(${rise * (1 - progress)}rem);`;
  }

  /** A press on a control, or a slight lift while the pointer is over it. */
  function pressStyle(press: number, lifted: boolean): string | undefined {
    if (press > 0) return `transform: ${filmPressTransform(press)};`;
    return lifted ? "transform: scale(1.04);" : undefined;
  }

  /** The shimmer an active line sweeps across its text. */
  const shimmerStyle = $derived(`background-position: ${shimmer}% 0;`);

  /** How far the folded summary's chevron and the work it folds away show. */
  const foldShown = $derived(filmSpan(folded, 0, 1));
</script>

<!-- An activity line of the turn: its icon, then its text, which shimmers while the step runs. -->
{#snippet step(icon: typeof FileText, text: string, active: boolean)}
  {@const Icon = icon}
  <span class={cn("process-step", active && "active")}>
    <Icon size={15} />
    <span class="process-text" style={active ? shimmerStyle : undefined}>{text}</span>
  </span>
{/snippet}

<!-- The sent message, with its mention of Claude, and in the channel its reply strip once Claude replies. -->
{#snippet message(inChannel: boolean)}
  <div class={cn("message-row", !inChannel && "px-4")} style={inChannel ? riseStyle(messageIn) : undefined}>
    <LocalPersonAvatar size={MESSAGE_AVATAR_SIZE} />
    <div class="message-body">
      <header><strong>{userName}</strong><time>{sentTime}</time></header>
      <div class="message-copy"><span class="mention">{mention}</span> {question}</div>
      {#if inChannel && stripShown}
        <span class={cn("reply-strip", hovered("strip") && "hovered")} data-scene-hover="strip">
          <span class="reply-avatars"><ChatModelAvatar familyId={CHAT_SCENE_MENTIONED.familyId} label={CHAT_SCENE_MENTIONED.name} size={STRIP_AVATAR_SIZE} /></span>
          <MessageCircle size={13} />
          <span>{t("chat.organization.replies", 1)}</span>
        </span>
      {/if}
    </div>
  </div>
{/snippet}

<!-- A composer's footer: its tools, and Send, or Stop while Claude works on the message's assignment. -->
{#snippet footer(prefix: string, inThread: boolean, hasContent: boolean, press: number)}
  <div class="composer-footer">
    <div class="composer-tools">
      <span class={cn("tool-button", hovered(`${prefix}:attach`) && "hovered")} data-scene-hover={`${prefix}:attach`}><Plus size={16} /></span>
      <span class={cn("tool-button", hovered(`${prefix}:bold`) && "hovered")} data-scene-hover={`${prefix}:bold`}><Bold size={15} /></span>
      <span class={cn("tool-button", hovered(`${prefix}:italic`) && "hovered")} data-scene-hover={`${prefix}:italic`}><Italic size={15} /></span>
      <span class={cn("tool-button", hovered(`${prefix}:schedule`) && "hovered")} data-scene-hover={`${prefix}:schedule`}><Clock3 size={15} /></span>
      {#if inThread}
        <span class={cn("tool-button", hovered(`${prefix}:share`) && "hovered")} data-scene-hover={`${prefix}:share`}><MessageSquareShare size={15} /></span>
      {/if}
    </div>
    {#if inThread && working}
      <span class="send-button" style={pressStyle(0, hovered(`${prefix}:send`))} data-scene-hover={`${prefix}:send`}><Square size={13} /></span>
    {:else}
      <span
        class={cn("send-button", !hasContent && "disabled")}
        data-cursor={inThread ? undefined : "chat-send"}
        data-scene-hover={`${prefix}:send`}
        style={pressStyle(press, hasContent && hovered(`${prefix}:send`))}
      >
        <ArrowUp size={16} />
        {#if mobileShell && !inThread}<WelcomeTouch opacity={touches.send} />{/if}
      </span>
    {/if}
  </div>
{/snippet}

<!-- The reply thread: its tab, the message it answers, Claude's turn, and its own composer. -->
{#snippet thread(reserveGlobalActions: boolean)}
  <div class="reply-thread-panel" data-cursor="chat-thread">
    <div class="thread-header" style:height={`${HEADER_H}px`} style:padding-right={reserveGlobalActions ? `${GLOBAL_ACTIONS_REM}rem` : undefined}>
      <span class={cn("thread-tab-shell", (mobileShell || hovered("thread-tab")) && "show-close")} data-scene-hover="thread-tab">
        <span class="thread-tab"><MessagesSquare size={13} /><strong>{t("chat.organization.thread")}</strong></span>
        <span class="tab-close"><X size={11} /></span>
      </span>
    </div>
    <div class="flex min-h-0 flex-1 flex-col justify-end overflow-hidden">
      <div class="thread-scroll">
        <div class="date-divider"><span>{sentDay}</span></div>
        {@render message(false)}
        {#if replyIn > 0}
          <div class="turn-row" style={riseStyle(replyIn)}>
            <ChatModelAvatar familyId={CHAT_SCENE_MENTIONED.familyId} label={CHAT_SCENE_MENTIONED.name} size={MESSAGE_AVATAR_SIZE} />
            <div class="turn-content">
              <div class="turn-header"><strong>{CHAT_SCENE_MENTIONED.name}</strong><span>{sentTime}</span></div>
              {#if thinking}
                <div class="process-history mt-0.5">
                  <span class="process-step thinking active"><span class="process-text" style={shimmerStyle}>{t("chat.timeline.thinking")}</span></span>
                </div>
              {:else}
                <!-- The work so far, folding into its summary line as the reply starts to stream. -->
                <div class="process-fold" style:--fold={folded} style:--lines={searchState === "hidden" ? 1 : 2}>
                  {#if foldShown > 0}
                    <span class={cn("process-toggle", hovered("worked") && "text-foreground")} style:opacity={foldShown} data-scene-hover="worked">
                      <FileText size={15} />
                      <span class="truncate">{t("chat.timeline.workedFor", t("chat.timeline.seconds", formatNumber(localization.locale, CHAT_SCENE_WORKED_SECONDS)))}</span>
                      <ChevronRight size={14} />
                    </span>
                  {/if}
                  {#if folded < 1}
                    <div class="process-history" style:opacity={1 - folded}>
                      {@render step(FileText, readState === "active" ? t("chat.timeline.readingPath", chapterPath) : t("chat.timeline.readPath", chapterPath), readState === "active")}
                      {#if searchState !== "hidden"}
                        {@render step(Search, searchState === "active" ? t("chat.timeline.searchingFor", searchQuery) : t("chat.timeline.searchedFor", searchQuery), searchState === "active")}
                      {/if}
                    </div>
                  {/if}
                </div>
                {#if streamed.length > 0}<div class="turn-copy mt-1.5">{streamed}</div>{/if}
              {/if}
            </div>
          </div>
        {/if}
      </div>
    </div>
    <div class="thread-composer">
      <div class="organizational-composer">
        <div class="editor-frame">
          <div class="organizational-editor empty" data-placeholder={t("chat.organization.replyInThread")}></div>
        </div>
        {@render footer("thread", true, false, 0)}
      </div>
    </div>
  </div>
{/snippet}

<div class="rounded-3xl bg-foreground/6 p-1.5">
  <div
    bind:this={canvas}
    class="chat-scene relative flex flex-col overflow-hidden rounded-2xl bg-(--cal-bg) text-foreground"
    style:width={`${canvasW}px`}
    style:height={`${canvasH}px`}
  >
    {#if mobileShell}
      <!-- The phone's top bar, with the Novel project's context switcher. -->
      <WelcomePhoneTopBar height={TOP_BAR_H} identity="project" />
    {:else}
      <!-- The title bar's tabs, with the project's Chat showing, and its utilities. -->
      <WelcomeTitleBar height={TOP_BAR_H} endPadding={TITLE_BAR_END_PX} tabs={TITLE_TABS} />
    {/if}

    <div class="relative flex min-h-0 flex-1">
      <div data-cursor="chat-panel" class="flex min-w-0 shrink-0 flex-col" style:width={`${mainW}px`}>
        <!-- The workspace header over the channel: where the channel is, with the members kept at the top right. -->
        <div
          class={cn("flex shrink-0 items-center", mobileShell ? "px-(--mobile-chrome-inset)" : "px-3")}
          style:height={`${HEADER_H}px`}
          style:padding-right={!mobileShell && !threadColumn ? `${GLOBAL_ACTIONS_REM}rem` : undefined}
        >
          {#if mobileShell}
            <span class="flex h-12 w-(--mobile-chrome-column) shrink-0 items-center justify-center text-foreground">
              <PanelLeft size={MOBILE_ICON_SIZE} strokeWidth={MOBILE_ICON_STROKE_WIDTH} />
            </span>
            <span class="flex h-12 min-w-0 flex-1 items-center gap-1.5 px-1.5 text-foreground">
              <Hash size={MOBILE_ICON_SIZE} strokeWidth={MOBILE_ICON_STROKE_WIDTH} class="shrink-0" />
              <span class="min-w-0 truncate text-xs font-medium">{channelName}</span>
            </span>
            <span class="members"><span class="members-stack"><LocalPersonAvatar size={MEMBER_AVATAR_SIZE} /></span><span class="members-count tabular-nums">{formatNumber(localization.locale, CHAT_SCENE_AGENTS.length + 1)}</span></span>
          {:else}
            <span class="flex h-7 min-w-0 items-center gap-0.5 overflow-hidden text-identity font-medium">
              <span class={cn("flex h-7 min-w-0 items-center gap-1.5 rounded-md px-1.5", hovered("crumb:group") && "bg-accent")} data-scene-hover="crumb:group">
                <Heart size={IDENTITY_ICON_SIZE} strokeWidth={IDENTITY_ICON_STROKE_WIDTH} class="shrink-0" />
                <span class="truncate">{t("vaultSetup.film.personalGroup")}</span>
              </span>
              <span class="shrink-0 px-0.5 font-semibold text-muted-foreground">/</span>
              <span class={cn("flex h-7 min-w-0 items-center gap-1.5 rounded-md px-1.5", hovered("crumb:project") && "bg-accent")} data-scene-hover="crumb:project">
                <Feather size={IDENTITY_ICON_SIZE} strokeWidth={IDENTITY_ICON_STROKE_WIDTH} class="shrink-0" />
                <span class="truncate">{projectName}</span>
              </span>
              <span class="shrink-0 px-0.5 font-semibold text-muted-foreground">/</span>
              <span class={cn("flex h-7 min-w-0 items-center gap-1.5 rounded-md px-1.5", hovered("crumb:channel") && "bg-accent")} data-scene-hover="crumb:channel">
                <Hash size={IDENTITY_ICON_SIZE} strokeWidth={IDENTITY_ICON_STROKE_WIDTH} class="shrink-0" />
                <span class="truncate">{channelName}</span>
                <WorkspaceBreadcrumbTerminalIcon kind="chevron" context="chat" class="shrink-0 text-muted-foreground" />
              </span>
            </span>
          {/if}
        </div>

        <div class="relative min-h-0 flex-1">
          <!-- The channel: its feed, and its composer docked under it. -->
          <div class="absolute inset-0 flex flex-col" style:opacity={threadColumn ? undefined : 1 - threadShown} style:--gutter={`${gutterRem}rem`}>
            <div class="flex min-h-0 flex-1 flex-col justify-end overflow-hidden">
              <div class="feed-canvas">
                <header class="channel-introduction">
                  <h1>#{channelName}</h1>
                  <p>{t("chat.channels.welcomeDescription")}</p>
                </header>
                {#if sent}
                  <div class="divider first-date" style={riseStyle(messageIn)}><span>{sentDay}</span></div>
                  {@render message(true)}
                {/if}
              </div>
            </div>

            <div class="channel-composer-dock">
              <div class="relative w-full">
                {#if previewShown > 0}
                  <div class="overflow-hidden" style:height={previewShown < 1 ? `${previewH * previewShown}px` : undefined} style:opacity={previewShown}>
                    <div bind:offsetHeight={previewH} class="pb-[0.4rem]">
                      <section class="assignment-preview">
                        <Bot size={13} />
                        <div>
                          <strong>{projectName}</strong>
                          <span>{t("chat.organization.noSecondaryFolders")}</span>
                        </div>
                        <span class="approval-pill">{t("settings.chat.teammates.runtime.ask")}</span>
                      </section>
                    </div>
                  </div>
                {/if}

                <div class="organizational-composer">
                  <div class="editor-frame" data-cursor="chat-editor">
                    <div class={cn("organizational-editor", draftEmpty && "empty")} data-placeholder={t("chat.organization.messageChannel", channelName)}>
                      {#if draft.mention}<span class="reference-atom">{mention}</span>{" "}{draft.text}{:else}{draft.query}{/if}{#if caretOn}<WelcomeCaret />{/if}
                    </div>
                    {#if mobileShell}<WelcomeTouch opacity={touches.editor} />{/if}
                  </div>

                  {#if pickerShown > 0}
                    <!-- The reference picker, open over the feed with what an at sign can mention. -->
                    <div data-scene-cover class="reference-picker surface-floating" style={riseStyle(pickerShown, PICKER_RISE_REM)}>
                      <div class="surface-floating-body">
                        <div class="picker-group menu-label">{t("chat.organization.channelParticipants")}</div>
                        {#each pickerAgents as agent (agent.key)}
                          <span
                            class="menu-item"
                            data-highlighted={highlightedAgent === agent.key ? "" : undefined}
                            data-scene-hover={`pick:${agent.key}`}
                            style={agent.key === CHAT_SCENE_MENTIONED.key && pickPress > 0 ? `transform: ${filmPressTransform(pickPress)};` : undefined}
                          >
                            <span class="candidate-icon"><ChatModelAvatar familyId={agent.familyId} label={agent.name} size={PICKER_AVATAR_SIZE} /></span>
                            <span class="candidate-copy"><strong>{agent.name}</strong><small>{agentRoles[agent.key]}</small></span>
                            {#if mobileShell && agent.key === CHAT_SCENE_MENTIONED.key}<WelcomeTouch opacity={touches.pick} />{/if}
                          </span>
                        {/each}
                        {#if !narrowed}
                          <div class="picker-group menu-label">{t("chat.organization.projectFolders")}</div>
                          <span class="menu-item" data-highlighted={hovered("pick:folder") ? "" : undefined} data-scene-hover="pick:folder">
                            <span class="candidate-icon"><Folder size={15} /></span>
                            <span class="candidate-copy"><strong>{projectName}</strong><small>{t("chat.organization.folderReference")}</small></span>
                          </span>
                        {/if}
                      </div>
                    </div>
                  {/if}

                  {@render footer("channel", false, !draftEmpty, sendPress)}
                </div>
              </div>
            </div>
          </div>

          {#if !threadColumn && threadShown > 0}
            <!-- The reply thread over the channel, where there is no room for both. -->
            <div class="absolute inset-0 flex" style={riseStyle(threadShown)}>{@render thread(false)}</div>
          {/if}
        </div>
      </div>

      {#if threadColumnW > 0}
        <!-- The reply thread beside the channel, from the top of the header row, behind a separator below it. -->
        <div class="relative min-w-0 overflow-hidden" style:width={`${threadColumnW}px`}>
          <span class="thread-separator" style:top={`${HEADER_H}px`}></span>
          <div class="flex h-full" style:width={`${CHAT_REPLY_THREAD_WIDTH_PX}px`}>{@render thread(true)}</div>
        </div>
      {/if}

      {#if !mobileShell}
        <!-- The workspace's global actions, at the top right over whichever header is there. -->
        <span class="global-actions" style:height={`${HEADER_H}px`} style:min-width={`${GLOBAL_ACTIONS_REM}rem`}>
          <span class={cn("members h-7 rounded-md", hovered("members") && "hovered")} data-scene-hover="members">
            <span class="members-stack"><LocalPersonAvatar size={MEMBER_AVATAR_SIZE} /></span>
            <span class="members-count tabular-nums">{formatNumber(localization.locale, CHAT_SCENE_AGENTS.length + 1)}</span>
          </span>
        </span>
      {/if}
    </div>

    {#if !mobileShell}
      <WelcomeCursor script={CHAT_CURSOR} {elapsed} width={canvasW} bind:position={cursorPoint} />
    {/if}
  </div>
</div>

<style>
  .members { display: flex; flex: 0 0 auto; align-items: center; gap: 0.3rem; padding-inline: 0.3rem 0.5rem; color: var(--foreground); }
  .members.hovered { background-color: var(--accent); }
  .members-stack { display: flex; align-items: center; }
  .members-stack > :global(span) { flex: 0 0 auto; border-radius: 22%; box-shadow: 0 0 0 1.5px var(--cal-bg); }
  .members.hovered .members-stack > :global(span) { box-shadow: 0 0 0 1.5px var(--accent); }
  .members-count { font-size: 0.733333rem; font-weight: 500; }
  .global-actions { position: absolute; top: 0; right: 0; z-index: 5; display: flex; align-items: center; justify-content: flex-end; background: var(--cal-bg); padding-right: 0.75rem; }

  /* The channel feed, its introduction, and its date divider, as the channel draws them. */
  .feed-canvas { width: calc(100% - 2 * var(--gutter)); min-height: 100%; flex-shrink: 0; margin-inline: auto; padding-bottom: 1rem; }
  .channel-introduction { padding-top: 1.2rem; }
  .channel-introduction h1 { font-size: 1.25rem; font-weight: 700; }
  .channel-introduction p { margin-top: 0.3rem; color: var(--muted-foreground); font-size: 0.82rem; line-height: 1.35rem; }
  .divider, .date-divider { display: flex; align-items: center; gap: 0.5rem; color: var(--muted-foreground); font-size: 0.65rem; }
  .divider::before, .divider::after, .date-divider::before, .date-divider::after { height: 1px; flex: 1; background: var(--border); content: ""; }
  .divider.first-date { margin-block: 1.2rem 0.75rem; }
  .date-divider { margin: 0.75rem; }

  /* A message as an organizational message draws it, with its mention and reply strip. */
  .message-row { display: grid; grid-template-columns: 34px minmax(0, 1fr); gap: 0.65rem; padding-block: 0.45rem; }
  .message-body { min-width: 0; transform: translateY(-4px); }
  .message-body header { display: flex; min-height: 1.3rem; align-items: baseline; gap: 0.4rem; }
  .message-body header strong { font-size: 0.875rem; }
  .message-body header time { color: var(--muted-foreground); font-size: 0.6875rem; }
  .message-copy { white-space: pre-wrap; overflow-wrap: anywhere; color: var(--foreground); font-size: 0.875rem; line-height: 1.3125rem; }
  .mention { border-radius: 0.28rem; background: color-mix(in srgb, var(--primary) 14%, transparent); padding: 0.05em 0.22em; color: color-mix(in srgb, var(--primary) 76%, var(--foreground)); font-weight: 650; box-decoration-break: clone; -webkit-box-decoration-break: clone; }
  .reply-strip { display: flex; width: 100%; min-height: 2rem; align-items: center; gap: 0.4rem; margin-top: 0.35rem; border-radius: 0.4rem; color: color-mix(in srgb, var(--primary) 70%, var(--foreground)); font-size: 0.68rem; }
  .reply-strip.hovered { background: color-mix(in srgb, var(--accent) 45%, transparent); }
  .reply-avatars { display: flex; align-items: center; gap: 0.25rem; padding-left: 0.2rem; --chat-participant-avatar-radius: 0.2rem; }

  /* The composer, its assignment preview, and its reference picker, as the organizational composer draws them. */
  .channel-composer-dock { display: flex; flex: 0 0 auto; justify-content: center; padding: 0.5rem var(--gutter) 0.75rem; background: linear-gradient(to bottom, transparent, var(--cal-bg) 18%); }
  .assignment-preview { display: grid; min-height: 2.45rem; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 0.5rem; margin-inline: 0.4rem; border: 1px solid color-mix(in srgb, var(--border) 68%, transparent); border-radius: 0.65rem; background: color-mix(in srgb, var(--card) 80%, transparent); padding: 0.35rem 0.55rem; color: var(--muted-foreground); }
  .assignment-preview > div { display: grid; min-width: 0; }
  .assignment-preview strong, .assignment-preview span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .assignment-preview strong { color: var(--foreground); font-size: 0.68rem; }
  .assignment-preview span { font-size: 0.6rem; }
  .approval-pill { border-radius: 999px; background: var(--accent); padding: 0.18rem 0.4rem; color: var(--foreground); }
  .organizational-composer { position: relative; width: 100%; border: 1px solid color-mix(in srgb, var(--border) 88%, transparent); border-radius: 1.3rem; background: var(--card); box-shadow: 0 8px 22px -18px rgb(0 0 0 / 0.24), 0 1px 4px -3px rgb(0 0 0 / 0.16); }
  .editor-frame { position: relative; padding: 1rem 1.25rem 0.4rem; }
  .organizational-editor { position: relative; min-height: 2.625rem; white-space: pre-wrap; overflow-wrap: anywhere; color: var(--foreground); font-size: 0.875rem; line-height: 1.3125rem; }
  .organizational-editor.empty::before { position: absolute; inset-inline: 0; pointer-events: none; content: attr(data-placeholder); color: color-mix(in srgb, var(--muted-foreground) 52%, transparent); }
  .reference-atom { display: inline-block; border-radius: 0.3rem; background: color-mix(in srgb, var(--primary) 13%, transparent); padding-inline: 0.12rem; color: color-mix(in srgb, var(--primary) 78%, var(--foreground)); }
  .reference-picker { position: absolute; inset-inline: 0; bottom: calc(100% + 4px); z-index: 45; display: flex; flex-direction: column; overflow: hidden; }
  .picker-group { background: var(--popover); }
  .menu-item { position: relative; }
  .candidate-icon { display: grid; width: 2rem; flex: 0 0 auto; place-items: center; color: var(--muted-foreground); }
  .candidate-copy { display: grid; min-width: 0; flex: 1 1 0%; }
  .candidate-copy strong, .candidate-copy small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .candidate-copy strong { font-weight: 500; }
  .candidate-copy small { color: var(--muted-foreground); font-size: var(--panel-detail-font-size); }
  .composer-footer { display: flex; min-height: 2.6rem; align-items: center; justify-content: space-between; gap: 0.5rem; padding: 0 0.75rem 0.5rem; }
  .composer-tools { display: flex; min-width: 0; align-items: center; gap: 0.3rem; overflow: hidden; }
  .tool-button { display: grid; width: 1.9rem; height: 1.9rem; flex: 0 0 auto; place-items: center; border-radius: 0.5rem; color: var(--muted-foreground); }
  .tool-button.hovered { background: var(--accent); color: var(--foreground); }
  .send-button { position: relative; display: grid; width: 2rem; height: 2rem; flex: 0 0 auto; place-items: center; border-radius: 999px; background: color-mix(in srgb, var(--primary) 92%, transparent); color: var(--primary-foreground); box-shadow: 0 2px 7px color-mix(in srgb, var(--primary) 22%, transparent); }
  .send-button.disabled { opacity: 0.45; }

  /* The reply thread, as its panel draws it. */
  .reply-thread-panel { display: flex; width: 100%; height: 100%; min-height: 0; flex-direction: column; background: var(--cal-bg); }
  .thread-header { display: flex; flex: 0 0 auto; align-items: center; gap: 0.2rem; padding-inline: 0.45rem; }
  .thread-tab-shell { display: flex; width: 9.5rem; min-width: 3.75rem; height: 2rem; flex: 0 1 9.5rem; align-items: stretch; overflow: hidden; border-radius: 0.55rem; background: var(--accent); color: var(--foreground); }
  .thread-tab { display: flex; min-width: 0; flex: 1 1 auto; align-items: center; gap: 0.4rem; overflow: hidden; padding: 0.3rem 0.2rem 0.3rem 0.65rem; }
  .thread-tab :global(svg) { flex: 0 0 auto; }
  .thread-tab strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 0.733333rem; font-weight: 500; }
  .tab-close { display: grid; width: 1.5rem; flex: 0 0 auto; place-items: center; border-radius: 0.3rem; opacity: 0; }
  .thread-tab-shell.show-close .tab-close { opacity: 1; }
  .thread-scroll { min-height: 100%; flex-shrink: 0; padding-block: 0.4rem; }
  .thread-composer { display: flex; flex: 0 0 auto; justify-content: center; padding: 0.5rem 0.5rem 0.75rem; }
  .thread-separator { position: absolute; bottom: 0; left: 0; z-index: 1; width: 1px; background: var(--border); }

  /* Claude's turn in the thread, as the embedded execution timeline draws it. */
  .turn-row { display: grid; min-width: 0; grid-template-columns: 34px minmax(0, 1fr); align-items: start; gap: 0.65rem; padding: 0.45rem 1rem 0; }
  .turn-content { min-width: 0; transform: translateY(-4px); }
  .turn-header { display: flex; min-height: 1.25rem; min-width: 0; align-items: baseline; gap: 0.45rem; margin-bottom: 0.12rem; line-height: 1.25rem; }
  .turn-header strong { min-width: 0; overflow: hidden; color: var(--foreground); font-size: 0.875rem; font-weight: 650; text-overflow: ellipsis; white-space: nowrap; }
  .turn-header span { flex: 0 0 auto; color: var(--muted-foreground); font-size: 0.6875rem; font-weight: 400; }
  .turn-copy { min-width: 0; color: var(--foreground); font-size: 0.875rem; line-height: 1.3125rem; overflow-wrap: anywhere; }

  /* The steps fold from their lines into the one summary line, as the disclosure collapses. */
  .process-fold { position: relative; overflow: hidden; height: calc(1.1875rem + (1.1875rem + 0.125rem) * (var(--lines) - 1) * (1 - var(--fold))); margin-top: 0.125rem; }
  .process-fold > .process-toggle { position: absolute; inset: 0 0 auto; }
  .process-history { display: grid; min-width: 0; gap: 0.125rem; color: var(--muted-foreground); }
  .process-step { display: grid; width: 100%; min-width: 0; grid-template-columns: 1rem minmax(0, 1fr); align-items: start; gap: 0.45rem; color: var(--muted-foreground); font-size: 0.8125rem; line-height: 1.1875rem; }
  .process-step.thinking { grid-template-columns: minmax(0, 1fr); }
  .process-step :global(svg) { margin-top: 0.15625rem; }
  .process-text { width: fit-content; min-width: 0; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .process-step.active { color: color-mix(in srgb, var(--muted-foreground) 78%, var(--foreground)); }
  .process-step.active .process-text {
    background: linear-gradient(100deg, var(--muted-foreground) 0%, var(--muted-foreground) 42%, var(--foreground) 50%, var(--muted-foreground) 58%, var(--muted-foreground) 100%);
    background-repeat: no-repeat;
    background-size: 320% 100%;
    background-clip: text;
    color: transparent;
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
  }
  .process-toggle { display: flex; min-width: 0; min-height: 1.1875rem; align-items: center; gap: 0.4rem; color: var(--muted-foreground); font-size: 0.8125rem; line-height: 1.1875rem; }
  .process-toggle :global(svg) { flex: 0 0 auto; }
</style>
