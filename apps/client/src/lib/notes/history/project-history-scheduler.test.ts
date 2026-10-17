import { describe, expect, it, vi } from "vitest";
import type { NotesProjectHistorySchedule } from "$lib/api/notes/project-history";
import type { SchedulerClock, SchedulerTimeout } from "$lib/scheduling/lifecycle-scheduler";
import { createNotesProjectHistoryScheduler } from "./project-history-scheduler";

class FakeClock implements SchedulerClock {
  private nextId = 1;
  private timers = new Map<number, { at: number; callback: () => void }>();

  constructor(private nowMs = 0) {}

  now(): number {
    return this.nowMs;
  }

  setTimeout(callback: () => void, delayMs: number): SchedulerTimeout {
    const id = this.nextId++;
    this.timers.set(id, { at: this.nowMs + Math.max(0, delayMs), callback });
    return id as unknown as SchedulerTimeout;
  }

  clearTimeout(timeout: SchedulerTimeout): void {
    this.timers.delete(timeout as unknown as number);
  }

  async advanceTo(nowMs: number): Promise<void> {
    while (true) {
      const next = [...this.timers.entries()]
        .filter(([, timer]) => timer.at <= nowMs)
        .sort((left, right) => left[1].at - right[1].at || left[0] - right[0])[0];
      if (!next) break;
      const [id, timer] = next;
      this.timers.delete(id);
      this.nowMs = timer.at;
      timer.callback();
      await settle();
    }
    this.nowMs = nowMs;
    await settle();
  }
}

async function settle(): Promise<void> {
  for (let index = 0; index < 8; index += 1) await Promise.resolve();
}

function schedule(nextCheckpointAt: string | null = null): NotesProjectHistorySchedule {
  return {
    createdCount: 0,
    nextCheckpointAt,
    nextMaintenanceAt: new Date(24 * 60 * 60 * 1_000).toISOString(),
  };
}

describe("Notes project history scheduler", () => {
  it("wakes for dirty deadlines, vault changes, resume, and shutdown only", async () => {
    const clock = new FakeClock();
    const flush = vi.fn().mockResolvedValue(schedule());
    const scheduler = createNotesProjectHistoryScheduler({ clock, flush });

    scheduler.setEnabled(true);
    await settle();
    expect(flush).toHaveBeenCalledOnce();

    scheduler.applyMutationDeadline(new Date(180_000).toISOString());
    await clock.advanceTo(179_999);
    expect(flush).toHaveBeenCalledOnce();
    await clock.advanceTo(180_000);
    expect(flush).toHaveBeenCalledTimes(2);

    scheduler.applyMutationDeadline(new Date(180_000).toISOString());
    await clock.advanceTo(180_000);
    expect(flush).toHaveBeenCalledTimes(3);

    scheduler.switchVault();
    await settle();
    expect(flush).toHaveBeenCalledTimes(4);
    scheduler.resume();
    await settle();
    expect(flush).toHaveBeenCalledTimes(5);
    await scheduler.shutdown();
    expect(flush).toHaveBeenCalledTimes(6);
  });

  it("uses an authoritative recovered dirty deadline on startup", async () => {
    const clock = new FakeClock();
    const flush = vi.fn()
      .mockResolvedValueOnce(schedule(new Date(90_000).toISOString()))
      .mockResolvedValue(schedule());
    const scheduler = createNotesProjectHistoryScheduler({ clock, flush });

    scheduler.setEnabled(true);
    await settle();
    await clock.advanceTo(89_999);
    expect(flush).toHaveBeenCalledOnce();
    await clock.advanceTo(90_000);
    expect(flush).toHaveBeenCalledTimes(2);
  });
});
