import type { LocalRootBinding, MusicInspectorDetail } from "$lib/music/library/contracts";
import {
  getMusicPlayer,
  type MusicReviewPlaybackCheckpoint,
} from "$lib/stores/music-player.svelte";

export const MUSIC_CONTEXT_BOUNDARY_EVENT = "ganbaru-ai-music-context-boundary";

export class MusicReviewAuditionController {
  active = $state(false);
  reviewItemId = $state<string | null>(null);
  error = $state<string | null>(null);
  private reviewPositions: Record<string, number> = {};
  private readonly player = getMusicPlayer();
  private returnPlayback: MusicReviewPlaybackCheckpoint | null = null;
  private restoreTask: Promise<void> | null = null;
  private operationGeneration = 0;

  get musicPlayer(): ReturnType<typeof getMusicPlayer> {
    return this.player;
  }

  get ownsPlayback(): boolean {
    return this.active && this.player.contextOwner === "review";
  }

  enter(): void {
    if (this.active) return;
    this.active = true;
  }

  async preview(
    detail: MusicInspectorDetail,
    _bindings: readonly LocalRootBinding[],
    autoplay: boolean,
  ): Promise<boolean> {
    if (this.restoreTask) await this.restoreTask;
    if (this.ownsPlayback && this.reviewItemId === detail.item.id) return true;
    const generation = ++this.operationGeneration;
    if (!this.returnPlayback) {
      this.enter();
      try {
        const checkpoint = await this.player.suspendForReview();
        if (generation !== this.operationGeneration) {
          await this.player.restoreAfterReview(checkpoint);
          return false;
        }
        this.returnPlayback = checkpoint;
      } catch (error) {
        if (generation === this.operationGeneration) {
          this.error = error instanceof Error ? error.message : String(error);
          this.clearOwnership();
        }
        return false;
      }
    }
    if (generation !== this.operationGeneration) return false;
    this.error = null;
    try {
      await this.player.loadReviewItem(detail.item.id, autoplay);
      if (generation !== this.operationGeneration) return false;
      this.player.contextOwner = "review";
      this.reviewItemId = detail.item.id;
      const retainedPosition = this.reviewPositions[detail.item.id] ?? 0;
      if (retainedPosition > 0) await this.player.seekToMs(retainedPosition);
      return true;
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      return false;
    }
  }

  async restore(): Promise<void> {
    if (this.restoreTask) return this.restoreTask;
    const checkpoint = this.returnPlayback;
    this.operationGeneration += 1;
    this.clearOwnership();
    this.returnPlayback = null;
    if (!checkpoint) return;
    const task = this.player.restoreAfterReview(checkpoint)
      .catch((error: unknown) => {
        this.error = error instanceof Error ? error.message : String(error);
      })
      .finally(() => {
        if (this.restoreTask === task) this.restoreTask = null;
      });
    this.restoreTask = task;
    await task;
  }

  discard(): void {
    this.operationGeneration += 1;
    this.returnPlayback = null;
    this.clearOwnership();
  }

  supersedeForBoundary(owner: "calendar-event" | "pomodoro"): void {
    if (this.ownsPlayback) {
      if (this.reviewItemId) this.reviewPositions[this.reviewItemId] = this.player.snapshot.positionMs;
      this.player.contextOwner = owner;
    }
    this.discard();
  }

  private clearOwnership(): void {
    this.active = false;
    this.reviewItemId = null;
    this.error = null;
  }
}

export function createMusicReviewAuditionController(): MusicReviewAuditionController {
  return new MusicReviewAuditionController();
}
