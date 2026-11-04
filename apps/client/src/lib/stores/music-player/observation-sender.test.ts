import { describe, expect, it } from "vitest";
import { MusicObservationSender } from "./observation-sender";

function deferred(): { promise: Promise<void>; resolve: () => void } {
  let resolve = () => {};
  const promise = new Promise<void>((done) => { resolve = done; });
  return { promise, resolve };
}

describe("MusicObservationSender", () => {
  it("sends the state that arrives while a send is in flight once that send finishes", async () => {
    const gates = [deferred(), deferred()];
    const sent: string[] = [];
    let state = "playing";
    const sender = new MusicObservationSender(async () => {
      sent.push(state);
      await gates[sent.length - 1]?.promise;
    });

    const progress = sender.request();
    state = "ended";
    const ended = sender.request();
    expect(sent).toEqual(["playing"]);

    gates[0]?.resolve();
    await new Promise((done) => setTimeout(done, 0));
    expect(sent).toEqual(["playing", "ended"]);
    gates[1]?.resolve();
    await Promise.all([progress, ended]);
  });

  it("folds several requests made during one send into a single follow-up with the newest state", async () => {
    const gate = deferred();
    const sent: number[] = [];
    let position = 0;
    let isFirstSend = true;
    const sender = new MusicObservationSender(async () => {
      sent.push(position);
      if (isFirstSend) {
        isFirstSend = false;
        await gate.promise;
      }
    });

    const first = sender.request();
    for (const next of [1, 2, 3]) {
      position = next;
      void sender.request();
    }
    gate.resolve();
    await first;

    expect(sent).toEqual([0, 3]);
  });

  it("starts a fresh send after an idle period", async () => {
    let sends = 0;
    const sender = new MusicObservationSender(async () => { sends += 1; });

    await sender.request();
    await sender.request();

    expect(sends).toBe(2);
  });

  it("recovers after a failed send", async () => {
    let shouldFail = true;
    let sends = 0;
    const sender = new MusicObservationSender(async () => {
      sends += 1;
      if (shouldFail) throw new Error("observe failed");
    });

    await expect(sender.request()).rejects.toThrow("observe failed");
    shouldFail = false;
    await sender.request();

    expect(sends).toBe(2);
  });
});
