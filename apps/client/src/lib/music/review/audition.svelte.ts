import type { LocalRootBinding, MusicInspectorDetail } from "$lib/music/library/contracts";
import { getMusicPlayer } from "$lib/stores/music-player.svelte";
import { musicErrorMessage } from "$lib/music/user-errors";
import { translate } from "$lib/i18n/translator.svelte";
import {
  IDLE_MUSIC_AUDITION,
  transitionMusicAudition,
  type MusicAuditionEvent,
  type MusicAuditionState,
} from "./audition-state";

export const MUSIC_CONTEXT_BOUNDARY_EVENT = "ganbaru-music-context-boundary";

export class MusicReviewAuditionController {
  error = $state<string | null>(null);
  private state = $state.raw<MusicAuditionState>(IDLE_MUSIC_AUDITION);
  private reviewPositions: Record<string, number> = {};
  private readonly player = getMusicPlayer();
  private restoreTask: Promise<void> | null = null;
  private operationGeneration = 0;

  get musicPlayer(): ReturnType<typeof getMusicPlayer> {
    return this.player;
  }

  /** Whether Review has suspended, or is suspending, the playback it interrupts. */
  get active(): boolean {
    return this.state.phase !== "idle";
  }

  /** The item Review last loaded, or `null` before one loads. */
  get reviewItemId(): string | null {
    return this.state.phase === "auditioning" ? this.state.itemId : null;
  }

  get ownsPlayback(): boolean {
    return this.active && this.player.contextOwner === "review";
  }

  async preview(
    detail: MusicInspectorDetail,
    _bindings: readonly LocalRootBinding[],
    autoplay: boolean,
  ): Promise<boolean> {
    if (this.restoreTask) await this.restoreTask;
    if (this.ownsPlayback && this.reviewItemId === detail.item.id) return true;
    const generation = ++this.operationGeneration;
    if (this.state.phase !== "auditioning") {
      this.dispatch({ type: "suspendStarted" });
      try {
        const checkpoint = await this.player.suspendForReview();
        if (generation !== this.operationGeneration) {
          await this.player.restoreAfterReview(checkpoint);
          return false;
        }
        this.dispatch({ type: "suspended", checkpoint });
      } catch (error) {
        if (generation === this.operationGeneration) {
          this.error = musicErrorMessage(error, translate("music.errors.playerFailed"));
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
      this.dispatch({ type: "itemLoaded", itemId: detail.item.id });
      const retainedPosition = this.reviewPositions[detail.item.id] ?? 0;
      if (retainedPosition > 0) await this.player.seekToMs(retainedPosition);
      return true;
    } catch (error) {
      this.error = musicErrorMessage(error, translate("music.errors.playerFailed"));
      return false;
    }
  }

  async restore(): Promise<void> {
    if (this.restoreTask) return this.restoreTask;
    const checkpoint = this.state.phase === "auditioning" ? this.state.checkpoint : null;
    this.operationGeneration += 1;
    this.clearOwnership();
    if (!checkpoint) return;
    const task = this.player.restoreAfterReview(checkpoint)
      .catch((error: unknown) => {
        this.error = musicErrorMessage(error, translate("music.errors.playerFailed"));
      })
      .finally(() => {
        if (this.restoreTask === task) this.restoreTask = null;
      });
    this.restoreTask = task;
    await task;
  }

  discard(): void {
    this.operationGeneration += 1;
    this.clearOwnership();
  }

  supersedeForBoundary(owner: "calendar-event" | "pomodoro"): void {
    if (this.ownsPlayback) {
      if (this.reviewItemId) this.reviewPositions[this.reviewItemId] = this.player.snapshot.positionMs;
      this.player.contextOwner = owner;
    }
    this.discard();
  }

  private dispatch(event: MusicAuditionEvent): void {
    this.state = transitionMusicAudition(this.state, event);
  }

  private clearOwnership(): void {
    this.dispatch({ type: "ended" });
    this.error = null;
  }
}

export function createMusicReviewAuditionController(): MusicReviewAuditionController {
  return new MusicReviewAuditionController();
}
