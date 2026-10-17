<script lang="ts">
  import { untrack } from "svelte";
  import AlertTriangle from "@lucide/svelte/icons/alert-triangle";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import Moon from "@lucide/svelte/icons/moon";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Sun from "@lucide/svelte/icons/sun";
  import Wand2 from "@lucide/svelte/icons/wand-2";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import { cn } from "$lib/utils";
  import {
    DERIVATION_ENGINE_VERSION,
    type Theme,
    type UserTheme,
  } from "$lib/themes";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { toUserThemeSnapshot } from "$lib/themes/operations";
  import { createThemeEditorActions } from "./actions";
  import { ThemeJsonController } from "./json-controller.svelte";
  import { ThemeContrastController } from "./contrast-controller.svelte";
  import ColorField from "$lib/components/ui/ColorField.svelte";
  import ThemeContrastNotice from "./ThemeContrastNotice.svelte";
  import ThemeEventPaletteSection from "./ThemeEventPaletteSection.svelte";
  import ThemeJsonSection from "./ThemeJsonSection.svelte";
  import ThemeRebakeBanner from "./ThemeRebakeBanner.svelte";
  import ThemeEditorNavigation from "./ThemeEditorNavigation.svelte";
  import ThemeSourcePairRow from "./ThemeSourcePairRow.svelte";
  import ThemeTokenEditor from "./ThemeTokenEditor.svelte";
  import ActionToast from "$lib/components/ui/ActionToast.svelte";
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import {
    SOURCE_GROUPS,
    isCalendarGroup,
    isTextActionGroup,
    localizedCalendarDefaultOptions,
    localizedSourceGroups,
    localizedThemeNavItems,
    localizedThemeSectionLabel,
    localizedTokenInfo,
    type GroupContrastRow,
    type GroupPairRow,
    type GroupSingleRow,
    type GroupSourcePairRow,
    type SourceGroup,
    type ThemeNavTarget,
  } from "./model";

  let { theme }: { theme: Theme } = $props();

  const themeStore = getTheme();
  const { t } = getLocalization();
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";
  const sourceGroups = $derived(localizedSourceGroups(t));
  const textActionGroups = $derived(sourceGroups.filter(isTextActionGroup));
  const calendarGroups = $derived(sourceGroups.filter(isCalendarGroup));
  const themeNavItems = $derived(localizedThemeNavItems(t));
  const calendarDefaultOptions = $derived(localizedCalendarDefaultOptions(t));

  const isBuiltin = $derived(theme.kind === "builtin");
  const readOnly = $derived(theme.kind === "builtin");
  const userTheme = $derived(
    theme.kind === "user" ? (theme as UserTheme) : undefined,
  );
  const viewTheme = $derived.by(() => toUserThemeSnapshot(theme));
  // The iconLabel icon only labels a theme as meant for day or night use. It
  // does not drive the runtime `.dark` class or palette choice. Built-ins pin
  // their iconLabel; user themes can flip it.
  const BaseIcon = $derived(theme.iconLabel === "dark" ? Moon : Sun);
  // The rebake banner appears when the saved theme's derivation engine
  // version trails the current one and the upgrade prompt for the current
  // version has not been dismissed.
  const offerRebake = $derived(
    userTheme ? themeStore.shouldOfferRebake(userTheme) : false,
  );

  let scrollViewport: HTMLDivElement | undefined = $state();
  let scrollContent: HTMLDivElement | undefined = $state();
  let navigation: ThemeEditorNavigation | undefined = $state();

  // Collapse state is not persisted. Multi-row groups start collapsed;
  // single-row source groups render their row as a peer of the header.
  let collapsed = $state<Record<string, boolean>>(
    untrack(() =>
      Object.fromEntries(
        SOURCE_GROUPS.filter((g) => g.rows.length > 1).map((g) => [
          g.id,
          true,
        ]),
      ),
    ),
  );

  function toggleGroup(id: SourceGroup["id"]) {
    collapsed[id] = !collapsed[id];
  }

  // The JSON drawer mirrors the theme's serialized form. It refreshes from the
  // store only while the draft is clean, so pending JSON edits survive form
  // field updates.
  const json = new ThemeJsonController({
    store: themeStore,
    themeId: () => theme.id,
    translate: t,
    reportError: (message, error) => { console.error(message, error); },
  });


  function setName(next: string) {
    actions.rename(next);
  }

  const actions = createThemeEditorActions({
    store: themeStore,
    themeId: () => theme.id,
    readOnly: () => readOnly,
    userTheme: () => userTheme,
  });
  const setSlot = actions.setPaletteSlot;
  const setAppToken = actions.setAppToken;
  const setCalendarToken = actions.setCalendarToken;
  const setSource = actions.setSource;
  const applyCalendarDefault = actions.applyCalendarDefault;
  const setCalendarDefaultCustom = (hex: string) =>
    actions.applyCalendarDefault("custom", hex);
  const resetCalendarDefault = actions.resetCalendarDefault;
  const isolateAppToken = actions.isolateAppToken;
  const isolateCalendarToken = actions.isolateCalendarToken;
  const relinkAppToken = actions.relinkAppToken;
  const relinkCalendarToken = actions.relinkCalendarToken;
  const canResetSource = actions.canResetSource;
  const resetSource = actions.resetSource;
  const canResetAppToken = actions.canResetAppToken;
  const resetAppToken = actions.resetAppToken;
  const canResetCalendarToken = actions.canResetCalendarToken;
  const resetCalendarToken = actions.resetCalendarToken;
  const rebake = actions.rebake;
  const dismissRebake = actions.dismissRebake;

  const contrast = new ThemeContrastController({
    theme: () => viewTheme,
    readOnly: () => readOnly,
    translate: t,
    setSource,
    setAppToken,
    setCalendarToken,
    expandGroup: (groupId) => {
      collapsed = { ...collapsed, [groupId]: false };
    },
  });
  const pairContrast = contrast.pairContrast;
  const contrastTitle = contrast.contrastTitle;
  const autoFixPair = contrast.autoFix;
  const pairKey = contrast.pairKey;
  const failingPairs = $derived(contrast.failingPairs);
  const jumpToNextFailingPair = contrast.jumpToNext;
  const fixAllFailingPairs = contrast.fixAll;

