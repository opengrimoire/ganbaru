<script lang="ts">
  import {
    isProtectedDistractionsDesktopAppName,
    type DistractionsAppRule,
  } from "$lib/distractions";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getDistractions } from "$lib/stores/distractions.svelte";
  import { cn } from "$lib/utils";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import DistractionsAppSelector from "$lib/components/settings/distractions/DistractionsAppSelector.svelte";
  import DistractionsConfigurationSection from "./DistractionsConfigurationSection.svelte";
  import DistractionsRuleList from "./DistractionsRuleList.svelte";

  const distractions = getDistractions();
  const { t } = getLocalization();

  type DesktopListKind = "blocked";
  type DesktopConfigurationToggle = "enabled" | "focus" | "shortBreaks" | "longBreaks" | "pause";

  interface DistractionsAppSelection {
    name: string;
    matchNames: readonly string[];
  }

  interface DesktopListSection {
    kind: DesktopListKind;
    id: string;
    heading: string;
    description: string;
    placeholder: string;
    emptyText: string;
    errorText: string;
    apps: () => readonly DistractionsAppRule[];
    add: (text: string) => boolean;
    remove: (name: string) => void;
    setEnabled: (name: string, enabled: boolean) => void;
  }

  interface RuleListItem {
    id: string;
    label: string;
    enabled: boolean;
    locked?: boolean;
    stateLabel?: string;
  }

  interface PendingDesktopAppAction {
    type: "disable" | "delete";
    kind: DesktopListKind;
    name: string;
  }

  interface PendingDesktopConfigurationAction {
    toggle: DesktopConfigurationToggle;
  }

  type PendingAction =
    | { target: "app"; action: PendingDesktopAppAction }
    | { target: "desktopConfiguration"; action: PendingDesktopConfigurationAction };

  const appSections = {
    blocked: {
      kind: "blocked",
      id: "distractions-blocked-apps",
      heading: t("settings.distractions.desktop.blockedApps"),
      description: t("settings.distractions.desktop.blockedAppsDescription"),
      placeholder: t("settings.distractions.desktop.enterApp"),
      emptyText: t("settings.distractions.desktop.noBlockedApps"),
      errorText: t("settings.distractions.desktop.invalidApp"),
      apps: () => distractions.blockedApps,
      add: (text: string) => distractions.addBlockedAppsText(text),
      remove: (name: string) => distractions.removeBlockedApp(name),
      setEnabled: (name: string, enabled: boolean) => distractions.setBlockedAppEnabled(name, enabled),
    },
  } satisfies Record<DesktopListKind, DesktopListSection>;

  let pendingAction = $state<PendingAction | null>(null);
  let appPickerSection = $state<DesktopListKind | null>(null);
  const activePickerSection = $derived(appPickerSection ? appSections[appPickerSection] : null);

  function appItems(section: DesktopListSection): RuleListItem[] {
    const apps = section.apps()
      .filter((rule) => !isProtectedDistractionsDesktopAppName(rule.name))
      .map((rule) => ({
        id: rule.name.toLowerCase(),
        label: rule.name,
        enabled: rule.enabled,
      }));
    return apps;
  }

  function existingAppNames(section: DesktopListSection): string[] {
    return appItems(section).map((app) => app.label);
  }

  function pickerSection(): DesktopListSection | null {
    return activePickerSection;
  }

  function openAppPicker(kind: DesktopListKind): void {
    appPickerSection = kind;
  }

  function closeAppPicker(): void {
    appPickerSection = null;
  }

  function addPickedApp(app: DistractionsAppSelection): boolean {
    const section = pickerSection();
    return section ? distractions.addBlockedApp(app.name, app.matchNames) : false;
  }

  function requestAppEnabledChange(section: DesktopListSection, name: string, enabled: boolean): void {
    if (enabled) {
      section.setEnabled(name, true);
      return;
    }
    pendingAction = {
      target: "app",
      action: {
        type: "disable",
        kind: section.kind,
        name,
      },
    };
  }

  function requestAppDelete(section: DesktopListSection, name: string): void {
    pendingAction = {
      target: "app",
      action: {
        type: "delete",
        kind: section.kind,
        name,
      },
    };
  }

  function setDesktopConfigurationToggle(
    toggle: DesktopConfigurationToggle,
    checked: boolean,
  ): void {
    if (toggle === "enabled") {
      distractions.setDesktopEnabled(checked);
    } else if (toggle === "focus") {
      distractions.setDesktopBlockDuringFocus(checked);
    } else if (toggle === "shortBreaks") {
      distractions.setDesktopBlockDuringShortBreaks(checked);
    } else if (toggle === "longBreaks") {
      distractions.setDesktopBlockDuringLongBreaks(checked);
    } else {
      distractions.setDesktopPauseDuringFocusPause(checked);
    }
  }

  function requestDesktopConfigurationToggleChange(
    toggle: DesktopConfigurationToggle,
    checked: boolean,
  ): void {
    if (checked) {
      setDesktopConfigurationToggle(toggle, true);
      return;
    }
    pendingAction = { target: "desktopConfiguration", action: { toggle } };
  }

  function confirmPendingAction(): void {
    if (!pendingAction) return;
    if (pendingAction.target === "desktopConfiguration") {
      setDesktopConfigurationToggle(pendingAction.action.toggle, false);
    } else {
      const { type, kind, name } = pendingAction.action;
      const section = appSections[kind];
      if (type === "disable") {
        section.setEnabled(name, false);
      } else {
        section.remove(name);
      }
    }
    pendingAction = null;
  }

  function cancelPendingAction(): void {
    pendingAction = null;
  }

  function pendingActionTitle(action: PendingAction): string {
    if (action.target === "desktopConfiguration") {
      if (action.action.toggle === "enabled") return t("settings.distractions.desktop.turnOffTitle");
      if (action.action.toggle === "focus") return t("settings.distractions.desktop.allowAppsFocusTitle");
      if (action.action.toggle === "shortBreaks") return t("settings.distractions.desktop.allowAppsShortBreaksTitle");
      if (action.action.toggle === "longBreaks") return t("settings.distractions.desktop.allowAppsLongBreaksTitle");
      return t("settings.distractions.desktop.keepBlockingPausedTitle");
    }
    return action.action.type === "disable"
      ? t("settings.distractions.desktop.allowAppTitle", action.action.name)
      : t("settings.distractions.desktop.removeAppTitle", action.action.name);
  }

  function pendingActionMessage(action: PendingAction): string {
    if (action.target === "desktopConfiguration") {
      if (action.action.toggle === "enabled") {
        return t("settings.distractions.desktop.appOffMessage");
      }
      if (action.action.toggle === "focus") {
        return t("settings.distractions.desktop.focusOffMessage");
      }
      if (action.action.toggle === "shortBreaks") {
        return t("settings.distractions.desktop.shortBreaksOffMessage");
      }
      if (action.action.toggle === "longBreaks") {
        return t("settings.distractions.desktop.longBreaksOffMessage");
      }
      return t("settings.distractions.desktop.pauseActiveMessage");
    }
    return action.action.type === "disable"
      ? t("settings.distractions.desktop.appDisableMessage")
      : t("settings.distractions.desktop.removeMessage");
  }

  function pendingActionConfirmLabel(action: PendingAction): string {
    if (action.target === "desktopConfiguration") {
      return action.action.toggle === "enabled"
        ? t("settings.distractions.shared.turnOffAction")
        : t("settings.distractions.shared.allowAction");
    }
    return action.action.type === "disable"
      ? t("settings.distractions.shared.allowAction")
      : t("settings.distractions.shared.removeAction");
  }
