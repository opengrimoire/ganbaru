<script lang="ts">
  import Ban from "@lucide/svelte/icons/ban";
  import Check from "@lucide/svelte/icons/check";
  import Coffee from "@lucide/svelte/icons/coffee";
  import FolderKanban from "@lucide/svelte/icons/folder-kanban";
  import Gamepad2 from "@lucide/svelte/icons/gamepad-2";
  import PomodoroProgressRing from "$lib/components/pomodoro/PomodoroProgressRing.svelte";
  import type { EventColor } from "$lib/calendar/types";
  import { getEventColor } from "$lib/calendar/utils";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { cn } from "$lib/utils";
  import {
    FILM_BEATS,
    FILM_PULL_BACK_MS,
    FILM_SESSION_MINUTES,
    easeInOutCubic,
    filmBreakSpans,
    filmSessionMinute,
    filmSessionMoment,
    filmSpan,
    formatFilmCountdown,
  } from "$lib/vault/welcome-film";

  /**
   * One frame of the welcome film, drawn from the Calendar's own surfaces: a day column with a Focus rail, the current
   * time line, and a floating panel. Everything is laid out on a fixed canvas so the choreography lands on exact
   * points; the player scales the canvas to fit.
   */
  let { elapsed }: { elapsed: number } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const theme = getTheme();

  /** Canvas geometry in logical pixels. The visible day runs from `FIRST_HOUR` for `HOURS` hours. */
  const CANVAS_W = 360;
  const HEADER_H = 34;
  const HOUR_PX = 45;
  const HOURS = 6;
  const FIRST_HOUR = 9;
  const BODY_H = HOUR_PX * HOURS;
  const CANVAS_H = HEADER_H + BODY_H;
  const GUTTER_W = 34;
  const BLOCK_RIGHT = 3;
  const BADGE = 14;
  const BADGE_INSET = 4;
  const PANEL_W = 232;
  const PANEL_H = 52;
  const PANEL_INSET = 12;
  const PANEL_PAD = 12;
  const PANEL_BAR_BOTTOM = 10;
  const PANEL_BAR_H = 4;

  /** The project the session counts toward, before and after it lands. */
  const PROJECT_TASKS = 8;
  const PROJECT_DONE_BEFORE = 3;

  /** Days shown once the view pulls back, as offsets from today. Only today is visible before that. */
  const DAY_OFFSETS = [-3, -2, -1, 0, 1] as const;

  /** Palette slots, so the drawing follows the active theme's event colors. */
  const BLUE = 18;
  const PURPLE = 22;
  const ORANGE = 7;
  const CYAN = 16;
  const VIOLET = 23;

  type FocusRail = "live" | "done" | "planned";
  interface FilmBlock {
    start: number;
    end: number;
    color: EventColor;
    rail?: FocusRail;
  }

  /** The other days' blocks, in minutes from the first hour. Past sessions are done; tomorrow's are planned. */
  const OTHER_DAYS: Record<number, readonly FilmBlock[]> = {
    [-3]: [{ start: 0, end: 90, color: CYAN, rail: "done" }, { start: 150, end: 195, color: ORANGE }],
    [-2]: [{ start: 30, end: 150, color: BLUE, rail: "done" }],
    [-1]: [{ start: 0, end: 60, color: PURPLE, rail: "done" }, { start: 90, end: 150, color: CYAN, rail: "done" }],
    1: [{ start: 0, end: FILM_SESSION_MINUTES, color: BLUE, rail: "planned" }, { start: 165, end: 200, color: PURPLE }],
  };

  const today = new Date();
  const days = DAY_OFFSETS.map((offset) => {
    const date = new Date(today.getFullYear(), today.getMonth(), today.getDate() + offset);
    return { offset, date };
  });
  const hourLabels = Array.from({ length: HOURS - 1 }, (_, index) => index + 1);

  const minute = $derived(filmSessionMinute(elapsed));
  const created = $derived(easeInOutCubic(filmSpan(elapsed, FILM_BEATS.createStart, FILM_BEATS.createEnd)));
  const pressing = $derived(elapsed >= FILM_BEATS.createStart - 120 && elapsed < FILM_BEATS.createEnd + 160);
  const focusing = $derived(elapsed >= FILM_BEATS.focusStart);
  const done = $derived(elapsed >= FILM_BEATS.sessionDone);
  /** The running phase, held on its last second until the session is marked done. */
  const running = $derived.by(() => {
    const moment = filmSessionMoment(Math.min(minute, FILM_SESSION_MINUTES - 0.01));
    return moment.kind === "done" ? null : moment;
  });
  const breaks = $derived(filmBreakSpans(minute));
  const distraction = $derived(elapsed >= FILM_BEATS.distractionIn && elapsed < FILM_BEATS.distractionOut);
  const opening = $derived(elapsed >= FILM_BEATS.distractionOpen && elapsed < FILM_BEATS.distractionBlocked);
  const blocked = $derived(elapsed >= FILM_BEATS.distractionBlocked);
  const panel = $derived(elapsed < FILM_BEATS.sessionDone ? "session" : elapsed < FILM_BEATS.projectIn ? "done" : "project");
  const sparkFlying = $derived(elapsed >= FILM_BEATS.sparkStart);
  const landed = $derived(elapsed >= FILM_BEATS.sparkLand);
  const pulledBack = $derived(elapsed >= FILM_BEATS.pullBack);
  const settled = $derived(elapsed >= FILM_BEATS.end);
  const projectDone = $derived(landed ? PROJECT_DONE_BEFORE + 1 : PROJECT_DONE_BEFORE);

  const session = $derived(getEventColor(BLUE, theme.current));
  const project = $derived(getEventColor(CYAN, theme.current));
  const distractingApp = $derived(getEventColor(VIOLET, theme.current));

  /** The spark leaves the session's done badge and lands where the project's progress is about to grow. */
  const panelLeft = (CANVAS_W - PANEL_W) / 2;
  const barWidth = PANEL_W - 2 * PANEL_PAD;
  const sparkFrom = { x: CANVAS_W - BLOCK_RIGHT - BADGE_INSET - BADGE / 2, y: HEADER_H + BADGE_INSET + BADGE / 2 };
  const sparkTo = {
    x: panelLeft + PANEL_PAD + (barWidth * (PROJECT_DONE_BEFORE + 1)) / PROJECT_TASKS,
    y: CANVAS_H - PANEL_INSET - PANEL_BAR_BOTTOM - PANEL_BAR_H / 2,
  };
  const sparkPath = `path("M ${sparkFrom.x} ${sparkFrom.y} C ${sparkFrom.x + 8} ${sparkFrom.y + 110}, ${sparkTo.x + 70} ${sparkTo.y - 30}, ${sparkTo.x} ${sparkTo.y}")`;

  function px(minutes: number): number {
    return (minutes * HOUR_PX) / 60;
  }

  function hourLabel(hour: number): string {
    return formatDateTime(localization.locale, new Date(2000, 0, 1, FIRST_HOUR + hour), { hour: "numeric" });
  }

  function timeRange(start: number, end: number): string {
    const base = new Date(2000, 0, 1, FIRST_HOUR);
    const from = new Date(base.getTime() + start * 60_000);
    const to = new Date(base.getTime() + end * 60_000);
    return new Intl.DateTimeFormat(localization.locale, { hour: "numeric", minute: "2-digit" }).formatRange(from, to);
  }
