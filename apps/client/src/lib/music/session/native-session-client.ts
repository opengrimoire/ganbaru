import { Channel } from "@tauri-apps/api/core";
import { musicSessionCommand, musicSessionObserve, musicSessionStart, musicSessionHost, musicSessionSubscribe, musicSessionReadFrame, musicSessionAcknowledge, musicSessionUnsubscribe } from "$lib/api/music-session";
import {
  parseNativeMusicFrame, parseNativeMusicNotice,
  type NativeMusicCommand, type NativeMusicEffect, type NativeMusicIntent, type NativeMusicObservation,
  type NativeMusicSnapshot, type NativeMusicStart,
} from "./native-session";

/** The native subscription lifecycle. Notices are accepted once subscribing starts. */
type NativeMusicConnection =
  | { phase: "idle" }
  | { phase: "connecting"; id: string; task: Promise<void> }
  | { phase: "connected"; id: string };

interface NativeMusicClientContext {
  snapshot(value: NativeMusicSnapshot): void;
  effect(value: NativeMusicEffect): Promise<void>;
  error(error: unknown): void;
  resetProjection?(): void;
}

/** Connects presentation to native state and preserves action identity across uncertain replies. */
export class NativeMusicSessionClient {
  private stream: Promise<void> = Promise.resolve();
  private sequence = 0;
  private pendingAcknowledgement: number | null = null;
  private pendingNotice: number | null = null;
  private acknowledging: Promise<void> | null = null;
  private connection: NativeMusicConnection = { phase: "idle" };
  private sessionId: string | null = null;
  private revision = -1;
  private generation = -1;
  private pending: NativeMusicCommand | NativeMusicStart | null = null;
  private actions: Promise<unknown> = Promise.resolve();
  private epoch = 0;

  constructor(private readonly context: NativeMusicClientContext) {}

  /** The native subscription includes an initial canonical snapshot before later transitions. */
  connect(available = true): Promise<void> {
    if (this.connection.phase === "connecting") return this.connection.task;
    if (this.connection.phase === "connected") return Promise.resolve();
    const id = crypto.randomUUID();
    // Notices may arrive while subscribing, so the connecting phase starts before the request.
    const connecting: NativeMusicConnection & { phase: "connecting" } = { phase: "connecting", id, task: Promise.resolve() };
    this.connection = connecting;
    connecting.task = this.open(id, this.epoch, available);
    void connecting.task.catch(() => undefined);
    return connecting.task;
  }

  private async open(id: string, epoch: number, available: boolean): Promise<void> {
    try {
      const channel = new Channel<unknown>((value) => {
        if (epoch !== this.epoch || !this.isOpen(id)) return;
        try {
          const notice = parseNativeMusicNotice(value);
          if (notice.subscriptionId !== id) throw new Error("Music notice belongs to another subscription");
          if (notice.kind === "invalidate") {
            const generation = this.generation;
            this.reset();
            void this.context.effect({ kind: "stop", generation: Math.max(0, generation) }).catch((error: unknown) => this.context.error(error));
            return;
          }
        } catch (error) { this.context.error(error); this.disconnect(); return; }
        this.stream = this.stream.then(async () => {
          if (epoch !== this.epoch || !this.isOpen(id)) return;
          await this.consume(value, id, epoch);
        }).catch((error: unknown) => {
          if (epoch !== this.epoch) return;
          this.context.error(error);
          this.disconnect();
        });
      });
      await musicSessionSubscribe(id, channel);
      if (epoch !== this.epoch || !this.isOpen(id)) {
        await musicSessionUnsubscribe(id);
        return;
      }
      await musicSessionHost(id, available);
      if (epoch === this.epoch && this.isOpen(id)) this.connection = { phase: "connected", id };
    } catch (error) { if (epoch === this.epoch) this.disconnect(); throw error; }
  }

  /** The subscription that accepts notices, while connecting or connected. */
  private get subscriptionId(): string | null {
    return this.connection.phase === "idle" ? null : this.connection.id;
  }

  private isOpen(id: string): boolean {
    return this.subscriptionId === id;
  }

  disconnect(): void {
    const id = this.subscriptionId;
    this.epoch += 1;
    this.connection = { phase: "idle" };
    this.sequence = 0;
    this.pendingAcknowledgement = null;
    this.pendingNotice = null;
    this.acknowledging = null;
    this.stream = Promise.resolve();
    if (id) void musicSessionUnsubscribe(id).catch((error: unknown) => this.context.error(error));
  }

  /** Drops old-vault presentation and pending requests without replaying them into another vault. */
  reset(): void {
    this.disconnect();
    this.sessionId = null; this.revision = -1; this.generation = -1;
    this.pending = null; this.actions = Promise.resolve();
    this.context.resetProjection?.();
  }

