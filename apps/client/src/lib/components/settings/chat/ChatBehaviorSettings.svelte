<script lang="ts">
  import { CHAT_BEHAVIOR_DEFAULTS, type ChatBehaviorPreferences } from "$lib/chat/contracts";
  import { readTimelineDensity, timelineDensityOptions } from "$lib/chat/timeline/density";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import {
    DEFAULT_SIDEBAR_SORT_ORDER,
    isSidebarSortOrder,
    SIDEBAR_SORT_ORDER_LABEL_KEYS,
    SIDEBAR_SORT_ORDERS,
  } from "$lib/utils/sidebar-order";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import SwitchField from "$lib/components/ui/SwitchField.svelte";
  import ChatAppAgentSettings from "./ChatAppAgentSettings.svelte";

  /** Chat behavior preferences as the general, agents, terminal, and notification sections of Chat settings. */
  const localization = getLocalization();
  const { t } = localization;
  const chat = getChat();
  const preferences = getPreferences();
  const sidebarOrderOptions = $derived(
    SIDEBAR_SORT_ORDERS.map((order) => ({ value: order, label: t(SIDEBAR_SORT_ORDER_LABEL_KEYS[order]) })),
  );
  let saving = $state(false);
  let error = $state<string | null>(null);
  const behavior = $derived(chat.settings?.configuration.behavior ?? null);
  const sendKeyOptions = $derived([
    { value: "enter", label: t("settings.chat.behavior.enter") },
    { value: "mod_enter", label: t("settings.chat.behavior.modEnter") },
  ]);
  const densityOptions = $derived(timelineDensityOptions(t));
  const scrollbackOptions = $derived([1_000, 5_000, 10_000, 25_000, 50_000, 100_000].map((value) => ({
    value: String(value),
    label: formatNumber(localization.locale, value),
  })));
  const idleTimeoutOptions = $derived([
    { value: "60", label: t("settings.chat.behavior.oneMinute") },
    { value: "300", label: t("settings.chat.behavior.fiveMinutes") },
    { value: "900", label: t("settings.chat.behavior.fifteenMinutes") },
    { value: "1800", label: t("settings.chat.behavior.thirtyMinutes") },
    { value: "3600", label: t("settings.chat.behavior.oneHour") },
    { value: "7200", label: t("settings.chat.behavior.twoHours") },
  ]);

  async function update(patch: Partial<ChatBehaviorPreferences>): Promise<void> {
    if (!behavior || saving) return;
    saving = true;
    error = null;
    try {
      await chat.updateBehavior({ ...behavior, ...patch });
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chatErrors.actions.settingsAction"));
    } finally {
      saving = false;
    }
  }

  /** Whether the field differs from the fresh vault value, so its reset button is offered. */
  function changed<Key extends keyof ChatBehaviorPreferences>(current: ChatBehaviorPreferences, key: Key): boolean {
    return current[key] !== CHAT_BEHAVIOR_DEFAULTS[key];
  }

  function reset<Key extends keyof ChatBehaviorPreferences>(key: Key): void {
    const patch: Partial<ChatBehaviorPreferences> = {};
    patch[key] = CHAT_BEHAVIOR_DEFAULTS[key];
    void update(patch);
  }
</script>

