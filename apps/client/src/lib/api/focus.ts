import { Channel, invoke } from "@tauri-apps/api/core";
import type { Translate } from "$lib/i18n/translator.svelte";
import {
  focusProjectionIsCurrent, parseFocusCommandResult, parseFocusNotice, parseFocusProjection,
  type FocusIntent, type FocusProjection, type FocusRequest,
} from "$lib/pomodoro/native-focus";

const RENEW_INTERVAL_MS = 10_000;

/** Localized desktop alert text contains no execution or scheduling input. */
export interface DesktopFocusNotificationCopy {
  endingWarningTitle: string;
  extendFocusLabel: string;
  pausedReminderTitle: string;
  pausedReminderBody: string;
  resumeFocusLabel: string;
  dismissPromptsLabel: string;
}

/** Build alert text from the current typed locale catalog. */
export function buildDesktopFocusNotificationCopy(t: Translate): DesktopFocusNotificationCopy {
  return {
    endingWarningTitle: t("pomodoroNotification.endingWarningTitle"),
    extendFocusLabel: t("pomodoroNotification.extendFocusLabel"),
    pausedReminderTitle: t("pomodoroNotification.pausedReminderTitle"),
    pausedReminderBody: t("pomodoroNotification.pausedReminderBody"),
    resumeFocusLabel: t("pomodoroNotification.resumeFocusLabel"),
    dismissPromptsLabel: t("pomodoroNotification.dismissPromptsLabel"),
  };
}

/** Identity of the accepted phase that created a detached control surface. */
export interface FocusControlScope {
  vaultGeneration: number;
  runId: string;
  segmentId: string;
}

/** Presentation identifies the idle episode; the native owner records its clock. */
export interface FocusIdleOverlayScope extends FocusControlScope {
  idleDetectedAtMs: number;
}

/** Acknowledge a painted native warning or its authorized main-window fallback. */
export function reportIdleOverlayVisible(scope: FocusIdleOverlayScope): Promise<void> {
  return invoke("focus_idle_overlay_visible", { scope });
}

/** One window's command and presentation connection. Native code owns execution. */
export class NativeFocusClient {
  private projection: FocusProjection | null = null;
  private initialization: Promise<void> | null = null;
  private commandTail: Promise<void> = Promise.resolve();
  private reading: Promise<FocusProjection> | null = null;
  private renewTimer: ReturnType<typeof setInterval> | null = null;
  private subscriptionId: string | null = null;
  private channel: Channel<unknown> | null = null;
  private uncertain: FocusRequest | null = null;

  constructor(private readonly publish: (projection: FocusProjection) => void, private readonly fail: (error: unknown) => void) {}

  private accept(projection: FocusProjection): void {
    if (!focusProjectionIsCurrent(this.projection, projection)) return;
    this.projection = projection;
    this.publish(projection);
  }

  /** Start a leased native-window-bound stream and read canonical startup state. */
  initialize(): Promise<void> {
    if (this.initialization) return this.initialization;
    this.initialization = (async () => {
      const id = crypto.randomUUID();
      const channel = new Channel<unknown>();
      channel.onmessage = (value) => {
        try {
          const notice = parseFocusNotice(value);
          if (!notice.available && this.projection && notice.vaultGeneration >= this.projection.vaultGeneration) {
            this.accept({ ...this.projection,
              vaultId: notice.vaultGeneration === this.projection.vaultGeneration ? this.projection.vaultId : null,
              vaultGeneration: notice.vaultGeneration, snapshot: null });
          }
          void this.refresh().catch(this.fail);
        } catch (error) { this.fail(error); }
      };
      await invoke("focus_subscribe", { subscriptionId: id, channel });
      this.subscriptionId = id;
      this.channel = channel;
      this.renewTimer = setInterval(() => { void this.renew().catch(this.fail); }, RENEW_INTERVAL_MS);
      await this.refresh();
    })().catch(async (error: unknown) => { await this.dispose().catch(this.fail); throw error; });
    return this.initialization;
  }

  /** Coalesce simultaneous presentation reads without creating execution work. */
  refresh(): Promise<FocusProjection> {
    if (this.reading) return this.reading;
    this.reading = invoke<unknown>("focus_snapshot").then(parseFocusProjection).then((projection) => {
      this.accept(projection); return projection;
    }).finally(() => { this.reading = null; });
    return this.reading;
  }

  private async renew(): Promise<void> {
    const id = this.subscriptionId;
    if (!id) return;
    try {
      this.accept(parseFocusProjection(await invoke<unknown>("focus_renew_subscription", { subscriptionId: id })));
    } catch (error) {
      await this.dispose();
      this.fail(error);
      await this.initialize();
    }
  }

  /** Serialize this window's user actions; uncertain replies reuse one command identity. */
  command(intent: FocusIntent, scope?: FocusControlScope): Promise<void> {
    const displayed = this.projection;
    if (!displayed?.vaultId || !displayed.snapshot) return Promise.reject(new Error("Native Focus is unavailable"));
    if (scope && (scope.vaultGeneration !== displayed.vaultGeneration
      || scope.runId !== displayed.snapshot.run?.id || scope.segmentId !== displayed.snapshot.segment?.id)) {
      return Promise.reject(new Error("This Focus control belongs to a previous phase"));
    }
    const request: FocusRequest = {
      vaultId: displayed.vaultId, vaultGeneration: displayed.vaultGeneration,
      command: { commandId: crypto.randomUUID(), expectedRevision: displayed.snapshot.revision, intent },
    };
    const task = this.commandTail.then(async () => {
      await this.initialize();
      if (this.uncertain) {
        if (this.projection?.vaultId !== this.uncertain.vaultId) {
          this.uncertain = null;
          throw new Error("An unresolved Focus action belongs to a previous vault");
        }
        await this.execute(this.uncertain);
      }
      await this.execute(request);
    });
    this.commandTail = task.catch(this.fail);
    return task;
  }

  private async execute(request: FocusRequest): Promise<void> {
    for (let attempt = 0; attempt < 2; attempt += 1) {
      try {
        const result = parseFocusCommandResult(await invoke<unknown>("focus_command", { request }), request.command.expectedRevision);
        this.uncertain = null;
        this.accept(result.projection);
        return;
      } catch (error) {
        const code = typeof error === "object" && error !== null && "code" in error ? error.code : null;
        const uncertain = code === null || code === "persistence" || code === "unavailable";
        if (attempt === 1 || !uncertain) {
          this.uncertain = uncertain ? request : null;
          await this.refresh().catch(this.fail);
          throw error;
        }
      }
    }
  }

  /** Drop presentation only; closing a WebView cannot stop accepted native execution. */
  async dispose(): Promise<void> {
    const id = this.subscriptionId;
    this.subscriptionId = null;
    this.initialization = null;
    if (this.renewTimer !== null) clearInterval(this.renewTimer);
    this.renewTimer = null;
    if (this.channel) this.channel.onmessage = () => {};
    this.channel = null;
    if (id) await invoke("focus_unsubscribe", { subscriptionId: id });
  }
}