  private async consume(value: unknown, id: string, epoch: number): Promise<void> {
    const notice = parseNativeMusicNotice(value);
    if (notice.kind !== "update") throw new Error("Music frame requires an update notice");
    if (notice.subscriptionId !== id) throw new Error("Music frame belongs to another subscription");
    if (notice.sequence <= this.sequence) {
      this.pendingAcknowledgement = this.sequence;
      await this.acknowledge();
      return;
    }
    if (notice.sequence !== this.sequence + 1) throw new Error("Music stream skipped a sequence");
    this.pendingNotice = notice.sequence;
    let valueFromNative: unknown;
    try { valueFromNative = await musicSessionReadFrame(id, notice.sequence); }
    catch (error) {
      if (epoch === this.epoch) this.context.error(error);
      return;
    }
    if (epoch !== this.epoch || !this.isOpen(id)) return;
    const frame = parseNativeMusicFrame(valueFromNative);
    if (frame.subscriptionId !== id || frame.sequence !== notice.sequence) throw new Error("Music frame does not match its notice");
    this.pendingNotice = null;
    this.accept(frame.snapshot);
    try {
      if (frame.effects.length > 0 && frame.effectsExpireAtMs <= Date.now()) {
        await this.context.effect({ kind: "stop", generation: this.generation });
      } else {
        for (const effect of frame.effects) {
          if (epoch !== this.epoch || !this.isOpen(id)) return;
          if (effect.generation < this.generation) continue;
          this.generation = effect.generation;
          await this.context.effect(effect);
        }
      }
    } catch (error) {
      this.context.error(error);
      await this.context.effect({ kind: "stop", generation: this.generation });
      await musicSessionHost(id, false);
    }
    if (epoch !== this.epoch || !this.isOpen(id)) return;
    this.sequence = frame.sequence;
    this.pendingAcknowledgement = frame.sequence;
    await this.acknowledge();
  }

  private acknowledge(): Promise<void> {
    if (this.acknowledging) return this.acknowledging;
    const sequence = this.pendingAcknowledgement;
    const id = this.subscriptionId;
    const epoch = this.epoch;
    if (sequence === null || !id) return Promise.resolve();
    const task = musicSessionAcknowledge(id, sequence).then(() => {
      if (epoch === this.epoch && this.pendingAcknowledgement === sequence) this.pendingAcknowledgement = null;
    }).catch((error: unknown) => {
      if (epoch === this.epoch) this.context.error(error);
    });
    this.acknowledging = task;
    void task.finally(() => { if (this.acknowledging === task) this.acknowledging = null; });
    return task;
  }

  /** Renew presentation availability and retry lost acknowledgements without replaying effects. */
  async host(available: boolean): Promise<void> {
    await this.connect(available);
    const pendingNotice = this.pendingNotice;
    const expectedEpoch = this.epoch;
    if (pendingNotice !== null && this.subscriptionId) {
      const id = this.subscriptionId;
      this.stream = this.stream.then(async () => {
        if (this.pendingNotice === pendingNotice && this.epoch === expectedEpoch) {
          await this.consume({ kind: "update", subscriptionId: id, sequence: pendingNotice }, id, expectedEpoch);
        }
      }).catch((error: unknown) => {
        if (this.epoch === expectedEpoch) { this.context.error(error); this.disconnect(); }
      });
      await this.stream;
    }
    await this.acknowledge();
    const id = this.subscriptionId;
    if (!id) return;
    try { await musicSessionHost(id, available); }
    catch (error) {
      if (typeof error === "object" && error !== null && "code" in error && error.code === "not-found") {
        this.disconnect();
        await this.connect(available);
      } else { throw error; }
    }
  }

  private accept(snapshot: NativeMusicSnapshot): void {
    if (snapshot.sessionId === this.sessionId && snapshot.revision < this.revision) return;
    if (snapshot.generation < this.generation) return;
    this.sessionId = snapshot.sessionId;
    this.revision = snapshot.revision;
    this.generation = snapshot.generation;
    this.context.snapshot(snapshot);
  }

  private async executePending(): Promise<NativeMusicSnapshot> {
    const pending = this.pending;
    const epoch = this.epoch;
    if (!pending) throw new Error("Music action is missing");
    try {
      const result = "intent" in pending ? await musicSessionCommand(pending) : await musicSessionStart(pending);
      if (epoch !== this.epoch) throw new Error("Music vault changed while the command was pending");
      if (this.pending === pending) this.pending = null;
      this.accept(result);
      return result;
    } catch (error) {
      if (typeof error === "object" && error !== null && "code" in error
        && ["validation", "not-found", "stale-write", "conflict"].includes(String(error.code))
        && this.pending === pending) this.pending = null;
      throw error;
    }
  }

  private enqueue(action: () => NativeMusicCommand | NativeMusicStart): Promise<NativeMusicSnapshot> {
    const epoch = this.epoch;
    const task = this.actions.catch(() => undefined).then(async () => {
      if (epoch !== this.epoch) throw new Error("Music vault changed before the command was sent");
      // Resolve an earlier uncertain response before admitting a different action.
      if (this.pending) await this.executePending();
      if (epoch !== this.epoch) throw new Error("Music vault changed before the command was sent");
      this.pending = action();
      try { return await this.executePending(); }
      catch (error) { if (!this.pending) throw error; return this.executePending(); }
    });
    this.actions = task;
    return task;
  }

  command(intent: NativeMusicIntent): Promise<NativeMusicSnapshot> {
    return this.enqueue(() => ({ actionId: crypto.randomUUID(), sessionId: this.sessionId, intent }));
  }

  start(start: Omit<NativeMusicStart, "actionId">): Promise<NativeMusicSnapshot> {
    return this.enqueue(() => ({ ...start, actionId: crypto.randomUUID() }));
  }

  async observe(observation: NativeMusicObservation): Promise<void> {
    const id = this.subscriptionId;
    if (id) await musicSessionObserve(id, observation);
  }
}