<div class="flex flex-col gap-6">
  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.chat.behavior.heading")}</h2>
    {#if error}<Notice class="px-1 text-[0.733333rem]" message={error} />{/if}
    <div class="flex flex-col gap-3">
      <Select
        label={t("settings.chat.behavior.sidebarOrder")}
        description={t("settings.chat.behavior.sidebarOrderDescription")}
        value={preferences.chatSidebarOrder}
        options={sidebarOrderOptions}
        onChange={(value) => { if (isSidebarSortOrder(value)) preferences.setChatSidebarOrder(value); }}
        canReset={preferences.chatSidebarOrder !== DEFAULT_SIDEBAR_SORT_ORDER}
        onReset={() => preferences.resetChatSidebarOrder()}
      />
      {#if behavior}
        <Select
          label={t("settings.chat.behavior.sendKey")}
          description={t("settings.chat.behavior.sendKeyDescription")}
          value={behavior.sendKey}
          options={sendKeyOptions}
          onChange={(value) => void update({ sendKey: value === "mod_enter" ? "mod_enter" : "enter" })}
          disabled={saving}
          canReset={changed(behavior, "sendKey")}
          onReset={() => reset("sendKey")}
        />
        <Select
          label={t("settings.chat.behavior.timelineDensity")}
          description={t("settings.chat.behavior.timelineDensityDescription")}
          value={behavior.timelineDensity}
          options={densityOptions}
          onChange={(value) => void update({ timelineDensity: readTimelineDensity(value) })}
          disabled={saving}
          canReset={changed(behavior, "timelineDensity")}
          onReset={() => reset("timelineDensity")}
        />
        <SwitchField label={t("settings.chat.behavior.restoreThread")} description={t("settings.chat.behavior.restoreThreadDescription")} checked={behavior.restoreLastSelectedThread} disabled={saving} onChange={(value) => void update({ restoreLastSelectedThread: value })} />
        <SwitchField label={t("settings.chat.behavior.reasoning")} description={t("settings.chat.behavior.reasoningDescription")} checked={behavior.showReasoningSummaries} disabled={saving} onChange={(value) => void update({ showReasoningSummaries: value })} />
        <SwitchField label={t("settings.chat.behavior.foldWork")} description={t("settings.chat.behavior.foldWorkDescription")} checked={behavior.automaticallyFoldSettledWork} disabled={saving} onChange={(value) => void update({ automaticallyFoldSettledWork: value })} />
      {/if}
    </div>
  </section>

  <div class="h-px shrink-0 bg-border/50" aria-hidden="true"></div>

  <ChatAppAgentSettings />

  {#if behavior}
    <div class="h-px shrink-0 bg-border/50" aria-hidden="true"></div>

    <section class="flex flex-col gap-4">
      <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.chat.behavior.sessionsHeading")}</h2>
      <div class="flex flex-col gap-3">
        <Select
          label={t("settings.chat.behavior.idleTimeout")}
          description={t("settings.chat.behavior.idleTimeoutDescription")}
          value={String(behavior.idleSessionTimeoutSeconds)}
          options={idleTimeoutOptions}
          onChange={(value) => void update({ idleSessionTimeoutSeconds: Number(value) })}
          disabled={saving}
          canReset={changed(behavior, "idleSessionTimeoutSeconds")}
          onReset={() => reset("idleSessionTimeoutSeconds")}
        />
        <Select
          label={t("settings.chat.behavior.terminalScrollback")}
          description={t("settings.chat.behavior.terminalScrollbackDescription")}
          value={String(behavior.terminalScrollbackLines)}
          options={scrollbackOptions}
          onChange={(value) => void update({ terminalScrollbackLines: Number(value) })}
          disabled={saving}
          canReset={changed(behavior, "terminalScrollbackLines")}
          onReset={() => reset("terminalScrollbackLines")}
        />
        <SwitchField label={t("settings.chat.behavior.confirmPaste")} description={t("settings.chat.behavior.confirmPasteDescription")} checked={behavior.confirmMultilineTerminalPaste} disabled={saving} onChange={(value) => void update({ confirmMultilineTerminalPaste: value })} />
      </div>
    </section>

    <div class="h-px shrink-0 bg-border/50" aria-hidden="true"></div>

    <section class="flex flex-col gap-4">
      <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.chat.behavior.notificationsHeading")}</h2>
      <div class="flex flex-col gap-3">
        <SwitchField label={t("settings.chat.behavior.notifyWaiting")} description={t("settings.chat.behavior.notifyWaitingDescription")} checked={behavior.notifyWaitingForYou} disabled={saving} onChange={(value) => void update({ notifyWaitingForYou: value })} />
        <SwitchField label={t("settings.chat.behavior.notifyReview")} description={t("settings.chat.behavior.notifyReviewDescription")} checked={behavior.notifyReviewReady} disabled={saving} onChange={(value) => void update({ notifyReviewReady: value })} />
        <SwitchField label={t("settings.chat.behavior.notifyFailures")} description={t("settings.chat.behavior.notifyFailuresDescription")} checked={behavior.notifyFailures} disabled={saving} onChange={(value) => void update({ notifyFailures: value })} />
        <SwitchField label={t("settings.chat.behavior.notificationContent")} description={t("settings.chat.behavior.notificationContentDescription")} checked={behavior.notificationIncludeContent} disabled={saving} onChange={(value) => void update({ notificationIncludeContent: value })} />
        <SwitchField label={t("settings.chat.behavior.announceBackground")} description={t("settings.chat.behavior.announceBackgroundDescription")} checked={behavior.announceBackgroundWork} disabled={saving} onChange={(value) => void update({ announceBackgroundWork: value })} />
      </div>
    </section>
  {:else}
    <Loading layout="rows" rows={3} />
  {/if}
</div>
