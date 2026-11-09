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
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Music from "@lucide/svelte/icons/eighth-notes";
  import Feather from "@lucide/svelte/icons/feather";
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import Hash from "@lucide/svelte/icons/hash";
  import Heart from "@lucide/svelte/icons/heart";
  import Italic from "@lucide/svelte/icons/italic";
  import Laptop from "@lucide/svelte/icons/laptop";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import Plus from "@lucide/svelte/icons/plus";
  import Search from "@lucide/svelte/icons/search";
  import ShieldQuestionMark from "@lucide/svelte/icons/shield-question-mark";
  import Square from "@lucide/svelte/icons/square";
  import Zap from "@lucide/svelte/icons/zap";
  import { compactModelOptionLabel, modelEffortStopPosition } from "$lib/chat/composer/model-picker-model";
  import ChatModelAvatar from "$lib/components/chat/identity/ChatModelAvatar.svelte";
  import LocalPersonAvatar from "$lib/components/contacts/LocalPersonAvatar.svelte";
  import WorkspaceBreadcrumbTerminalIcon from "$lib/components/ui/WorkspaceBreadcrumbTerminalIcon.svelte";
  import WelcomeCaret from "$lib/components/vault/welcome/WelcomeCaret.svelte";
  import WelcomeCursor from "$lib/components/vault/welcome/WelcomeCursor.svelte";
  import WelcomeTouch from "$lib/components/vault/welcome/WelcomeTouch.svelte";
  import { formatDateTime, formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { cn } from "$lib/utils";
  import {
    CHAT_CURSOR,
    CHAT_SCENE_EFFORTS,
    CHAT_SCENE_MODEL_NAME,
    CHAT_SCENE_START_EFFORT,
    CHAT_SCENE_WORKED_SECONDS,
    chatActiveSince,
    chatComposerCaretOn,
    chatComposerDraft,
    chatEffort,
    chatKnobHeld,
    chatKnobStop,
    chatPickerOpen,
    chatPickerShown,
    chatReplyIn,
    chatSendTap,
    chatSent,
    chatShimmerPosition,
    chatStepState,
    chatStepsFolded,
    chatStreamedReply,
    chatStripShown,
    chatThinkingShown,
    chatTouches,
    chatUltra,
    chatUserMessageIn,
    chatWorking,
    type ChatSceneEffort,
  } from "$lib/vault/welcome-chat-scene";
  import { welcomeCursorFrame, type WelcomeCursorPoint } from "$lib/vault/welcome-cursor";
  import { filmPressTransform, filmSpan, filmTap } from "$lib/vault/welcome-film";
  import { WELCOME_NO_HOVER, welcomeHoverAt, welcomeHoverSame } from "$lib/vault/welcome-hover";

  /**
   * One frame of the welcome scene on Chat, drawn with the workspace's own surfaces: a prompt is typed into the Novel
   * project's composer, the model picker raises Claude's effort from High to Max, and once the prompt is sent, Claude
   * thinks, reads the chapter, searches the manuscript, folds that work into one line, and streams its reply. Every style
   * is a function of `elapsed`, with no CSS transitions.
   */
  let { elapsed, width, height }: { elapsed: number; width: number; height: number } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const preferences = getPreferences();
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";

  /** The title bar on desktop, or the phone's top bar, tall enough for its context switcher. */
  const TOP_BAR_H = mobileShell ? 48 : 36;
  /** The workspace header, as `--toolbar-h` sizes it on each shell. */
  const HEADER_H = mobileShell ? 56 : 40;
  /** The title bar's icons and its end padding, as the title bar draws them. */
  const TITLE_ICON_SIZE = 14;
  const TITLE_ICON_STROKE_WIDTH = 1.5;
  const TITLE_BAR_END_PX = 6;
  const TITLE_ICON_COLOR_CLASS = "text-foreground/68 dark:text-white/76";
  /** The header's identity icons, as the breadcrumb draws them, and the phone header's icons. */
  const IDENTITY_ICON_SIZE = 14;
  const IDENTITY_ICON_STROKE_WIDTH = 1.5;
  const MOBILE_ICON_SIZE = 16;
  const MOBILE_ICON_STROKE_WIDTH = 1.75;
  /** The members trigger's avatars, as the header stacks them. */
  const MEMBER_AVATAR_SIZE = 18;
  /** A timeline row's avatar. */
  const ROW_AVATAR_SIZE = 36;
  /** The branch the Novel project's working folder is on. */
  const CHAT_SCENE_BRANCH = "main";
  /** The workspace widths below which the header hides its branch and the strip its Local item, as both do. */
  const HEADER_BRANCH_MIN_W = 760;
  const STRIP_COMPACT_MAX_W = 560;
  /** The composer widths below which it hides its format buttons and effort name, then its context ring, as it does. */
  const COMPOSER_FORMAT_MAX_W = 460;
  const COMPOSER_RING_MAX_W = 390;
  /** The room around the composer: in the first use hero, and docked under a thread once the first message sends. */
  const HERO_PAD_PX = 16;
  const HERO_BOTTOM_PX = 28;
  const DOCK_PAD_PX = 12;
  /** The effort ladder's track height and the inset its end stops keep, as the ladder lays them out. */
  const EFFORT_TRACK_REM = 1.75;
  const EFFORT_INSET_REM = EFFORT_TRACK_REM / 2;
  /** How full the context ring is before the turn, and once the reply has streamed in. */
  const CONTEXT_START = 0.03;
  const CONTEXT_END = 0.11;
  /** The context ring's circumference, as its circle of radius 7 draws it. */
  const CONTEXT_RING_LENGTH = 43.9823;
  /** How far a new message rises into the timeline, in rem. */
  const MESSAGE_RISE_REM = 0.2;
  /** How far the model picker rises as it opens, in rem. */
  const PICKER_RISE_REM = 0.25;

  const canvasW = $derived(width - 2 * BEZEL);
  const canvasH = $derived(height - 2 * BEZEL);

  /** The time the scene's messages are sent at, as the timeline shows it. */
  const sentAt = new Date();
  const sentTime = $derived(formatDateTime(localization.locale, sentAt, { timeStyle: "short" }));
  const userName = $derived(preferences.profileDisplayName.trim() || t("chat.timeline.you"));
  const projectName = $derived(t("vaultSetup.projects.project"));
  const prompt = $derived(t("vaultSetup.chat.prompt"));
  const reply = $derived(t("vaultSetup.chat.reply"));
  const chapterPath = $derived(t("vaultSetup.chat.chapterPath"));
  const searchQuery = $derived(t("vaultSetup.notes.characters.mateo"));

  const draft = $derived(chatComposerDraft(elapsed, prompt, mobileShell));
  const caretOn = $derived(chatComposerCaretOn(elapsed, mobileShell));
  const sent = $derived(chatSent(elapsed, mobileShell));
  const working = $derived(chatWorking(elapsed, mobileShell));
  const pickerShown = $derived(chatPickerShown(elapsed, mobileShell));
  const pickerOpen = $derived(chatPickerOpen(elapsed, mobileShell));
  const effort = $derived(chatEffort(elapsed, mobileShell));
  const knobStop = $derived(chatKnobStop(elapsed, mobileShell));
  const ultra = $derived(chatUltra(elapsed, mobileShell));
  const knobHeld = $derived(chatKnobHeld(elapsed, mobileShell));
  const stripShown = $derived(chatStripShown(elapsed, mobileShell));
  const userIn = $derived(chatUserMessageIn(elapsed, mobileShell));
  const replyIn = $derived(chatReplyIn(elapsed, mobileShell));
  const thinking = $derived(chatThinkingShown(elapsed, mobileShell));
  const readState = $derived(chatStepState("read", elapsed, mobileShell));
  const searchState = $derived(chatStepState("search", elapsed, mobileShell));
  const folded = $derived(chatStepsFolded(elapsed, mobileShell));
  const streamed = $derived(chatStreamedReply(elapsed, reply, mobileShell));
  const shimmer = $derived(chatShimmerPosition(elapsed, chatActiveSince(elapsed, mobileShell)));
  const sendPress = $derived(filmTap(elapsed, chatSendTap(mobileShell)));
  const touches = $derived(chatTouches(elapsed));

  /** The room around the composer, moving from the hero's to the docked composer's as the strip folds away. */
  const composerPadX = $derived(DOCK_PAD_PX + (HERO_PAD_PX - DOCK_PAD_PX) * stripShown);
  const composerPadBottom = $derived(DOCK_PAD_PX + (HERO_BOTTOM_PX - DOCK_PAD_PX) * stripShown);
  const composerW = $derived(canvasW - 2 * composerPadX);
  const composerCompact = $derived(composerW <= COMPOSER_FORMAT_MAX_W);

  /** How full the context ring is, growing as the reply streams in. */
  const contextFill = $derived(
    CONTEXT_START + (CONTEXT_END - CONTEXT_START) * (reply.length === 0 ? 0 : streamed.length / reply.length),
  );

  /** The effort's name as the model trigger and the ladder show it. */
  function effortLabel(level: ChatSceneEffort): string {
    return compactModelOptionLabel(level, "claude", t("chat.composer.light"), t("chat.composer.extraHigh"));
  }

  /** A stop's place on the effort ladder. */
  function stopLeft(index: number): string {
    return modelEffortStopPosition(index, CHAT_SCENE_EFFORTS.length, EFFORT_INSET_REM);
  }

  const effortIndex = $derived(CHAT_SCENE_EFFORTS.indexOf(effort));
  /** The fill reaches the knob, then the whole track once Max turns the ladder to its Max colors, as the ladder fills. */
  const fillWidth = $derived.by(() => {
    const from = CHAT_SCENE_EFFORTS.indexOf(CHAT_SCENE_START_EFFORT);
    const last = CHAT_SCENE_EFFORTS.length - 1;
    const slide = last === from ? 1 : (knobStop - from) / (last - from);
    const start = stopLeft(from);
    return slide <= 0 ? start : `calc(${start} + (100% - ${start}) * ${slide})`;
  });

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

  /** A row rising into the timeline as far as `progress` says. */
  function riseStyle(progress: number): string | undefined {
    if (progress >= 1) return undefined;
    return `opacity: ${progress}; transform: translateY(${MESSAGE_RISE_REM * (1 - progress)}rem);`;
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

<div class="rounded-3xl bg-foreground/6 p-1.5">
  <div
    bind:this={canvas}
    class="chat-scene relative flex flex-col overflow-hidden rounded-2xl bg-(--cal-bg) text-foreground"
    style:width={`${canvasW}px`}
    style:height={`${canvasH}px`}
  >
    {#if mobileShell}
      <!-- The phone's top bar, with the Novel project's context switcher. -->
      <div class="flex shrink-0 items-center gap-2 bg-background px-1.5" style:height={`${TOP_BAR_H}px`}>
        <span class="flex h-9 min-w-0 items-center gap-2 rounded-full bg-accent/55 pr-2.5 pl-2 text-identity font-medium">
          <span class="grid size-5 shrink-0 place-items-center"><Feather size={IDENTITY_ICON_SIZE} strokeWidth={IDENTITY_ICON_STROKE_WIDTH} /></span>
          <span class="truncate">{projectName}</span>
          <ChevronDown size={16} class="shrink-0 text-muted-foreground" />
        </span>
      </div>
    {:else}
      <!-- The title bar's Music. -->
      <div class="flex shrink-0 items-center justify-end gap-0.5 bg-sidebar" style:height={`${TOP_BAR_H}px`} style:padding-right={`${TITLE_BAR_END_PX}px`}>
        <span class={cn("grid size-7 place-items-center rounded-lg", TITLE_ICON_COLOR_CLASS)}>
          <Music size={TITLE_ICON_SIZE} strokeWidth={TITLE_ICON_STROKE_WIDTH} />
        </span>
      </div>
    {/if}

    <!-- The workspace header: where the channel is, its members, and the working folder's branch. -->
    <div class={cn("flex shrink-0 items-center", mobileShell ? "px-(--mobile-chrome-inset)" : "gap-1 px-3")} style:height={`${HEADER_H}px`}>
      {#if mobileShell}
        <span class="flex h-12 w-(--mobile-chrome-column) shrink-0 items-center justify-center text-foreground">
          <PanelLeft size={MOBILE_ICON_SIZE} strokeWidth={MOBILE_ICON_STROKE_WIDTH} />
        </span>
        <span class="flex h-12 min-w-0 flex-1 items-center gap-1.5 px-1.5 text-foreground">
          <Hash size={MOBILE_ICON_SIZE} strokeWidth={MOBILE_ICON_STROKE_WIDTH} class="shrink-0" />
          <span class="min-w-0 truncate text-xs font-medium">{t("vaultSetup.chat.channel")}</span>
        </span>
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
            <span class="truncate">{t("vaultSetup.chat.channel")}</span>
            <WorkspaceBreadcrumbTerminalIcon kind="chevron" context="chat" class="shrink-0 text-muted-foreground" />
          </span>
        </span>
        <span class="flex-1"></span>
      {/if}
      <span class={cn("members", !mobileShell && "h-7 rounded-md", hovered("members") && "hovered")} data-scene-hover="members">
        <span class="members-stack"><LocalPersonAvatar size={MEMBER_AVATAR_SIZE} /></span>
        <span class="members-count tabular-nums">{formatNumber(localization.locale, 1)}</span>
      </span>
      {#if !mobileShell && canvasW >= HEADER_BRANCH_MIN_W}
        <span class="header-branch"><GitBranch size={13} /><span class="truncate">{CHAT_SCENE_BRANCH}</span></span>
      {/if}
    </div>

    <div data-cursor="chat-panel" class="relative flex min-h-0 flex-1 flex-col">
      <!-- The thread, anchored to its end once it outgrows the room above the composer. -->
      <div class="flex min-h-0 flex-1 flex-col justify-end overflow-hidden">
        <div class="flex min-h-full shrink-0 flex-col px-4 pt-4 pb-2">
          {#if sent}
            <div class="participant-row" style={riseStyle(userIn)}>
              <LocalPersonAvatar size={ROW_AVATAR_SIZE} />
              <div class="participant-content">
                <div class="participant-header"><strong>{userName}</strong><span>{sentTime}</span></div>
                <div class="message">{prompt}</div>
              </div>
            </div>
          {/if}
          {#if replyIn > 0}
            <div class="participant-row mt-[1.1rem]" style={riseStyle(replyIn)}>
              <ChatModelAvatar familyId="claude" label="Claude" size={ROW_AVATAR_SIZE} />
              <div class="participant-content">
                <div class="participant-header"><strong>{CHAT_SCENE_MODEL_NAME}</strong><span>{sentTime}</span></div>
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
                  {#if streamed.length > 0}<div class="message mt-1.5">{streamed}</div>{/if}
                {/if}
              </div>
            </div>
          {/if}
        </div>
      </div>

      <!-- The composer, under the working folder strip until the first message sends and docks it under the thread. -->
      <div class="relative shrink-0" style:padding={`0 ${composerPadX}px ${composerPadBottom}px`}>
        {#if stripShown > 0}
          <div class="strip-fold" style:--strip={stripShown}>
            <div class={cn("working-folder", canvasW <= STRIP_COMPACT_MAX_W && "compact")}>
              <span class={cn("strip-item max-w-[45%]", hovered("folder") && "text-primary")} data-scene-hover="folder"><Folder size={15} /><span class="truncate">{projectName}</span></span>
              {#if canvasW > STRIP_COMPACT_MAX_W}<span class="strip-item"><Laptop size={15} />{t("chat.header.local")}</span>{/if}
              <span class="strip-item"><GitBranch size={15} /><span class="truncate">{CHAT_SCENE_BRANCH}</span></span>
            </div>
          </div>
        {/if}

        <div class="composer" style:margin-top={stripShown > 0 ? `${-0.45 * stripShown}rem` : undefined}>
          <div class="editor-frame" data-cursor="chat-editor">
            <div class="editor">
              {#if draft.length === 0}
                <span class="placeholder">{t(working ? "chat.composer.placeholderWorking" : "chat.composer.placeholder")}</span>
              {/if}{draft}{#if caretOn}<WelcomeCaret />{/if}
            </div>
          </div>
          <div class="toolbar">
            <div class="toolbar-side overflow-hidden">
              <span class={cn("tool", hovered("tool:attach") && "hovered")} data-scene-hover="tool:attach"><Plus size={15} /></span>
              <span class={cn("access", hovered("tool:access") && "hovered")} data-scene-hover="tool:access"><ShieldQuestionMark size={14} /></span>
              {#if !composerCompact}
                <span class={cn("tool", hovered("tool:bold") && "hovered")} data-scene-hover="tool:bold"><Bold size={14} /></span>
                <span class={cn("tool", hovered("tool:italic") && "hovered")} data-scene-hover="tool:italic"><Italic size={14} /></span>
              {/if}
            </div>
            <div class="toolbar-side justify-end">
              <span class="relative flex min-w-0">
                <span
                  class={cn("model-trigger", (pickerOpen || hovered("model")) && "hovered")}
                  data-cursor="chat-model"
                  data-scene-hover="model"
                >
                  <span class="truncate">{CHAT_SCENE_MODEL_NAME}</span>
                  {#if !composerCompact}
                    <span class="effort-name" style:--ultra={`${ultra * 100}%`}>{effortLabel(effort)}</span>
                  {/if}
                  <ChevronDown size={13} class="model-chevron" style={`transform: rotate(${180 * pickerShown}deg)`} />
                </span>

                {#if pickerShown > 0}
                  <!-- The model picker, open over the thread, with Claude's effort ladder. -->
                  <div
                    data-scene-cover
                    class="model-popover surface-floating w-floating"
                    style:opacity={pickerShown}
                    style:transform={pickerShown < 1 ? `translateY(${PICKER_RISE_REM * (1 - pickerShown)}rem)` : undefined}
                  >
                    <div class="pt-[0.45rem]">
                      <div class="ladder" style:--ultra={`${ultra * 100}%`} style:height={`${EFFORT_TRACK_REM}rem`}>
                        <span class="fill" style:width={fillWidth}>
                          <span class="fill-ultra" style:opacity={ultra}></span>
                        </span>
                        {#each CHAT_SCENE_EFFORTS as level, index (level)}
                          <span
                            class={cn("stop", index <= effortIndex && "reached", hovered(`stop:${level}`) && "hovered")}
                            style:left={stopLeft(index)}
                            data-cursor={`chat-effort-${level}`}
                            data-scene-hover={`stop:${level}`}
                          >
                            <i style:opacity={index < effortIndex ? 1 - ultra : undefined}></i>
                          </span>
                        {/each}
                        <span class="knob" style:left={stopLeft(knobStop)} style:transform={`translate(-50%, -50%) scale(${knobHeld ? 1.065 : 1})`}></span>
                      </div>
                      <div class="quick-actions">
                        <span class={cn("advanced", hovered("advanced") && "hovered")} data-scene-hover="advanced">
                          <span>{t("chat.composer.advanced")}</span><ChevronRight size={15} />
                        </span>
                        <span class={cn("fast", hovered("fast") && "hovered")} data-scene-hover="fast"><Zap size={16} strokeWidth={1} /></span>
                      </div>
                    </div>
                  </div>
                {/if}
              </span>
              {#if composerW > COMPOSER_RING_MAX_W}
                <span class="context-ring" aria-hidden="true">
                  <svg viewBox="0 0 18 18">
                    <circle class="track" cx="9" cy="9" r="7" />
                    <circle class="value" cx="9" cy="9" r="7" stroke-dasharray={CONTEXT_RING_LENGTH} stroke-dashoffset={CONTEXT_RING_LENGTH * (1 - contextFill)} />
                  </svg>
                </span>
              {/if}
              <span
                class="send"
                data-cursor="chat-send"
                data-scene-hover="send"
                style:transform={sendPress > 0 ? filmPressTransform(sendPress) : hovered("send") ? "scale(1.04)" : undefined}
              >
                {#if working}<Square size={13} fill="currentColor" />{:else}<ArrowUp size={16} />{/if}
                {#if mobileShell}<WelcomeTouch opacity={touches.send} />{/if}
              </span>
            </div>
          </div>
        </div>
      </div>
    </div>

    {#if !mobileShell}
      <WelcomeCursor script={CHAT_CURSOR} {elapsed} width={canvasW} bind:position={cursorPoint} />
    {/if}
  </div>
</div>

<style>
  /* Colors the effort ladder and the effort name share, in light and dark, as the model picker paints them. */
  .chat-scene {
    --ladder-track: #e4e5e9;
    --ladder-track-ultra: #ddd6fe;
    --ladder-border: color-mix(in srgb, var(--border) 88%, transparent);
    --ladder-border-ultra: color-mix(in srgb, #8b5cf6 30%, transparent);
    --ladder-fill: #0c78d0;
    --ladder-ultra: linear-gradient(105deg, #6ea8ff 0%, #7457f5 48%, #a83cf2 100%);
    --ladder-dot: color-mix(in srgb, var(--muted-foreground) 58%, #e4e5e9);
    --ladder-dot-reached: color-mix(in srgb, white 48%, #0c78d0);
    --ladder-knob: #ffffff;
    --ladder-knob-border: color-mix(in srgb, var(--foreground) 16%, transparent);
    --effort-ultra: #7c3aed;
  }
  :global(.dark) .chat-scene {
    --ladder-track: #45464a;
    --ladder-track-ultra: #4c3b78;
    --ladder-border: rgb(255 255 255 / 0.09);
    --ladder-border-ultra: rgb(139 92 246 / 0.4);
    --ladder-fill: #1681dc;
    --ladder-ultra: linear-gradient(105deg, #476cf4 0%, #7144e8 48%, #9f35d7 100%);
    --ladder-dot: color-mix(in srgb, white 32%, #45464a);
    --ladder-dot-reached: color-mix(in srgb, white 46%, #1681dc);
    --ladder-knob: #f4f4f6;
    --ladder-knob-border: rgb(255 255 255 / 0.18);
    --effort-ultra: #b794ff;
  }

  .members { display: flex; flex: 0 0 auto; align-items: center; gap: 0.3rem; padding-inline: 0.3rem 0.5rem; color: var(--foreground); }
  .members.hovered { background-color: var(--accent); }
  .members-stack { display: flex; align-items: center; }
  .members-stack > :global(span) { flex: 0 0 auto; border-radius: 22%; box-shadow: 0 0 0 1.5px var(--cal-bg); }
  .members.hovered .members-stack > :global(span) { box-shadow: 0 0 0 1.5px var(--accent); }
  .members-count { font-size: 0.733333rem; font-weight: 500; }
  .header-branch { display: inline-flex; min-width: 0; max-width: 9rem; align-items: center; gap: 0.3rem; border-radius: 0.375rem; padding: 0.25rem 0.4rem; color: var(--muted-foreground); font-size: 0.666667rem; }

  .participant-row { display: grid; min-width: 0; grid-template-columns: 36px minmax(0, 1fr); align-items: start; gap: 0.75rem; }
  .participant-content { min-width: 0; transform: translateY(-4px); }
  .participant-header { display: flex; min-height: 1.25rem; min-width: 0; align-items: baseline; gap: 0.45rem; margin-bottom: 0.12rem; line-height: 1.25rem; }
  .participant-header strong { min-width: 0; overflow: hidden; color: var(--foreground); font-size: 0.875rem; font-weight: 650; text-overflow: ellipsis; white-space: nowrap; }
  .participant-header span { flex: 0 0 auto; color: var(--muted-foreground); font-size: 0.6875rem; font-weight: 400; }
  .message { min-width: 0; color: var(--foreground); font-size: 0.875rem; line-height: 1.3125rem; overflow-wrap: anywhere; }

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

  /* The working folder strip folds down behind the composer as the first message sends. */
  .strip-fold { overflow: hidden; height: calc((3.15rem - 0.45rem) * var(--strip) + 0.45rem); opacity: var(--strip); }
  .working-folder { display: flex; min-width: 0; min-height: 3.15rem; align-items: center; gap: 1.2rem; margin-inline: 1.35rem; border-radius: 1.2rem 1.2rem 0 0; background: color-mix(in srgb, var(--muted) 72%, transparent); padding: 0.45rem 1.1rem 0.7rem; color: var(--foreground); font-size: 0.8rem; }
  .working-folder.compact { gap: 0.75rem; margin-inline: 0.65rem; }
  .working-folder.compact > .strip-item:first-child { max-width: 55%; }
  .strip-item { display: flex; min-width: 0; align-items: center; gap: 0.45rem; }
  .strip-item :global(svg) { flex: 0 0 auto; }

  .composer { position: relative; display: grid; width: 100%; border: 1px solid color-mix(in srgb, var(--border) 88%, transparent); border-radius: 1.3rem; background: var(--card); box-shadow: 0 8px 22px -18px rgb(0 0 0 / 0.24), 0 1px 4px -3px rgb(0 0 0 / 0.16); }
  .editor-frame { padding: 1rem 1.25rem 0; }
  .editor { position: relative; min-height: 2lh; overflow: hidden; color: var(--foreground); font-size: 0.875rem; line-height: 1.3125rem; overflow-wrap: anywhere; white-space: pre-wrap; }
  .placeholder { position: absolute; inset: 0 0 auto; overflow: hidden; color: color-mix(in srgb, var(--muted-foreground) 52%, transparent); text-overflow: ellipsis; white-space: nowrap; }
  .toolbar { display: grid; min-height: 2.6rem; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 0.5rem; padding: 0 0.75rem 0.5rem; }
  .toolbar-side { display: flex; min-width: 0; align-items: center; gap: 0.3rem; }
  .tool { display: inline-flex; width: 1.9rem; height: 1.9rem; flex: 0 0 auto; align-items: center; justify-content: center; border-radius: 0.5rem; color: var(--muted-foreground); }
  .access { display: inline-flex; height: 1.9rem; flex: 0 0 auto; align-items: center; border-radius: 0.55rem; padding: 0.25rem 0.35rem; color: var(--muted-foreground); }
  .tool.hovered, .access.hovered { background: var(--accent); color: var(--foreground); }

  .model-trigger { display: flex; height: 2rem; max-width: 17rem; align-items: center; gap: 0.3rem; border-radius: 999px; padding: 0.3rem 0.55rem; color: var(--foreground); font-size: 0.766667rem; white-space: nowrap; }
  .model-trigger.hovered { background: color-mix(in srgb, var(--accent) 52%, transparent); }
  .model-trigger :global(.model-chevron) { flex: 0 0 auto; margin-left: 0.2rem; color: var(--muted-foreground); }
  .effort-name { flex: 0 0 auto; color: color-mix(in srgb, var(--effort-ultra) var(--ultra), var(--primary)); }
  .model-popover { position: absolute; right: 0; bottom: calc(100% + 0.45rem); z-index: 45; overflow: visible; padding: 0.65rem 0.6rem 0.5rem; }

  .ladder { position: relative; border: 1px solid color-mix(in srgb, var(--ladder-border-ultra) var(--ultra), var(--ladder-border)); border-radius: 999px; background: color-mix(in srgb, var(--ladder-track-ultra) var(--ultra), var(--ladder-track)); }
  .fill { position: absolute; inset-block: 0; left: 0; overflow: hidden; border-radius: inherit; background: var(--ladder-fill); }
  .fill-ultra { position: absolute; inset: 0; background-image: var(--ladder-ultra); }
  .stop { position: absolute; top: 50%; display: grid; width: 2.15rem; height: 2.15rem; place-items: center; transform: translate(-50%, -50%); border-radius: 999px; }
  .stop i { width: 0.3rem; height: 0.3rem; border-radius: 999px; background: var(--ladder-dot); }
  .stop.reached i { background: var(--ladder-dot-reached); }
  .stop.hovered i { transform: scale(1.4); }
  .knob { position: absolute; top: 50%; width: 2.15rem; height: 2.15rem; border: 1px solid var(--ladder-knob-border); border-radius: 999px; background: var(--ladder-knob); }
  .quick-actions { display: flex; min-height: 2rem; align-items: center; justify-content: space-between; gap: 0.25rem; margin-top: 0.4rem; }
  .advanced { display: flex; min-width: 0; min-height: 2rem; flex: 1 1 auto; align-items: center; gap: 0.2rem; border-radius: var(--floating-item-radius); padding: 0.3rem 0.2rem; color: var(--muted-foreground); }
  .fast { display: flex; width: 2rem; height: 2rem; flex: 0 0 auto; align-items: center; justify-content: center; margin-inline: 0.375rem -0.375rem; border-radius: 0.55rem; color: var(--muted-foreground); }
  .advanced.hovered, .fast.hovered { background: color-mix(in srgb, var(--accent) 65%, transparent); color: var(--foreground); }

  .context-ring { display: grid; width: 1.45rem; height: 1.45rem; flex: 0 0 auto; place-items: center; color: var(--muted-foreground); }
  .context-ring svg { width: 1.2rem; height: 1.2rem; transform: rotate(-90deg); overflow: visible; }
  .context-ring circle { fill: none; stroke-width: 3; }
  .context-ring .track { stroke: color-mix(in srgb, var(--muted-foreground) 22%, transparent); }
  .context-ring .value { stroke: currentColor; stroke-linecap: round; }
  .send { position: relative; display: inline-flex; width: 2rem; height: 2rem; flex: 0 0 auto; align-items: center; justify-content: center; border-radius: 999px; background: color-mix(in srgb, var(--primary) 92%, transparent); color: var(--primary-foreground); box-shadow: 0 2px 7px color-mix(in srgb, var(--primary) 22%, transparent); }
</style>
