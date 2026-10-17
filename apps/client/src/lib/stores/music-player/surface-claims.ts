interface SurfaceClaim {
  element: HTMLElement;
  priority: number;
  order: number;
}

/** Chooses one playback surface while panels and detached views mount independently. */
export class MusicSurfaceClaims {
  private readonly claims = new Map<string, SurfaceClaim>();
  private nextOrder = 0;

  constructor(private readonly apply: (element: HTMLElement | null) => void) {}

  claim(owner: string, element: HTMLElement, priority: number): () => void {
    this.claims.set(owner, { element, priority, order: ++this.nextOrder });
    this.sync();
    return () => {
      if (this.claims.get(owner)?.element !== element) return;
      this.claims.delete(owner);
      this.sync();
    };
  }

  private sync(): void {
    let active: SurfaceClaim | null = null;
    for (const claim of this.claims.values()) {
      if (!active || claim.priority > active.priority
        || (claim.priority === active.priority && claim.order > active.order)) active = claim;
    }
    this.apply(active?.element ?? null);
  }
}