</script>

<div class="flex flex-col gap-6">
  <DistractionsConfigurationSection
    title={t("settings.distractions.desktop.desktopConfiguration")}
    enabled={distractions.desktopEnabled}
    blockDuringFocus={distractions.desktopBlockDuringFocus}
    blockDuringShortBreaks={distractions.desktopBlockDuringShortBreaks}
    blockDuringLongBreaks={distractions.desktopBlockDuringLongBreaks}
    pauseDuringFocusPause={distractions.desktopPauseDuringFocusPause}
    showMode={false}
    enabledLabel={t("settings.distractions.desktop.enableDesktopBlocking")}
    enabledDescription={t("settings.distractions.desktop.enableDesktopBlockingDescription")}
    focusDescription={t("settings.distractions.desktop.focusDescription")}
    shortBreakDescription={t("settings.distractions.desktop.shortBreakDescription")}
    longBreakDescription={t("settings.distractions.desktop.longBreakDescription")}
    pauseDescription={t("settings.distractions.desktop.pauseDescription")}
    onScheduleChange={requestDesktopConfigurationToggleChange}
  />

  <fieldset
    disabled={!distractions.desktopEnabled}
    aria-disabled={!distractions.desktopEnabled}
    class={cn(
      "m-0 flex min-w-0 flex-col gap-6 border-0 p-0 transition-opacity",
      !distractions.desktopEnabled && "opacity-50",
    )}
  >
    <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

    <section class="flex flex-col gap-4">
      <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.distractions.desktop.blocklist")}</h2>
      <DistractionsRuleList
        id={appSections.blocked.id}
        heading={appSections.blocked.heading}
        description={appSections.blocked.description}
        placeholder={appSections.blocked.placeholder}
        emptyText={appSections.blocked.emptyText}
        errorText={appSections.blocked.errorText}
        items={appItems(appSections.blocked)}
        onAdd={appSections.blocked.add}
        onOpenSelector={() => openAppPicker("blocked")}
        selectorLabel={t("settings.distractions.desktop.addApp")}
        onEnabledChange={(name, enabled) => requestAppEnabledChange(appSections.blocked, name, enabled)}
        onDelete={(name) => requestAppDelete(appSections.blocked, name)}
      />
    </section>
  </fieldset>
</div>

{#if activePickerSection}
  <DistractionsAppSelector
    title={t("settings.distractions.desktop.chooseAppToBlock")}
    existingNames={existingAppNames(activePickerSection)}
    protectAppSelf
    onAdd={addPickedApp}
    onRemove={appSections.blocked.remove}
    onCancel={closeAppPicker}
  />
{/if}

{#if pendingAction}
  <ConfirmDialog
    title={pendingActionTitle(pendingAction)}
    message={pendingActionMessage(pendingAction)}
    confirmLabel={pendingActionConfirmLabel(pendingAction)}
    cancelLabel={t("settings.distractions.shared.cancelAction")}
    onConfirm={confirmPendingAction}
    onCancel={cancelPendingAction}
  />
{/if}
