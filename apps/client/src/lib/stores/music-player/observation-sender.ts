/**
 * Sends browser playback observations one at a time without losing the latest one.
 *
 * A request made while a send is in flight is folded into one follow-up send of the state at that
 * moment, so a terminal state such as an ended track always reaches the session even when it
 * arrives right after a progress report. The send callback handles its own errors.
 */
export class MusicObservationSender {
  private active: Promise<void> | null = null;
  private pending = false;

  constructor(private readonly send: () => Promise<void>) {}

  /** Sends the current state, resolving once it, or a newer state, has been sent. */
  request(): Promise<void> {
    if (this.active) {
      this.pending = true;
      return this.active;
    }
    this.active = this.drain();
    return this.active;
  }

  private async drain(): Promise<void> {
    try {
      do {
        this.pending = false;
        await this.send();
      } while (this.pending);
    } finally {
      this.active = null;
    }
  }
}
