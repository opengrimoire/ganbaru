/** Owns the generation shared by every asynchronous source replacement path. */
export class MusicLoadRuntime {
  private generation = 0;

  begin(): number {
    this.generation += 1;
    return this.generation;
  }

  current(): number {
    return this.generation;
  }

  isCurrent(generation: number): boolean {
    return generation === this.generation;
  }
}