</script>

{#snippet rail(kind: FocusRail, length: number, fill: number, bands: readonly { start: number; end: number }[])}
  <div
    class="absolute left-0.5 top-0 w-1 overflow-hidden rounded-[3px] bg-(--cal-timeline-rail)"
    style:height={`${px(length)}px`}
  >
    {#if kind !== "planned"}
      <div
        class="absolute inset-x-0 top-0 h-full origin-top bg-(--cal-timeline-focus)"
        style:transform={`scaleY(${fill})`}
      ></div>
      {#each bands as band (band.start)}
        <div
          class="absolute inset-x-0 bg-(--cal-timeline-break)"
          style:top={`${(band.start / length) * 100}%`}
          style:height={`${((band.end - band.start) / length) * 100}%`}
        ></div>
      {/each}
    {/if}
  </div>
{/snippet}

{#snippet otherBlock(block: FilmBlock)}
  {@const color = getEventColor(block.color, theme.current)}
  <div class="absolute inset-x-0" style:top={`${px(block.start)}px`} style:height={`${px(block.end - block.start)}px`}>
    {#if block.rail}
      {@render rail(block.rail, block.end - block.start, 1, block.end - block.start > 60 ? [{ start: 50, end: 60 }] : [])}
    {/if}
    <div
      class={cn("absolute inset-y-0 right-0.75 flex flex-col gap-1 rounded px-1 py-1.5", block.rail ? "left-2.25" : "left-0.75")}
      style:background-color={color.bg}
      style:color={color.text}
    >
      <span class="h-1 w-4/5 rounded-full bg-current opacity-60"></span>
      <span class="h-1 w-1/2 rounded-full bg-current opacity-40"></span>
    </div>
  </div>
{/snippet}

{#snippet todayBlock(start: number, end: number, color: EventColor, title: string, shown: boolean)}
  {@const entry = getEventColor(color, theme.current)}
  <div
    class={cn(
      "absolute left-0.75 right-0.75 overflow-hidden rounded px-1.5 py-1 transition-[opacity,translate] duration-500 ease-out motion-reduce:transition-none",
      shown ? "translate-y-0 opacity-100" : "-translate-y-1.5 opacity-0",
    )}
    style:top={`${px(start)}px`}
    style:height={`${px(end - start)}px`}
    style:background-color={entry.bg}
    style:color={entry.text}
  >
    <p class="truncate text-[0.6875rem] font-medium leading-tight">{title}</p>
    <p class="truncate text-[0.625rem] leading-tight opacity-85">{timeRange(start, end)}</p>
  </div>
{/snippet}

<!-- The canvas keeps its logical size inside a one pixel border, so the spark path matches the layout exactly. -->
<div class="relative" style:width={`${CANVAS_W + 2}px`} style:height={`${CANVAS_H + 2}px`}>
  <!-- A soft light behind the canvas that takes the color of what the film is about: the session, then its result. -->
  <div class="pointer-events-none absolute -inset-10">
    <div
      class="absolute inset-0 transition-opacity duration-1000 ease-out motion-reduce:transition-none"
      style:background={`radial-gradient(closest-side, color-mix(in srgb, ${session.bg} 22%, transparent), transparent)`}
      style:opacity={focusing && !done ? 1 : 0}
    ></div>
    <div
      class="absolute inset-0 transition-opacity duration-1000 ease-out motion-reduce:transition-none"
      style:background="radial-gradient(closest-side, color-mix(in srgb, var(--cal-timeline-focus) 20%, transparent), transparent)"
      style:opacity={done ? 1 : 0}
    ></div>
  </div>

  <div
    class="film-canvas relative box-content flex overflow-hidden rounded-2xl border border-border bg-(--cal-bg) text-foreground shadow-xl"
    style:width={`${CANVAS_W}px`}
    style:height={`${CANVAS_H}px`}
  >
    <!-- The time gutter. -->
    <div class="relative shrink-0" style:width={`${GUTTER_W}px`}>
      <div class="absolute inset-x-0 bottom-0" style:top={`${HEADER_H}px`}>
        {#each hourLabels as hour (hour)}
          <span
            class="film-fade absolute right-1.5 -translate-y-1/2 text-[0.5625rem] leading-none text-(--cal-time-label)"
            style:top={`${px(hour * 60)}px`}
          >{hourLabel(hour)}</span>
        {/each}
      </div>
    </div>

    <div class="relative flex min-w-0 flex-1">
      <div class="film-fade pointer-events-none absolute inset-x-0 bottom-0" style:top={`${HEADER_H}px`}>
        {#each hourLabels as hour (hour)}
          <div class="absolute inset-x-0 h-px bg-(--cal-gridline)" style:top={`${px(hour * 60)}px`}></div>
        {/each}
      </div>

      {#each days as day (day.offset)}
        {@const isToday = day.offset === 0}
        <div
          class={cn(
            "film-column relative flex min-w-0 basis-0 flex-col overflow-hidden border-l",
            isToday || pulledBack ? "border-(--cal-gridline)" : "border-transparent",
          )}
          style:flex-grow={isToday || pulledBack ? 1 : 0}
          style:transition-duration={`${FILM_PULL_BACK_MS}ms`}
        >
          <div
            class={cn(
              "flex shrink-0 items-center justify-center gap-1 whitespace-nowrap text-[0.625rem]",
              !isToday && "film-reveal",
              !isToday && (pulledBack ? "opacity-100" : "opacity-0"),
            )}
            style:height={`${HEADER_H}px`}
          >
            <span class="text-(--cal-time-label)">{formatDateTime(localization.locale, day.date, { weekday: "short" })}</span>
            <span
              class={cn(
                "flex size-5 items-center justify-center rounded-full font-medium",
                isToday ? "bg-primary text-primary-foreground" : "text-foreground",
              )}
            >{formatDateTime(localization.locale, day.date, { day: "numeric" })}</span>
          </div>

          <div class="relative min-h-0 flex-1">
            {#if isToday}
              <!-- The session being planned, focused on, and finished. -->
              <div
                class="absolute inset-x-0 top-0"
                style:height={`${px(FILM_SESSION_MINUTES)}px`}
              >
                <div
                  class={cn(
                    "absolute inset-y-0 left-0 w-2 origin-top transition-transform duration-500 ease-out motion-reduce:transition-none",
                    elapsed >= FILM_BEATS.railIn ? "scale-y-100" : "scale-y-0",
                  )}
                >
                  {@render rail("live", FILM_SESSION_MINUTES, minute / FILM_SESSION_MINUTES, breaks)}
                </div>
                <div
                  class="absolute inset-y-0 left-2.25 overflow-hidden rounded px-1.5 py-1"
                  style:right={`${BLOCK_RIGHT}px`}
                  style:background-color={session.bg}
                  style:color={session.text}
                  style:clip-path={`inset(0 0 ${(1 - created) * 100}% 0 round 4px)`}
                >
                  <div
                    class={cn(
                      "transition-opacity duration-300 motion-reduce:transition-none",
                      elapsed >= FILM_BEATS.createEnd ? "opacity-100" : "opacity-0",
                    )}
                  >
                    <p class="truncate pr-4 text-[0.6875rem] font-medium leading-tight">{t("vaultSetup.film.writeChapter")}</p>
                    <p class="truncate text-[0.625rem] leading-tight opacity-85">{timeRange(0, FILM_SESSION_MINUTES)}</p>
                  </div>
                  <span
                    class={cn(
                      "absolute flex items-center justify-center rounded-full bg-white/90 text-(--cal-timeline-focus) transition-[scale,opacity] duration-400 ease-[cubic-bezier(0.34,1.56,0.64,1)] motion-reduce:transition-none",
                      done ? "scale-100 opacity-100" : "scale-50 opacity-0",
                    )}
                    style:top={`${BADGE_INSET}px`}
                    style:right={`${BADGE_INSET}px`}
                    style:width={`${BADGE}px`}
                    style:height={`${BADGE}px`}
                  >
                    <Check class="size-2.5" strokeWidth={3.5} aria-hidden="true" />
                  </span>
                </div>
              </div>

              {@render todayBlock(120, 165, PURPLE, t("vaultSetup.film.reviewNotes"), elapsed >= FILM_BEATS.reviewIn)}
              {@render todayBlock(210, 255, ORANGE, t("vaultSetup.film.lunch"), elapsed >= FILM_BEATS.lunchIn)}

              <!-- The press that draws the session, following its growing edge. -->
              <span
                class={cn(
                  "pointer-events-none absolute left-3/5 size-6 -translate-1/2 rounded-full bg-foreground/20 ring-2 ring-foreground/35 transition-[opacity,scale] duration-200 motion-reduce:hidden",
                  pressing ? "scale-100 opacity-100" : "scale-150 opacity-0",
                )}
                style:top={`${created * px(FILM_SESSION_MINUTES)}px`}
              ></span>

              <!-- The current time, sweeping through the session as Focus runs. -->
              <div
                class={cn(
                  "pointer-events-none absolute inset-x-0 top-0 z-1 origin-left transition-[scale,opacity] duration-400 ease-out motion-reduce:transition-none",
                  focusing ? "scale-x-100 opacity-100" : "scale-x-0 opacity-0",
                )}
                style:transform={`translateY(${px(minute)}px)`}
              >
                <div class="h-0.5 -translate-y-1/2 bg-(--cal-current-time)"></div>
                <span
                  class={cn("absolute -left-1 top-0 size-2 -translate-y-1/2 rounded-full bg-(--cal-current-time)", settled && "film-breathe")}
                ></span>
              </div>
            {:else}
              <div
                class={cn("film-reveal absolute inset-0", pulledBack ? "opacity-100" : "opacity-0")}
              >
                {#each OTHER_DAYS[day.offset] ?? [] as block (block.start)}
                  {@render otherBlock(block)}
                {/each}
              </div>
            {/if}
          </div>
        </div>
      {/each}
    </div>

    <!-- A distracting app starts to open over the session, is blocked, and falls away. -->
    <div
      class={cn(
        "pointer-events-none absolute inset-0 bg-background/40 transition-opacity duration-300 motion-reduce:transition-none",
        distraction ? "opacity-100" : "opacity-0",
      )}
    ></div>
    <div
      class={cn(
        "pointer-events-none absolute left-1/2 flex -translate-x-1/2 flex-col items-center gap-2 transition-[opacity,translate,scale] motion-reduce:transition-none",
        distraction
          ? "translate-y-0 scale-100 opacity-100 duration-300 ease-[cubic-bezier(0.34,1.56,0.64,1)]"
          : blocked
            ? "translate-y-4 scale-90 opacity-0 duration-400 ease-in"
            : "translate-y-0 scale-50 opacity-0 duration-300",
      )}
      style:top={`${HEADER_H + 62}px`}
    >
      <div class="relative flex size-11 items-center justify-center">
        <!-- The app's window grows out of its icon as it opens, then snaps back when it is blocked. -->
        <span
          class={cn(
            "absolute left-1/2 top-1/2 h-24 w-36 -translate-1/2 rounded-xl border border-border bg-card shadow-lg transition-[scale,opacity] motion-reduce:transition-none",
            opening
              ? "scale-100 opacity-100 duration-300 ease-out"
              : "scale-30 opacity-0 duration-200 ease-in",
          )}
        ></span>
        <span
          class={cn("relative flex size-11 items-center justify-center rounded-[22%] shadow-md", blocked && "film-shake")}
          style:background-color={distractingApp.bg}
          style:color={distractingApp.text}
        >
          <Gamepad2 class="size-5.5" strokeWidth={1.8} aria-hidden="true" />
          <span
            class={cn(
              "absolute -right-1.5 -top-1.5 flex size-5 items-center justify-center rounded-full bg-destructive text-destructive-foreground ring-2 ring-(--cal-bg) transition-[scale,opacity] duration-300 ease-[cubic-bezier(0.34,1.56,0.64,1)] motion-reduce:transition-none",
              blocked ? "scale-100 opacity-100" : "scale-0 opacity-0",
            )}
          >
            <Ban class="size-3" strokeWidth={2.6} aria-hidden="true" />
          </span>
        </span>
      </div>
      <span
        class={cn(
          "rounded-full bg-card px-2 py-0.5 text-[0.625rem] font-medium text-destructive shadow-sm transition-[opacity,translate] duration-300 motion-reduce:transition-none",
          blocked ? "translate-y-0 opacity-100" : "-translate-y-1 opacity-0",
        )}
      >{t("vaultSetup.film.blocked")}</span>
    </div>

    <!-- The floating panel: the running session, then its result, then the project it counts toward. -->
    <div
      class={cn(
        "surface-floating absolute left-1/2 grid -translate-x-1/2 overflow-hidden rounded-xl transition-[opacity,translate,scale] duration-500 ease-out motion-reduce:transition-none",
        focusing ? "translate-y-0 scale-100 opacity-100" : "translate-y-3 scale-95 opacity-0",
      )}
      style:bottom={`${PANEL_INSET}px`}
      style:width={`${PANEL_W}px`}
      style:height={`${PANEL_H}px`}
    >
      {#if blocked && !done}
        <span class="film-pulse pointer-events-none absolute inset-0 rounded-xl"></span>
      {/if}
      {#if landed}
        <span class="film-pulse pointer-events-none absolute inset-0 rounded-xl"></span>
      {/if}

      <div
        class={cn(
          "col-start-1 row-start-1 flex items-center gap-2.5 transition-[opacity,translate] duration-300 motion-reduce:transition-none",
          panel === "session" ? "translate-y-0 opacity-100" : "-translate-y-2 opacity-0",
        )}
        style:padding-inline={`${PANEL_PAD}px`}
      >
        <span class="relative flex size-7 shrink-0 items-center justify-center">
          <PomodoroProgressRing
            active
            remainingSeconds={running?.remainingSeconds ?? 0}
            totalSeconds={running?.totalSeconds ?? 1}
            size={28}
            strokeWidth={2.2}
            trackClass="stroke-foreground/12"
            progressClass={running?.kind === "break" ? "stroke-foreground/55" : "stroke-(--cal-timeline-focus)"}
          />
          {#if running?.kind === "break"}
            <Coffee class="absolute size-3 text-muted-foreground" strokeWidth={2} aria-hidden="true" />
          {/if}
        </span>
        <span class="flex min-w-0 flex-1 flex-col">
          <span class="truncate text-xs font-semibold leading-tight">
            {running?.kind === "break" ? t("vaultSetup.film.break") : t("vaultSetup.film.focus")}
          </span>
          <span class="truncate text-[0.625rem] leading-tight text-muted-foreground">{t("vaultSetup.film.writeChapter")}</span>
        </span>
        <span class="shrink-0 text-sm font-semibold tabular-nums">
          {formatFilmCountdown(running?.remainingSeconds ?? 0)}
        </span>
      </div>

      <div
        class={cn(
          "col-start-1 row-start-1 flex items-center gap-2.5 transition-[opacity,translate] duration-300 motion-reduce:transition-none",
          panel === "done" ? "translate-y-0 opacity-100" : panel === "session" ? "translate-y-2 opacity-0" : "-translate-y-2 opacity-0",
        )}
        style:padding-inline={`${PANEL_PAD}px`}
      >
        <span class="flex size-7 shrink-0 items-center justify-center rounded-full bg-(--cal-timeline-focus) text-white">
          <Check class="size-4" strokeWidth={3} aria-hidden="true" />
        </span>
        <span class="flex min-w-0 flex-1 flex-col">
          <span class="truncate text-xs font-semibold leading-tight">{t("vaultSetup.film.sessionDone")}</span>
          <span class="truncate text-[0.625rem] leading-tight text-muted-foreground">{t("vaultSetup.film.writeChapter")}</span>
        </span>
      </div>

      <div
        class={cn(
          "relative col-start-1 row-start-1 flex items-start gap-2.5 pt-2 transition-[opacity,translate] duration-300 motion-reduce:transition-none",
          panel === "project" ? "translate-y-0 opacity-100" : "translate-y-2 opacity-0",
        )}
        style:padding-inline={`${PANEL_PAD}px`}
      >
        <span
          class="flex size-6 shrink-0 items-center justify-center rounded-md"
          style:background-color={`color-mix(in srgb, ${project.bg} 18%, transparent)`}
          style:color={project.bg}
        >
          <FolderKanban class="size-3.5" strokeWidth={2} aria-hidden="true" />
        </span>
        <span class="min-w-0 flex-1 truncate pt-0.5 text-xs font-semibold leading-tight">{t("vaultSetup.film.project")}</span>
        <span class="shrink-0 pt-0.5 text-[0.625rem] tabular-nums text-muted-foreground">
          {t("vaultSetup.film.tasksDone", projectDone, PROJECT_TASKS)}
        </span>
        <span
          class="absolute overflow-hidden rounded-full bg-foreground/10"
          style:left={`${PANEL_PAD}px`}
          style:right={`${PANEL_PAD}px`}
          style:bottom={`${PANEL_BAR_BOTTOM}px`}
          style:height={`${PANEL_BAR_H}px`}
        >
          <span
            class="block h-full origin-left rounded-full transition-transform duration-700 ease-out motion-reduce:transition-none"
            style:background-color={project.bg}
            style:transform={`scaleX(${projectDone / PROJECT_TASKS})`}
          ></span>
        </span>
      </div>
    </div>

    <!-- The finished session flies into the project's progress, with a short tail. -->
    {#each [0, 1, 2] as trail (trail)}
      <span
        class={cn(
          "film-spark pointer-events-none absolute left-0 top-0 rounded-full motion-reduce:hidden",
          sparkFlying && !landed ? "opacity-100" : "opacity-0",
        )}
        style:offset-path={sparkPath}
        style:offset-distance={sparkFlying ? "100%" : "0%"}
        style:transition-delay={`${trail * 45}ms`}
        style:width={`${8 - trail * 2}px`}
        style:height={`${8 - trail * 2}px`}
        style:background-color={session.bg}
        style:box-shadow={trail === 0 ? `0 0 0 3px color-mix(in srgb, ${session.bg} 30%, transparent)` : undefined}
        style:transition-duration={`${FILM_BEATS.sparkLand - FILM_BEATS.sparkStart}ms, 150ms`}
      ></span>
    {/each}
  </div>
</div>

<style>
  .film-canvas {
    animation: film-enter 800ms cubic-bezier(0.2, 0, 0, 1) both;
  }

  .film-fade {
    animation: film-fade 700ms ease-out 250ms both;
  }

  .film-column {
    transition-property: flex-grow, border-color;
    transition-timing-function: cubic-bezier(0.65, 0, 0.35, 1);
  }

  .film-reveal {
    transition: opacity 900ms ease-out 250ms;
  }

  .film-spark {
    offset-rotate: 0deg;
    offset-anchor: center;
    transition-property: offset-distance, opacity;
    transition-timing-function: cubic-bezier(0.5, 0, 0.25, 1), linear;
  }

  .film-shake {
    animation: film-shake 420ms cubic-bezier(0.36, 0.07, 0.19, 0.97) both;
  }

  .film-pulse {
    animation: film-pulse 700ms ease-out both;
  }

  .film-breathe {
    animation: film-breathe 2400ms ease-in-out infinite;
  }

  @keyframes film-enter {
    from { opacity: 0; translate: 0 14px; scale: 0.97; }
    to { opacity: 1; translate: 0 0; scale: 1; }
  }

  @keyframes film-fade {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  @keyframes film-shake {
    0%, 100% { translate: 0 0; }
    20% { translate: -4px 0; rotate: -6deg; }
    40% { translate: 4px 0; rotate: 5deg; }
    60% { translate: -3px 0; rotate: -3deg; }
    80% { translate: 2px 0; rotate: 1deg; }
  }

  @keyframes film-pulse {
    from { box-shadow: inset 0 0 0 2px color-mix(in srgb, var(--cal-timeline-focus) 70%, transparent); }
    to { box-shadow: inset 0 0 0 2px transparent; }
  }

  @keyframes film-breathe {
    0%, 100% { scale: 1; opacity: 1; }
    50% { scale: 1.6; opacity: 0.55; }
  }

  @media (prefers-reduced-motion: reduce) {
    .film-canvas, .film-fade, .film-shake, .film-pulse, .film-breathe { animation: none; }
    .film-column, .film-reveal { transition: none; }
  }
</style>
