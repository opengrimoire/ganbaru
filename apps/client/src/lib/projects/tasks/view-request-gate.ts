/** Rejects stale task-view responses after a newer request or explicit cancellation. */
export class ProjectTaskViewRequestGate {
  #generation = 0;

  begin(): number {
    this.#generation += 1;
    return this.#generation;
  }

  cancel(): void {
    this.#generation += 1;
  }

  isCurrent(generation: number): boolean {
    return generation === this.#generation;
  }
}