</script>

<div class="theme-editor-root relative flex h-full min-h-0 flex-col">
  <!-- Theme chrome sits above the editor scroll viewport so the scrollbar
       starts with the editable sections. -->
  <section
    class="theme-editor-chrome relative z-20 flex shrink-0 flex-col gap-1.5 border-b border-border/70 bg-sidebar px-3 py-2"
  >
    <div
      class="theme-editor-identity field flex h-9 min-w-0 items-center overflow-hidden p-0 text-[0.733333rem] text-muted-foreground"
    >
      <button
        type="button"
        onclick={() => {
          if (!readOnly) {
            themeStore.setThemeIconLabel(
              theme.id,
              theme.iconLabel === "dark" ? "light" : "dark",
            );
          }
        }}
        disabled={readOnly}
        aria-label={readOnly
          ? t(
              "settings.theme.editor.iconTagLabel",
              theme.iconLabel === "dark"
                ? t("settings.theme.editor.night")
                : t("settings.theme.editor.day"),
            )
          : t(
              "settings.theme.editor.iconTagEditableLabel",
              theme.iconLabel === "dark"
                ? t("settings.theme.editor.night")
                : t("settings.theme.editor.day"),
            )}
        title={readOnly
          ? t("settings.theme.editor.builtInIconTag")
          : t(
              "settings.theme.editor.iconTagTitle",
              theme.iconLabel === "dark"
                ? t("settings.theme.editor.night")
                : t("settings.theme.editor.day"),
            )}
        class={cn(
          "flex h-full w-9 shrink-0 items-center justify-center transition-colors focus:outline-none",
          readOnly
            ? "cursor-not-allowed"
            : "hover:bg-accent",
        )}
      >
        <BaseIcon size={12} strokeWidth={1.75} />
      </button>
      <span class="h-5 border-r border-border/70" aria-hidden="true"></span>
      <input
        type="text"
        value={theme.displayName}
        oninput={(e) => setName((e.currentTarget as HTMLInputElement).value)}
        readonly={readOnly}
        maxlength={60}
        aria-label={t("settings.theme.editor.themeName")}
        class={cn(
          "field-bare h-full px-3 font-medium text-muted-foreground",
          readOnly && "cursor-default",
        )}
      />
    </div>
    <ThemeEditorNavigation
      bind:this={navigation}
      items={themeNavItems}
      {scrollViewport}
      {scrollContent}
    />
  </section>

  {#snippet resetIconButton(
    onClick: () => void,
    label: string,
    canReset: boolean,
    disabledTitle = t("settings.theme.editor.originalValue"),
  )}
    <button
      type="button"
      disabled={!canReset}
      onclick={() => {
        if (!canReset) return;
        onClick();
      }}
      aria-disabled={!canReset}
      aria-label={t("settings.theme.editor.resetOriginal", label)}
      title={canReset ? t("settings.theme.editor.restoreOriginal") : disabledTitle}
      class={cn(
        "flex h-6.5 w-6.5 shrink-0 items-center justify-center rounded-md border border-border bg-secondary text-secondary-foreground transition-colors",
        canReset
          ? "hover:bg-accent hover:text-accent-foreground"
          : "cursor-not-allowed opacity-40",
      )}
    >
      <RotateCcw size={11} strokeWidth={2.25} />
    </button>
  {/snippet}

  {#snippet tokenEditor(
    key: string,
    scope: "app" | "cal",
    ariaLabel: string,
  )}
    {@const isolatedSet = viewTheme
      ? scope === "app"
        ? viewTheme.appIsolated
        : viewTheme.calendarIsolated
      : undefined}
    {@const isLinked = !(isolatedSet?.has(key) ?? false)}
    {@const snapshot = viewTheme
      ? scope === "app"
        ? viewTheme.appTokens
        : viewTheme.calendarTokens
      : undefined}
    {@const displayValue = snapshot?.[key] ?? ""}
    {@const canResetRow =
      scope === "app" ? canResetAppToken(key) : canResetCalendarToken(key)}
    <ThemeTokenEditor
      value={displayValue}
      label={ariaLabel}
      scope={scope === "app" ? "app" : "calendar"}
      linked={isLinked}
      {readOnly}
      canReset={canResetRow}
      onChange={(hex) => {
        if (scope === "app") setAppToken(key, hex);
        else setCalendarToken(key, hex);
      }}
      onReset={() => {
        if (scope === "app") resetAppToken(key);
        else resetCalendarToken(key);
      }}
      onIsolate={() => {
        if (scope === "app") isolateAppToken(key);
        else isolateCalendarToken(key);
      }}
      onRelink={() => {
        if (scope === "app") relinkAppToken(key);
        else relinkCalendarToken(key);
      }}
    />
  {/snippet}

  {#snippet groupSingleRow(row: GroupSingleRow)}
    {@const info = localizedTokenInfo(row, t)}
    <div class="theme-control-row flex items-center justify-between gap-3 px-1 py-2.5">
      <div class="min-w-0 flex-1">
        <div class="text-[0.8rem] text-foreground">{info.title}</div>
        <div class="text-[0.733333rem] text-muted-foreground">{info.description}</div>
      </div>
      {@render tokenEditor(row.key, row.scope, info.title)}
    </div>
  {/snippet}

  <!-- Peer-styled sub-row for single-row groups (Ink, Primary action).
       Mirrors the source header layout so the driven token reads as a peer
       of the source it tints. It still uses the normal token editor, so
       linked rows stay read-only until the user chooses Isolated edit. -->
  {#snippet groupHeaderStyleRow(row: GroupSingleRow)}
    {@const info = localizedTokenInfo(row, t)}
    <div class="theme-control-row flex items-center justify-between gap-3 px-1 py-2.5">
      <div class="min-w-0 flex-1">
        <div class="text-[0.866667rem] font-semibold text-foreground">{info.title}</div>
        <div class="text-[0.733333rem] text-muted-foreground">{info.description}</div>
      </div>
      {@render tokenEditor(row.key, row.scope, info.title)}
    </div>
  {/snippet}

  {#snippet groupPairRow(row: GroupPairRow)}
    {@const contrast = pairContrast(row)}
    <div
      data-pair-key={pairKey(row)}
      class="theme-pair-row flex flex-wrap items-start justify-between gap-x-4 gap-y-2 px-1 py-2.5"
    >
      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-1.5">
          <span class="text-[0.8rem] text-foreground">{row.title}</span>
          {#if !contrast.passes}
            <button
              type="button"
              onclick={() => {
                if (!readOnly) autoFixPair(row);
              }}
              disabled={readOnly}
              aria-label={readOnly
                ? t(
                    "settings.theme.editor.contrastReadOnlyLabel",
                    row.title,
                    contrast.ratio.toFixed(2),
                  )
                : t("settings.theme.editor.contrastAutoFixLabel", row.title)}
              title={contrastTitle(contrast)}
              class={cn(
                "flex items-center gap-1 rounded px-1 py-0.5 text-[0.666667rem] font-medium text-amber-700 transition-colors dark:text-amber-400",
                readOnly
                  ? "cursor-not-allowed opacity-75"
                  : "hover:bg-amber-500/10",
              )}
            >
              <AlertTriangle size={11} strokeWidth={2.25} />
              <span>{contrast.ratio.toFixed(1)}:1</span>
              {#if !readOnly}
                <Wand2 size={10} strokeWidth={2.25} />
              {/if}
            </button>
          {/if}
        </div>
        <div class="text-[0.733333rem] text-muted-foreground">{row.description}</div>
      </div>
      <div class="theme-pair-controls flex shrink-0 flex-col items-end gap-2">
        <div class="theme-pair-control-line flex items-center gap-1.5">
          <span
            class="theme-pair-label w-8.5 text-right text-[0.666667rem] font-medium uppercase tracking-wide text-muted-foreground"
          >
            {t("settings.theme.editor.backgroundShort")}
          </span>
          {@render tokenEditor(
            row.bg,
            row.scope,
            t("settings.theme.editor.backgroundControl", row.title),
          )}
        </div>
        <div class="theme-pair-control-line flex items-center gap-1.5">
          <span
            class="theme-pair-label w-8.5 text-right text-[0.666667rem] font-medium uppercase tracking-wide text-muted-foreground"
          >
            {t("settings.theme.editor.textShort")}
          </span>
          {@render tokenEditor(
            row.fg,
            row.scope,
            t("settings.theme.editor.textControl", row.title),
          )}
        </div>
      </div>
    </div>
  {/snippet}

  {#snippet groupSourcePairRow(row: GroupSourcePairRow)}
    {@const contrast = pairContrast(row)}
    <ThemeSourcePairRow
      {row}
      {contrast}
      contrastTitle={contrastTitle(contrast)}
      pairKey={pairKey(row)}
      {readOnly}
      sourceValue={(key) => viewTheme.sources[key]}
      {canResetSource}
      onSetSource={setSource}
      onResetSource={resetSource}
      onAutoFix={() => autoFixPair(row)}
    />
  {/snippet}

  {#snippet groupSection(group: SourceGroup)}
    {@const onlySourcePair =
      group.rows.length === 1 && group.rows[0].kind === "source-pair"
        ? group.rows[0]
        : undefined}
    {@const isCollapsible = group.rows.length > 1}
    {@const isCollapsed = isCollapsible && collapsed[group.id] === true}
    {@const showRows =
      group.rows.length > 0 && (!isCollapsible || !isCollapsed)}
    <section class="flex flex-col">
      {#if onlySourcePair}
        {@render groupSourcePairRow(onlySourcePair)}
      {:else}
        <header class="theme-group-header flex items-center justify-between gap-3 px-1 py-2.5">
          <div class="min-w-0 flex-1">
            <div class="text-[0.866667rem] font-semibold text-foreground">
              {group.title}
            </div>
            <div class="text-[0.733333rem] text-muted-foreground">
              {group.description}
            </div>
          </div>
          <div class="theme-group-controls flex shrink-0 items-center gap-1.5">
            {#if group.sourceKey !== null}
              {@const sourceKey = group.sourceKey}
              <ColorField
                value={viewTheme.sources[sourceKey]}
                onChange={(hex) => setSource(sourceKey, hex)}
                {readOnly}
                label={t("settings.theme.editor.sourceControl", group.title)}
              />
              {@render resetIconButton(
                () => resetSource(sourceKey),
                group.title,
                canResetSource(sourceKey),
                readOnly
                  ? t("settings.theme.editor.builtInReadOnly")
                  : t("settings.theme.editor.originalValue"),
              )}
            {/if}
            {#if isCollapsible}
              <button
                type="button"
                onclick={() => toggleGroup(group.id)}
                aria-expanded={!isCollapsed}
                aria-label={isCollapsed
                  ? t("settings.theme.editor.expandGroupLabel", group.title)
                  : t("settings.theme.editor.collapseGroupLabel", group.title)}
                class="theme-token-action theme-collapse-action flex min-w-27 shrink-0 items-center justify-center gap-1 rounded-md border border-border bg-card px-2 py-1 text-[0.666667rem] font-medium text-muted-foreground transition-colors hover:border-foreground/30 hover:bg-accent hover:text-foreground"
              >
                {#if isCollapsed}
                  <ChevronDown class="theme-collapse-action-icon" size={11} strokeWidth={2.25} />
                  <span>{t("settings.theme.editor.expand")}</span>
                  <ChevronDown class="theme-token-action-tail-icon" size={11} strokeWidth={2.25} />
                {:else}
                  <ChevronUp class="theme-collapse-action-icon" size={11} strokeWidth={2.25} />
                  <span>{t("settings.theme.editor.collapse")}</span>
                  <ChevronUp class="theme-token-action-tail-icon" size={11} strokeWidth={2.25} />
                {/if}
              </button>
            {:else}
              <div class="theme-token-action-spacer min-w-27 shrink-0" aria-hidden="true"></div>
            {/if}
          </div>
        </header>
        {#if showRows}
          <div class="divide-y divide-border/70 border-t border-border/70">
            {#if group.sourceKey !== null && group.rows.length === 1 && group.rows[0].kind === "single"}
              {@render groupHeaderStyleRow(group.rows[0])}
            {:else}
              {#each group.rows as row (row.kind === "single" ? row.key : row.bg)}
                {#if row.kind === "single"}
                  {@render groupSingleRow(row)}
                {:else if row.kind === "pair"}
                  {@render groupPairRow(row)}
                {:else}
                  {@render groupSourcePairRow(row)}
                {/if}
              {/each}
            {/if}
          </div>
        {/if}
      {/if}
    </section>
  {/snippet}

  {#snippet textActionsSection()}
    <section class="flex flex-col divide-y divide-border/70">
      {#each textActionGroups as group (group.id)}
        {@render groupSection(group)}
      {/each}
    </section>
  {/snippet}

  {#snippet calendarDefaultsSection()}
    <section class="flex flex-col gap-2 px-1 py-2.5">
      <header>
        <div class="min-w-0 flex-1">
          <h2 class="text-[0.866667rem] font-semibold text-foreground">
            {t("settings.theme.editor.colorDefaults")}
          </h2>
          <div class="text-[0.733333rem] text-muted-foreground">
            {t("settings.theme.editor.colorDefaultsDescription")}
          </div>
        </div>
      </header>
      <div class="theme-calendar-default-row flex min-w-0 flex-wrap items-center gap-1.5">
        {#each calendarDefaultOptions as option}
          {@const selected = viewTheme.calendarDefaultMode === option.mode}
          <button
            type="button"
            onclick={() => {
              if (!readOnly) applyCalendarDefault(option.mode);
            }}
            disabled={readOnly}
            aria-pressed={selected}
            class={cn(
              "min-h-7 rounded-md border px-2.5 py-1 text-[0.733333rem] font-medium transition-colors",
              selected
                ? "border-primary bg-primary text-primary-foreground"
                : readOnly
                  ? "border-border bg-card text-muted-foreground opacity-60"
                  : "border-border bg-card text-muted-foreground hover:border-foreground/30 hover:bg-accent hover:text-foreground",
              readOnly && "cursor-not-allowed",
            )}
          >
            {option.label}
          </button>
        {/each}
        {#if viewTheme.calendarDefaultMode === "custom"}
          <ColorField
            value={viewTheme.calendarDefaultCustom}
            onChange={setCalendarDefaultCustom}
            {readOnly}
            label={t("settings.theme.editor.customCalendarDefault")}
          />
        {/if}
        <div class="ml-auto shrink-0">
          {@render resetIconButton(
            resetCalendarDefault,
            t("settings.theme.editor.colorDefaults"),
            !readOnly && themeStore.canResetCalendarDefault(theme.id),
            readOnly
              ? t("settings.theme.editor.builtInReadOnly")
              : t("settings.theme.editor.originalValue"),
          )}
        </div>
      </div>
    </section>
  {/snippet}

  {#snippet calendarSection()}
    <section class="flex flex-col divide-y divide-border/70">
      {@render calendarDefaultsSection()}
      {#each calendarGroups as group (group.id)}
        {#if group.id === "calendar-details"}
          <ThemeEventPaletteSection
            theme={viewTheme}
            {readOnly}
            onSetSlot={setSlot}
          />
        {/if}
        {@render groupSection(group)}
      {/each}
    </section>
  {/snippet}

  {#snippet sectionHeader(target: ThemeNavTarget, note?: string)}
    <div
      class="theme-section-header flex scroll-mt-4 items-center gap-3 px-1 pt-1"
      data-theme-nav-target={target}
    >
      <h2 class="shrink-0 text-[0.866667rem] font-semibold uppercase text-foreground">
        {localizedThemeSectionLabel(target, t)}
      </h2>
      <div class="h-px min-w-4 flex-1 scale-y-50 bg-border" aria-hidden="true"></div>
      {#if note}
        <span class="shrink-0 text-[0.733333rem] text-muted-foreground">
          {note}
        </span>
      {/if}
    </div>
  {/snippet}

  <div class="relative min-h-0 flex-1">
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      bind:this={scrollViewport}
      use:scrollEdgeFadeAction
      class="theme-editor-scroll h-full overflow-y-auto focus:outline-none"
      role="region"
      aria-label={t("settings.theme.editor.controlsLabel")}
      tabindex="-1"
      onpointerdown={(event) => navigation?.focusViewportFromPointer(event)}
      onkeydown={(event) => navigation?.keepPanelScrollKey(event)}
      onscroll={() => navigation?.queueActiveSectionUpdate()}
    >
      <div
        bind:this={scrollContent}
        class={cn(
          "theme-editor-content flex flex-col gap-6 px-5 pt-6",
          !readOnly && userTheme && failingPairs.length > 0
            ? "theme-editor-content-with-notice pb-16"
            : "pb-4",
        )}
      >
        <!-- Body: grouped editor for user themes and read-only built-in views. -->
        {#if userTheme && offerRebake}
          <ThemeRebakeBanner
            savedVersion={userTheme.derivationEngineVersion}
            currentVersion={DERIVATION_ENGINE_VERSION}
            onDismiss={dismissRebake}
            onRebake={rebake}
          />
        {/if}

        {#each sourceGroups as group (group.id)}
          {#if isCalendarGroup(group)}
            {#if group.id === "calendar-surface"}
              <div class="flex flex-col gap-2">
                {@render sectionHeader("calendar")}
                {@render calendarSection()}
              </div>
            {/if}
          {:else if isTextActionGroup(group)}
            {#if group.id === "ink"}
              <div class="flex flex-col gap-2">
                {@render sectionHeader("signals")}
                {@render textActionsSection()}
              </div>
            {/if}
          {:else if group.navTarget}
            <div class="flex flex-col gap-2">
              {@render sectionHeader(group.navTarget)}
              {@render groupSection(group)}
            </div>
          {:else}
            {@render groupSection(group)}
          {/if}
        {/each}

        <div class="flex flex-col gap-2">
          {@render sectionHeader("json")}
          <ThemeJsonSection
            {isBuiltin}
            jsonDraft={json.draft}
            jsonDirty={json.dirty}
            jsonErrors={json.errors}
            jsonNotice={mobileShell ? undefined : json.notice?.message}
            jsonSaving={json.saving}
            fileSaveAvailable={json.fileSaveAvailable}
            onCopy={json.copy}
            onSave={json.save}
            onApply={json.apply}
            onReset={json.reset}
            onInput={json.input}
          />
        </div>
      </div>
      <CalendarScrollbar scrollContainer={scrollViewport} wheelPassthrough />
    </div>
    {#if !readOnly && userTheme && failingPairs.length > 0}
      <ThemeContrastNotice
        count={failingPairs.length}
        onJump={jumpToNextFailingPair}
        onFixAll={fixAllFailingPairs}
      />
    {/if}
  </div>

  {#if mobileShell && json.notice}
    <ActionToast
      message={json.notice.message}
      variant={json.notice.variant}
      dismissLabel={t("settings.theme.editor.dismissFileNotification")}
      onDismiss={json.dismissNotice}
    />
  {/if}
</div>

<style>
  .theme-editor-root {
    container: theme-editor / inline-size;
  }

  .theme-editor-scroll {
    scrollbar-width: none;
  }

  .theme-editor-scroll::-webkit-scrollbar {
    width: 0;
    height: 0;
    display: none;
  }

  @container theme-editor (max-width: 620px) {
    .theme-editor-chrome {
      padding-inline: 0.625rem;
    }

    .theme-editor-content {
      gap: 1rem;
      padding-inline: 0.875rem;
      padding-top: 1rem;
    }

    .theme-control-row,
    .theme-group-header {
      align-items: stretch;
      flex-direction: column;
      gap: 0.5rem;
    }

    .theme-row-controls,
    .theme-group-controls {
      flex-wrap: wrap;
      justify-content: flex-start;
      width: 100%;
    }

    .theme-pair-row {
      flex-direction: column;
    }

    .theme-pair-controls {
      align-items: stretch;
      width: 100%;
    }

    .theme-pair-control-line {
      display: grid;
      grid-template-columns: 2.25rem minmax(0, 1fr);
      align-items: center;
      justify-content: flex-start;
    }

    .theme-pair-label {
      width: auto;
    }

    .theme-token-editor,
    .theme-source-editor {
      justify-content: flex-start;
    }

    .theme-calendar-default-row {
      align-items: center;
    }

  }

  @container theme-editor (max-width: 430px) {
    .theme-editor-content {
      gap: 0.875rem;
      padding-inline: 0.625rem;
    }

    .theme-section-header {
      flex-wrap: wrap;
    }

    .theme-token-action {
      min-width: 5.25rem;
      padding-inline: 0.5rem;
      width: auto;
    }

    .theme-collapse-action {
      min-width: 4.75rem;
    }

    .theme-collapse-action-icon,
    .theme-token-action-tail-icon,
    .theme-token-action-spacer {
      display: none;
    }

    .theme-token-editor,
    .theme-source-editor {
      flex-wrap: wrap;
      min-width: 0;
      justify-content: flex-start;
    }

  }

  @container theme-editor (max-width: 380px) {
    .theme-editor-content-with-notice {
      padding-bottom: 7rem;
    }

  }

  @container theme-editor (max-width: 330px) {
    .theme-editor-content {
      padding-inline: 0.5rem;
    }
  }
</style>
